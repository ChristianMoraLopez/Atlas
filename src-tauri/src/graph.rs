use crate::{
    error::{AppError, Context, Result},
    models::{ExtractionResult, Interaction, SourceKind},
    ollama::{self, ChatEvidence},
    state::AppState,
};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use url::Url;

const GRAPH_ROOT: &str = "https://graph.microsoft.com/v1.0";

#[derive(Deserialize)]
struct Page<T> {
    value: Vec<T>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphDateTime {
    date_time: String,
    #[allow(dead_code)]
    time_zone: String,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct EmailAddress {
    name: Option<String>,
    address: Option<String>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Recipient {
    email_address: EmailAddress,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarEvent {
    id: String,
    subject: Option<String>,
    is_cancelled: Option<bool>,
    start: GraphDateTime,
    end: GraphDateTime,
    organizer: Option<Recipient>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MailMessage {
    id: String,
    subject: Option<String>,
    received_date_time: String,
    sender: Option<Recipient>,
    is_draft: Option<bool>,
}

#[derive(Deserialize)]
struct Chat {
    id: String,
    topic: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatMessage {
    id: String,
    created_date_time: String,
    message_type: Option<String>,
    body: Option<MessageBody>,
    from: Option<MessageFrom>,
}
#[derive(Deserialize)]
struct MessageBody {
    content: Option<String>,
}
#[derive(Deserialize)]
struct MessageFrom {
    user: Option<MessageUser>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageUser {
    display_name: Option<String>,
}

pub(crate) fn day_bounds(date: &str, timezone: &str) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").context("Choose a valid workday")?;
    let tz: Tz = timezone.parse().unwrap_or(chrono_tz::UTC);
    let start_local = tz
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .ok_or_else(|| {
            AppError::Message("The selected date has an ambiguous timezone boundary.".into())
        })?;
    let next = date
        .succ_opt()
        .ok_or_else(|| AppError::Message("The selected workday is out of range.".into()))?;
    let end_local = tz
        .from_local_datetime(&next.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .ok_or_else(|| {
            AppError::Message("The selected date has an ambiguous timezone boundary.".into())
        })?;
    Ok((
        start_local.with_timezone(&Utc),
        end_local.with_timezone(&Utc),
    ))
}

pub(crate) fn parse_graph_time(value: &str) -> Result<DateTime<Utc>> {
    if let Ok(value) = DateTime::parse_from_rfc3339(value) {
        return Ok(value.with_timezone(&Utc));
    }
    let normalized = format!("{}Z", value.trim_end_matches('Z'));
    DateTime::parse_from_rfc3339(&normalized)
        .map(|v| v.with_timezone(&Utc))
        .context("Microsoft Graph returned an invalid date/time")
}

fn graph_url(path: &str, pairs: &[(&str, String)]) -> Result<Url> {
    let mut url =
        Url::parse(&format!("{GRAPH_ROOT}{path}")).context("Invalid Microsoft Graph URL")?;
    url.query_pairs_mut()
        .extend_pairs(pairs.iter().map(|(k, v)| (*k, v.as_str())));
    Ok(url)
}

fn validate_next_link(value: &str) -> Result<Url> {
    let url = Url::parse(value).context("Microsoft Graph returned an invalid paging URL")?;
    if url.scheme() != "https" || url.host_str() != Some("graph.microsoft.com") {
        return Err(AppError::Message(
            "Microsoft Graph returned an unsafe paging destination.".into(),
        ));
    }
    Ok(url)
}

async fn graph_get<T: serde::de::DeserializeOwned>(
    state: &AppState,
    token: &str,
    url: Url,
) -> Result<T> {
    let response = state
        .http
        .get(url)
        .bearer_auth(token)
        .header("Prefer", "outlook.timezone=\"UTC\"")
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Message(format!(
            "Microsoft Graph returned {status}: {}",
            body.chars().take(350).collect::<String>()
        )));
    }
    response
        .json()
        .await
        .context("Microsoft Graph returned invalid data")
}

pub(crate) fn excluded_subject(subject: &str) -> bool {
    let clean = subject
        .to_lowercase()
        .replace(['-', '_', ':'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    clean == "lunch"
        || clean == "almuerzo"
        || clean.contains("tracker time")
        || clean.contains("hora del tracker")
}

pub(crate) fn classify_client(address: Option<&str>) -> (String, String) {
    let domain = address
        .and_then(|v| v.split('@').nth(1))
        .unwrap_or("")
        .to_ascii_lowercase();
    if domain.contains("circana") {
        ("Circana".into(), String::new())
    } else if domain.contains("capgemini") {
        ("Capgemini".into(), String::new())
    } else if !domain.is_empty() {
        ("End_Client".into(), String::new())
    } else {
        (String::new(), String::new())
    }
}

pub async fn extract(
    state: &AppState,
    token: &str,
    date: &str,
    timezone: &str,
    include_email: bool,
    include_teams: bool,
    model: &str,
) -> Result<ExtractionResult> {
    let (start, end) = day_bounds(date, timezone)?;
    let mut interactions = calendar(state, token, start, end, timezone).await?;
    let mut warnings = Vec::new();
    if include_email {
        interactions.extend(mail(state, token, start, end).await?);
    }
    if include_teams {
        match teams(state, token, start, end, model).await {
            Ok(values) => interactions.extend(values),
            Err(e) => warnings.push(format!("Teams suggestions were skipped: {e}")),
        }
    }
    interactions.sort_by(|a, b| a.interaction_date_time.cmp(&b.interaction_date_time));
    Ok(ExtractionResult {
        interactions,
        warnings,
    })
}

async fn calendar(
    state: &AppState,
    token: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    timezone: &str,
) -> Result<Vec<Interaction>> {
    let mut url = graph_url(
        "/me/calendarView",
        &[
            ("startDateTime", start.to_rfc3339()),
            ("endDateTime", end.to_rfc3339()),
            (
                "$select",
                "id,subject,isCancelled,start,end,organizer".into(),
            ),
            ("$orderby", "start/dateTime".into()),
            ("$top", "100".into()),
        ],
    )?;
    let mut events = Vec::new();
    loop {
        let page: Page<CalendarEvent> = graph_get(state, token, url).await?;
        events.extend(page.value);
        match page.next_link {
            Some(next) => url = validate_next_link(&next)?,
            None => break,
        }
    }
    let first_mmni_id = events
        .iter()
        .filter(|e| {
            !e.is_cancelled.unwrap_or(false)
                && !excluded_subject(e.subject.as_deref().unwrap_or(""))
        })
        .filter(|e| {
            let s = e.subject.as_deref().unwrap_or("").to_lowercase();
            s.contains("mmni") || s.contains("sparktriage")
        })
        .filter_map(|e| {
            parse_graph_time(&e.start.date_time)
                .ok()
                .map(|t| (t, e.id.clone()))
        })
        .min_by_key(|v| v.0)
        .map(|v| v.1);
    let mut result = Vec::new();
    let target_tz: Tz = timezone.parse().unwrap_or(chrono_tz::UTC);
    for event in events {
        let subject = event.subject.unwrap_or_else(|| "Untitled meeting".into());
        if event.is_cancelled.unwrap_or(false) || excluded_subject(&subject) {
            continue;
        }
        let mut event_start = parse_graph_time(&event.start.date_time)?;
        let mut event_end = parse_graph_time(&event.end.date_time)?;
        if first_mmni_id.as_deref() == Some(event.id.as_str()) {
            let day = event_start.with_timezone(&target_tz).date_naive();
            event_start = target_tz
                .from_local_datetime(&day.and_hms_opt(9, 0, 0).unwrap())
                .single()
                .unwrap()
                .with_timezone(&Utc);
            event_end = target_tz
                .from_local_datetime(&day.and_hms_opt(9, 30, 0).unwrap())
                .single()
                .unwrap()
                .with_timezone(&Utc);
        }
        let organizer = event
            .organizer
            .as_ref()
            .and_then(|v| v.email_address.address.as_deref());
        let (client_type, end_client) = classify_client(organizer);
        let resolved = event_end <= Utc::now();
        result.push(Interaction {
            source_kind: SourceKind::Calendar,
            source_id: format!("graph:calendar:{}", event.id),
            interaction_type: "Meeting".into(),
            reception_date_time: event_start.to_rfc3339(),
            interaction_date_time: event_start.to_rfc3339(),
            resolution_date_time: resolved.then(|| event_end.to_rfc3339()),
            client_type,
            end_client,
            status: if resolved { "Resolved" } else { "In Progress" }.into(),
            resolution_type: if resolved { "Processed & Resolved" } else { "" }.into(),
            category: String::new(),
            subcategory: String::new(),
            priority: "Intermediate".into(),
            incident_number: String::new(),
            comments: subject.clone(),
            selected: true,
            reviewed: true,
            manual_authored: false,
            ai_suggested: false,
            evidence_label: subject,
        });
    }
    Ok(result)
}

async fn mail(
    state: &AppState,
    token: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<Interaction>> {
    let filter = format!(
        "receivedDateTime ge {} and receivedDateTime lt {}",
        start.to_rfc3339(),
        end.to_rfc3339()
    );
    let mut url = graph_url(
        "/me/messages",
        &[
            ("$filter", filter),
            (
                "$select",
                "id,subject,receivedDateTime,sender,isDraft".into(),
            ),
            ("$orderby", "receivedDateTime asc".into()),
            ("$top", "100".into()),
        ],
    )?;
    let mut result = Vec::new();
    loop {
        let page: Page<MailMessage> = graph_get(state, token, url).await?;
        for message in page.value {
            if message.is_draft.unwrap_or(false) {
                continue;
            }
            let received = parse_graph_time(&message.received_date_time)?;
            let subject = message.subject.unwrap_or_else(|| "No subject".into());
            let sender = message
                .sender
                .as_ref()
                .and_then(|v| v.email_address.address.as_deref());
            let (client_type, end_client) = classify_client(sender);
            result.push(Interaction {
                source_kind: SourceKind::Email,
                source_id: format!("graph:mail:{}", message.id),
                interaction_type: "E-Mail".into(),
                reception_date_time: received.to_rfc3339(),
                interaction_date_time: received.to_rfc3339(),
                resolution_date_time: Some(received.to_rfc3339()),
                client_type,
                end_client,
                status: "Resolved".into(),
                resolution_type: "Processed & Resolved".into(),
                category: String::new(),
                subcategory: String::new(),
                priority: "Intermediate".into(),
                incident_number: String::new(),
                comments: subject.clone(),
                selected: false,
                reviewed: false,
                manual_authored: false,
                ai_suggested: false,
                evidence_label: subject,
            });
        }
        match page.next_link {
            Some(next) => url = validate_next_link(&next)?,
            None => break,
        }
    }
    Ok(result)
}

async fn teams(
    state: &AppState,
    token: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    model: &str,
) -> Result<Vec<Interaction>> {
    let mut chats_url = graph_url(
        "/me/chats",
        &[("$select", "id,topic".into()), ("$top", "50".into())],
    )?;
    let mut chats = Vec::new();
    loop {
        let page: Page<Chat> = graph_get(state, token, chats_url).await?;
        chats.extend(page.value);
        match page.next_link {
            Some(next) => chats_url = validate_next_link(&next)?,
            None => break,
        }
    }
    let tags = Regex::new(r"(?s)<[^>]*>").unwrap();
    let mut evidence = Vec::new();
    let mut topics = Vec::new();
    for chat in chats {
        let path = format!(
            "/chats/{}/messages",
            url::form_urlencoded::byte_serialize(chat.id.as_bytes()).collect::<String>()
        );
        let mut message_url = graph_url(
            &path,
            &[
                ("$top", "50".into()),
                ("$orderby", "lastModifiedDateTime desc".into()),
            ],
        )?;
        let mut pages = 0;
        'paging: loop {
            pages += 1;
            let page: Page<ChatMessage> = graph_get(state, token, message_url).await?;
            let mut older_than_day = false;
            for message in page.value {
                let created = parse_graph_time(&message.created_date_time)?;
                if created < start {
                    older_than_day = true;
                }
                if created < start
                    || created >= end
                    || message
                        .message_type
                        .as_deref()
                        .is_some_and(|v| v != "message")
                {
                    continue;
                }
                let html = message.body.and_then(|v| v.content).unwrap_or_default();
                let text = html_escape::decode_html_entities(&tags.replace_all(&html, " "))
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if text.is_empty() {
                    continue;
                }
                let author = message
                    .from
                    .and_then(|v| v.user)
                    .and_then(|v| v.display_name)
                    .unwrap_or_else(|| "Unknown participant".into());
                topics.push(chat.topic.clone().unwrap_or_else(|| "Teams chat".into()));
                evidence.push(ChatEvidence {
                    id: message.id,
                    author,
                    created,
                    text,
                });
            }
            if older_than_day || pages >= 20 {
                break 'paging;
            }
            match page.next_link {
                Some(next) => message_url = validate_next_link(&next)?,
                None => break 'paging,
            }
        }
    }
    let suggestions = ollama::summarize(&state.http, model, &evidence).await?;
    let real_ids: HashSet<&str> = evidence.iter().map(|e| e.id.as_str()).collect();
    let mut result = Vec::new();
    for suggestion in suggestions {
        if !suggestion
            .source_ids
            .iter()
            .all(|id| real_ids.contains(id.as_str()))
        {
            continue;
        }
        let digest = hex::encode(Sha256::digest(suggestion.source_ids.join("|").as_bytes()));
        let participants = suggestion.participants.join(", ");
        result.push(Interaction {
            source_kind: SourceKind::TeamsChat,
            source_id: format!("graph:teams:{digest}"),
            interaction_type: "Task".into(),
            reception_date_time: suggestion.start.to_rfc3339(),
            interaction_date_time: suggestion.start.to_rfc3339(),
            resolution_date_time: Some(suggestion.end.to_rfc3339()),
            client_type: String::new(),
            end_client: String::new(),
            status: "Resolved".into(),
            resolution_type: "Processed & Resolved".into(),
            category: String::new(),
            subcategory: String::new(),
            priority: "Intermediate".into(),
            incident_number: String::new(),
            comments: suggestion.summary,
            selected: false,
            reviewed: false,
            manual_authored: false,
            ai_suggested: true,
            evidence_label: if participants.is_empty() {
                "Teams messages".into()
            } else {
                format!("Teams messages with {participants}")
            },
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn business_exclusions_cover_required_variants() {
        for value in [
            "Lunch",
            "almuerzo",
            "Tracker-Time",
            "Weekly TRACKER_time",
            "Hora del Tracker",
        ] {
            assert!(excluded_subject(value), "{value}");
        }
        assert!(!excluded_subject("Customer triage"));
    }
    #[test]
    fn graph_paging_never_leaves_microsoft_graph() {
        assert!(validate_next_link("https://graph.microsoft.com/v1.0/me/messages?$skip=1").is_ok());
        assert!(validate_next_link("https://evil.example/me/messages").is_err());
    }
}
