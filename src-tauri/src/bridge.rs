use crate::{
    error::{AppError, Context, Result},
    evidence_validation,
    graph::{classify_client, day_bounds, excluded_subject, parse_graph_time},
    models::{ExtractionResult, Interaction, SourceKind},
    ollama::{self, ChatEvidence},
    state::AppState,
};
use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

const MAX_BUNDLE_BYTES: u64 = 25 * 1024 * 1024;
const MAX_CALENDAR_ITEMS: usize = 500;
const MAX_MAIL_ITEMS: usize = 2_000;
const MAX_TEAMS_ITEMS: usize = 20_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceBundle {
    schema_version: u32,
    exported_at: String,
    target_date: String,
    sources: BridgeSourceStatus,
    #[serde(default)]
    calendar: Vec<BridgeCalendarEvent>,
    #[serde(default)]
    mail: Vec<BridgeMailMessage>,
    #[serde(default)]
    teams: Vec<BridgeTeamsMessage>,
}

#[derive(Debug, Deserialize)]
struct BridgeSourceStatus {
    calendar: bool,
    mail: bool,
    teams: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BridgeCalendarEvent {
    id: String,
    #[serde(default)]
    subject: String,
    start: String,
    end: String,
    #[serde(default)]
    organizer_address: String,
    #[serde(default)]
    is_cancelled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BridgeMailMessage {
    id: String,
    #[serde(default)]
    subject: String,
    received_date_time: String,
    #[serde(default)]
    sender_address: String,
    #[serde(default)]
    is_draft: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BridgeTeamsMessage {
    id: String,
    #[serde(default)]
    chat_id: String,
    #[serde(default)]
    topic: String,
    created_date_time: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    content: String,
    #[serde(default = "default_message_type")]
    message_type: String,
}

fn default_message_type() -> String {
    "message".into()
}

fn newest_bundle(folder: &Path, date: &str) -> Result<(PathBuf, EvidenceBundle)> {
    if !folder.is_dir() {
        return Err(AppError::Message(format!(
            "The Power Automate inbox no longer exists: {}",
            folder.display()
        )));
    }
    let mut candidates = Vec::new();
    for entry in fs::read_dir(folder).context("Unable to read the Power Automate inbox")? {
        let entry = entry.context("Unable to inspect a Power Automate inbox item")?;
        let file_type = entry
            .file_type()
            .context("Unable to inspect a Power Automate inbox item type")?;
        let path = entry.path();
        let is_json = path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("json"));
        if !file_type.is_file() || !is_json {
            continue;
        }
        let metadata = entry
            .metadata()
            .context("Unable to inspect a Power Automate evidence file")?;
        if metadata.len() == 0 || metadata.len() > MAX_BUNDLE_BYTES {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let value: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !evidence_validation::valid(&value) {
            continue;
        }
        let bundle: EvidenceBundle = match serde_json::from_value(value) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if bundle.schema_version != 2 || bundle.target_date != date {
            continue;
        }
        let exported = match parse_graph_time(&bundle.exported_at) {
            Ok(value) => value,
            Err(_) => continue,
        };
        candidates.push((exported, path, bundle));
    }
    candidates
        .into_iter()
        .max_by_key(|(exported, _, _)| *exported)
        .map(|(_, path, bundle)| (path, bundle))
        .ok_or_else(|| {
            AppError::Message(format!(
                "No valid Atlas evidence package for {date} was found in {}. Run the Power Automate export flow and wait for OneDrive to finish syncing.",
                folder.display()
            ))
        })
}

fn validate_limits(bundle: &EvidenceBundle) -> Result<()> {
    for (label, actual, maximum) in [
        ("calendar", bundle.calendar.len(), MAX_CALENDAR_ITEMS),
        ("mail", bundle.mail.len(), MAX_MAIL_ITEMS),
        ("Teams", bundle.teams.len(), MAX_TEAMS_ITEMS),
    ] {
        if actual > maximum {
            return Err(AppError::Message(format!(
                "The Power Automate package contains too many {label} items ({actual}; maximum {maximum})."
            )));
        }
    }
    Ok(())
}

fn calendar_rows(
    events: Vec<BridgeCalendarEvent>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    timezone: &str,
) -> Result<Vec<Interaction>> {
    let mut seen = HashSet::new();
    let mut parsed = Vec::new();
    for event in events {
        if event.id.trim().is_empty() || !seen.insert(event.id.clone()) {
            continue;
        }
        let event_start = parse_graph_time(&event.start)?;
        let event_end = parse_graph_time(&event.end)?;
        if event_start < end && event_end > start {
            parsed.push((event, event_start, event_end));
        }
    }
    let first_mmni_id = parsed
        .iter()
        .filter(|(event, _, _)| !event.is_cancelled && !excluded_subject(&event.subject))
        .filter(|(event, _, _)| {
            let subject = event.subject.to_lowercase();
            subject.contains("mmni") || subject.contains("sparktriage")
        })
        .min_by_key(|(_, event_start, _)| *event_start)
        .map(|(event, _, _)| event.id.clone());
    let target_tz: Tz = timezone.parse().unwrap_or(chrono_tz::UTC);
    let mut rows = Vec::new();
    for (event, mut event_start, mut event_end) in parsed {
        let subject = if event.subject.trim().is_empty() {
            "Untitled meeting".to_string()
        } else {
            event.subject.trim().to_string()
        };
        if event.is_cancelled || excluded_subject(&subject) {
            continue;
        }
        if first_mmni_id.as_deref() == Some(event.id.as_str()) {
            let day = event_start.with_timezone(&target_tz).date_naive();
            event_start = target_tz
                .from_local_datetime(&day.and_hms_opt(9, 0, 0).unwrap())
                .single()
                .ok_or_else(|| {
                    AppError::Message("The MMNI time is ambiguous in the selected timezone.".into())
                })?
                .with_timezone(&Utc);
            event_end = target_tz
                .from_local_datetime(&day.and_hms_opt(9, 30, 0).unwrap())
                .single()
                .ok_or_else(|| {
                    AppError::Message(
                        "The MMNI end time is ambiguous in the selected timezone.".into(),
                    )
                })?
                .with_timezone(&Utc);
        }
        let (client_type, end_client) = classify_client(Some(&event.organizer_address));
        let resolved = event_end <= Utc::now();
        rows.push(Interaction {
            source_kind: SourceKind::Calendar,
            source_id: format!("bridge:calendar:{}", event.id),
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
    Ok(rows)
}

fn mail_rows(
    messages: Vec<BridgeMailMessage>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<Interaction>> {
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for message in messages {
        if message.id.trim().is_empty() || message.is_draft || !seen.insert(message.id.clone()) {
            continue;
        }
        let received = parse_graph_time(&message.received_date_time)?;
        if received < start || received >= end {
            continue;
        }
        let subject = if message.subject.trim().is_empty() {
            "No subject".to_string()
        } else {
            message.subject.trim().to_string()
        };
        let (client_type, end_client) = classify_client(Some(&message.sender_address));
        rows.push(Interaction {
            source_kind: SourceKind::Email,
            source_id: format!("bridge:mail:{}", message.id),
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
    Ok(rows)
}

async fn teams_rows(
    state: &AppState,
    messages: Vec<BridgeTeamsMessage>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<Interaction>> {
    let tags = Regex::new(r"(?s)<[^>]*>").unwrap();
    let mut seen = HashSet::new();
    let mut evidence = Vec::new();
    let mut labels = HashMap::new();
    for message in messages {
        if message.id.trim().is_empty() || !message.message_type.eq_ignore_ascii_case("message") {
            continue;
        }
        let evidence_id = if message.chat_id.trim().is_empty() {
            message.id.clone()
        } else {
            format!("{}:{}", message.chat_id, message.id)
        };
        if !seen.insert(evidence_id.clone()) {
            continue;
        }
        let created = parse_graph_time(&message.created_date_time)?;
        if created < start || created >= end {
            continue;
        }
        let text = html_escape::decode_html_entities(&tags.replace_all(&message.content, " "))
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            continue;
        }
        let author = if message.author.trim().is_empty() {
            "Unknown participant".to_string()
        } else {
            message.author.trim().to_string()
        };
        let label = if message.topic.trim().is_empty() {
            if message.chat_id.trim().is_empty() {
                "Teams chat".to_string()
            } else {
                format!("Teams chat {}", message.chat_id)
            }
        } else {
            message.topic.trim().to_string()
        };
        labels.insert(evidence_id.clone(), label);
        evidence.push(ChatEvidence {
            id: evidence_id,
            author,
            created,
            text,
        });
    }
    let suggestions = ollama::summarize(&state.http, ollama::BUNDLED_MODEL, &evidence).await?;
    let real_ids: HashSet<&str> = evidence.iter().map(|value| value.id.as_str()).collect();
    let mut rows = Vec::new();
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
        let mut topics = suggestion
            .source_ids
            .iter()
            .filter_map(|id| labels.get(id))
            .cloned()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        topics.sort();
        let topics = topics.join(", ");
        rows.push(Interaction {
            source_kind: SourceKind::TeamsChat,
            source_id: format!("bridge:teams:{digest}"),
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
                topics
            } else if topics.is_empty() {
                format!("Teams messages with {participants}")
            } else {
                format!("{topics} · {participants}")
            },
        });
    }
    Ok(rows)
}

pub async fn extract(
    state: &AppState,
    folder: &Path,
    date: &str,
    timezone: &str,
    include_email: bool,
    include_teams: bool,
) -> Result<ExtractionResult> {
    let (path, mut bundle) = newest_bundle(folder, date)?;
    validate_limits(&bundle)?;
    let (start, end) = day_bounds(date, timezone)?;
    let source_status = &bundle.sources;
    let mut warnings = Vec::new();
    for (ready, label) in [
        (source_status.calendar, "calendario"),
        (source_status.mail, "correo"),
        (source_status.teams, "Teams"),
    ] {
        if !ready {
            warnings.push(format!(
                "Power Automate no pudo leer {label} en esta ejecución. Revisa la conexión y añade una interacción real manualmente si falta."
            ));
        }
    }
    if source_status.calendar && bundle.calendar.is_empty() {
        warnings.push(
            "No se encontraron reuniones de hoy; añade una real manualmente si corresponde.".into(),
        );
    }
    if source_status.mail && bundle.mail.is_empty() {
        warnings.push(
            "No se encontraron correos de hoy; añade uno real manualmente si corresponde.".into(),
        );
    }
    if source_status.teams && bundle.teams.is_empty() {
        warnings.push("No se encontraron mensajes de Teams de hoy; añade una interacción real manualmente si corresponde.".into());
    }
    let mut interactions = calendar_rows(bundle.calendar, start, end, timezone)?;
    if include_email {
        interactions.extend(mail_rows(bundle.mail, start, end)?);
    }
    if include_teams && !bundle.teams.is_empty() {
        match state.local_ai.ensure_ready(&state.http).await {
            Ok(()) => match teams_rows(state, std::mem::take(&mut bundle.teams), start, end).await {
                Ok(values) => interactions.extend(values),
                Err(error) => warnings.push(format!(
                    "Las sugerencias de Teams de {} se omitieron: {error}",
                    path.display()
                )),
            },
            Err(error) => warnings.push(format!(
                "Teams se recibió, pero el análisis local no está disponible: {error}. Añade una tarea real manualmente."
            )),
        }
    }
    interactions.sort_by(|a, b| a.interaction_date_time.cmp(&b.interaction_date_time));
    crate::diagnostics::info(
        "bridge/import",
        &format!(
            "Imported {} verified interactions from {}",
            interactions.len(),
            path.display()
        ),
    );
    Ok(ExtractionResult {
        interactions,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn bridge_calendar_and_mail_keep_existing_selection_rules() {
        let start = parse_graph_time("2026-09-07T00:00:00Z").unwrap();
        let end = parse_graph_time("2026-09-08T00:00:00Z").unwrap();
        let calendar = calendar_rows(
            vec![BridgeCalendarEvent {
                id: "event-1".into(),
                subject: "Customer review".into(),
                start: "2026-09-07T14:00:00Z".into(),
                end: "2026-09-07T15:00:00Z".into(),
                organizer_address: "owner@circana.com".into(),
                is_cancelled: false,
            }],
            start,
            end,
            "UTC",
        )
        .unwrap();
        let mail = mail_rows(
            vec![BridgeMailMessage {
                id: "mail-1".into(),
                subject: "Incident follow-up".into(),
                received_date_time: "2026-09-07T16:00:00Z".into(),
                sender_address: "client@example.com".into(),
                is_draft: false,
            }],
            start,
            end,
        )
        .unwrap();
        assert!(calendar[0].selected);
        assert!(!mail[0].selected);
        assert_eq!(calendar[0].source_id, "bridge:calendar:event-1");
        assert_eq!(mail[0].client_type, "End_Client");
    }

    #[test]
    fn newest_valid_bundle_is_selected_for_the_day() {
        let directory = tempfile::tempdir().unwrap();
        for (name, exported_at) in [
            ("older.json", "2026-09-07T10:00:00Z"),
            ("newer.json", "2026-09-07T11:00:00Z"),
        ] {
            let mut file = NamedTempFile::new_in(directory.path()).unwrap();
            write!(
                file,
                r#"{{"schemaVersion":2,"exportedAt":"{exported_at}","targetDate":"2026-09-07","sources":{{"calendar":true,"mail":true,"teams":true}},"calendar":[],"mail":[],"teams":[]}}"#
            )
            .unwrap();
            file.persist(directory.path().join(name)).unwrap();
        }
        let (path, _) = newest_bundle(directory.path(), "2026-09-07").unwrap();
        assert_eq!(path.file_name().unwrap(), "newer.json");
    }
}
