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
    let mut latest_other_date: Option<(DateTime<Utc>, String)> = None;
    let mut unavailable_files = 0usize;
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
        if metadata.len() == 0 {
            unavailable_files += 1;
            continue;
        }
        if metadata.len() > MAX_BUNDLE_BYTES {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(value) => value,
            Err(_) => {
                unavailable_files += 1;
                continue;
            }
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
        if bundle.schema_version != 2 {
            continue;
        }
        let exported = match parse_graph_time(&bundle.exported_at) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if bundle.target_date != date {
            let replace = latest_other_date
                .as_ref()
                .is_none_or(|(current, _)| exported > *current);
            if replace {
                latest_other_date = Some((exported, bundle.target_date));
            }
            continue;
        }
        candidates.push((exported, path, bundle));
    }
    if candidates.is_empty() {
        let detail = if unavailable_files > 0 {
            format!(
                "Hay {unavailable_files} archivo(s) de OneDrive sin descargar; reanuda OneDrive y marca la carpeta como 'Siempre mantener en este dispositivo'."
            )
        } else if let Some((exported, target)) = latest_other_date {
            format!(
                "El paquete válido más reciente corresponde a {target} y fue exportado a las {}. El flujo no ha entregado el día solicitado; comprueba que 'Atlas - Export evidence to OneDrive' esté activo y que OneDrive esté sincronizando.",
                exported.to_rfc3339()
            )
        } else {
            "No hay ningún paquete Atlas válido. Comprueba que el flujo esté activo y que OneDrive esté sincronizando.".into()
        };
        return Err(AppError::Message(format!(
            "No se encontró un paquete de evidencia para {date} en {}. {detail}",
            folder.display()
        )));
    }

    // A connector may time out for one source while another run succeeds. Start with
    // the newest package, then recover only unavailable sources from older packages
    // for the same day. A successful empty source remains authoritative.
    candidates.sort_by(|left, right| right.0.cmp(&left.0));
    let (_, path, mut merged) = candidates.remove(0);
    let mut calendar_selected = !merged.calendar.is_empty();
    let mut mail_selected = !merged.mail.is_empty();
    // Teams event capture writes small, independent packages during the day. Keep
    // the newest scheduled snapshot for calendar and mail, but union every Teams
    // message for the requested day so activity from different chats is retained.
    let mut teams_ready = merged.sources.teams;
    let mut seen_teams = merged
        .teams
        .iter()
        .map(|message| {
            if message.chat_id.trim().is_empty() {
                message.id.clone()
            } else {
                format!("{}:{}", message.chat_id, message.id)
            }
        })
        .collect::<HashSet<_>>();
    for (_, _, mut candidate) in candidates {
        if !calendar_selected && !candidate.calendar.is_empty() {
            merged.sources.calendar = candidate.sources.calendar;
            merged.calendar = std::mem::take(&mut candidate.calendar);
            calendar_selected = true;
        }
        if !mail_selected && !candidate.mail.is_empty() {
            merged.sources.mail = candidate.sources.mail;
            merged.mail = std::mem::take(&mut candidate.mail);
            mail_selected = true;
        }
        teams_ready |= candidate.sources.teams;
        for message in candidate.teams {
            let key = if message.chat_id.trim().is_empty() {
                message.id.clone()
            } else {
                format!("{}:{}", message.chat_id, message.id)
            };
            if seen_teams.insert(key) {
                merged.teams.push(message);
            }
        }
    }
    merged.sources.teams = teams_ready;
    Ok((path, merged))
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

fn teams_rows(
    messages: Vec<BridgeTeamsMessage>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<Interaction>> {
    let tags = Regex::new(r"(?s)<[^>]*>").unwrap();
    let mut seen = HashSet::new();
    let mut groups: HashMap<String, Vec<(String, DateTime<Utc>, String, String)>> = HashMap::new();
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
        let chat_key = if message.chat_id.trim().is_empty() {
            evidence_id.clone()
        } else {
            message.chat_id.clone()
        };
        labels.entry(chat_key.clone()).or_insert(label);
        groups
            .entry(chat_key)
            .or_default()
            .push((evidence_id, created, author, text));
    }
    let mut rows = Vec::new();
    for (chat_key, mut messages) in groups {
        messages.sort_by_key(|message| message.1);
        let start = messages.first().unwrap().1;
        let finish = messages.last().unwrap().1;
        let ids = messages
            .iter()
            .map(|message| message.0.as_str())
            .collect::<Vec<_>>()
            .join("|");
        let digest = hex::encode(Sha256::digest(ids.as_bytes()));
        let mut participants = messages
            .iter()
            .map(|message| message.2.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        participants.sort();
        let comments = messages
            .iter()
            .take(8)
            .map(|message| format!("{}: {}", message.2, message.3))
            .collect::<Vec<_>>()
            .join(" | ")
            .chars()
            .take(2_000)
            .collect::<String>();
        let topic = labels
            .get(&chat_key)
            .cloned()
            .unwrap_or_else(|| "Teams chat".into());
        let evidence_label = if participants.is_empty() {
            topic
        } else {
            format!("{topic} · {}", participants.join(", "))
        };
        rows.push(Interaction {
            source_kind: SourceKind::TeamsChat,
            source_id: format!("bridge:teams:{digest}"),
            interaction_type: "Task".into(),
            reception_date_time: start.to_rfc3339(),
            interaction_date_time: start.to_rfc3339(),
            resolution_date_time: Some(finish.to_rfc3339()),
            client_type: String::new(),
            end_client: String::new(),
            status: "Resolved".into(),
            resolution_type: "Processed & Resolved".into(),
            category: String::new(),
            subcategory: String::new(),
            priority: "Intermediate".into(),
            incident_number: String::new(),
            comments,
            selected: false,
            reviewed: false,
            manual_authored: false,
            ai_suggested: false,
            evidence_label,
        });
    }
    Ok(rows)
}

async fn interpret_teams(state: &AppState, rows: &[Interaction]) -> Result<Vec<Interaction>> {
    let evidence = rows
        .iter()
        .map(|row| {
            Ok(ChatEvidence {
                id: row.source_id.clone(),
                author: row.evidence_label.clone(),
                created: parse_graph_time(&row.interaction_date_time)?,
                text: row.comments.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let labels: HashMap<_, _> = rows
        .iter()
        .map(|row| (row.source_id.as_str(), row.evidence_label.as_str()))
        .collect();
    let suggestions = state
        .local_ai
        .summarize(&state.http, ollama::BUNDLED_MODEL, &evidence)
        .await?;
    Ok(suggestions
        .into_iter()
        .map(|suggestion| {
            let digest = hex::encode(Sha256::digest(suggestion.source_ids.join("|").as_bytes()));
            let sources = suggestion
                .source_ids
                .iter()
                .filter_map(|id| labels.get(id.as_str()))
                .copied()
                .collect::<Vec<_>>()
                .join(", ");
            Interaction {
                source_kind: SourceKind::TeamsChat,
                source_id: format!("bridge:teams:ai:{digest}"),
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
                evidence_label: if sources.is_empty() {
                    format!("AI review · {}", suggestion.participants.join(", "))
                } else {
                    format!("AI review · {sources}")
                },
            }
        })
        .collect())
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
    crate::diagnostics::info(
        "bridge/sources",
        &format!(
            "Selected calendar={}, mail={}, Teams={} evidence items for {date}",
            bundle.calendar.len(),
            bundle.mail.len(),
            bundle.teams.len()
        ),
    );
    let (start, end) = day_bounds(date, timezone)?;
    let source_status = &bundle.sources;
    let mut warnings = Vec::new();
    for (ready, count, label) in [
        (source_status.calendar, bundle.calendar.len(), "calendario"),
        (source_status.mail, bundle.mail.len(), "correo"),
        (source_status.teams, bundle.teams.len(), "Teams"),
    ] {
        if !ready && count == 0 {
            warnings.push(format!("Power Automate no pudo leer {label} en esta ejecución. Revisa la conexión y añade una interacción real manualmente si falta."));
        } else if !ready {
            warnings.push(format!("Power Automate solo pudo leer parte de {label} antes de agotar el tiempo. Revisa los elementos recibidos y añade manualmente lo que falte."));
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
        match teams_rows(std::mem::take(&mut bundle.teams), start, end) {
            Ok(values) => match interpret_teams(state, &values).await {
                Ok(suggestions) if !suggestions.is_empty() => interactions.extend(suggestions),
                Ok(_) => {
                    warnings.push("La IA local no identificó una interacción laboral en Teams. Revisa la evidencia directa.".into());
                    interactions.extend(values);
                }
                Err(error) => {
                    crate::diagnostics::error(
                        "bridge/local-ai",
                        &format!("Teams interpretation failed: {error}"),
                    );
                    warnings.push(format!("La interpretación local de Teams falló: {error}. Atlas conserva la evidencia directa para revisión."));
                    interactions.extend(values);
                }
            },
            Err(error) => warnings.push(format!(
                "Los mensajes de Teams de {} se omitieron: {error}",
                path.display()
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
    fn teams_messages_become_deterministic_reviewable_evidence_without_ai() {
        let start = parse_graph_time("2026-09-07T00:00:00Z").unwrap();
        let end = parse_graph_time("2026-09-08T00:00:00Z").unwrap();
        let rows = teams_rows(
            vec![BridgeTeamsMessage {
                id: "message-1".into(),
                chat_id: "chat-1".into(),
                topic: "Incident review".into(),
                created_date_time: "2026-09-07T16:00:00Z".into(),
                author: "Colleague".into(),
                content: "<p>Reviewed incident 42</p>".into(),
                message_type: "message".into(),
            }],
            start,
            end,
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].interaction_type, "Task");
        assert!(!rows[0].selected);
        assert!(!rows[0].ai_suggested);
        assert_eq!(rows[0].comments, "Colleague: Reviewed incident 42");
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

    #[test]
    fn unavailable_source_is_recovered_from_an_older_same_day_bundle() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("older.json"),
            r#"{"schemaVersion":2,"exportedAt":"2026-09-07T10:00:00Z","targetDate":"2026-09-07","sources":{"calendar":false,"mail":false,"teams":false},"calendar":[],"mail":[],"teams":[{"id":"team-1","chatId":"chat-1","topic":"Review","createdDateTime":"2026-09-07T09:30:00Z","author":"Colleague","content":"Reviewed task","messageType":"message"}]}"#,
        )
        .unwrap();
        fs::write(
            directory.path().join("newer.json"),
            r#"{"schemaVersion":2,"exportedAt":"2026-09-07T11:00:00Z","targetDate":"2026-09-07","sources":{"calendar":true,"mail":true,"teams":true},"calendar":[],"mail":[{"id":"mail-1","subject":"Follow-up","receivedDateTime":"2026-09-07T10:30:00Z","senderAddress":"client@example.com","isDraft":false}],"teams":[]}"#,
        )
        .unwrap();

        let (path, bundle) = newest_bundle(directory.path(), "2026-09-07").unwrap();
        assert_eq!(path.file_name().unwrap(), "newer.json");
        assert_eq!(bundle.mail.len(), 1);
        assert_eq!(bundle.teams.len(), 1);
        assert!(bundle.sources.teams);
    }

    #[test]
    fn teams_event_packages_are_unioned_and_deduplicated_for_the_day() {
        let directory = tempfile::tempdir().unwrap();
        for (name, exported_at, chat_id, message_id, content) in [
            (
                "first.json",
                "2026-09-07T10:00:00Z",
                "chat-1",
                "message-1",
                "First",
            ),
            (
                "second.json",
                "2026-09-07T11:00:00Z",
                "chat-2",
                "message-2",
                "Second",
            ),
            (
                "duplicate.json",
                "2026-09-07T12:00:00Z",
                "chat-1",
                "message-1",
                "First",
            ),
        ] {
            let bundle = serde_json::json!({
                "schemaVersion": 2,
                "exportedAt": exported_at,
                "targetDate": "2026-09-07",
                "sources": { "calendar": false, "mail": false, "teams": true },
                "calendar": [],
                "mail": [],
                "teams": [{
                    "id": message_id,
                    "chatId": chat_id,
                    "topic": "",
                    "createdDateTime": "2026-09-07T09:30:00Z",
                    "author": "Colleague",
                    "content": content,
                    "messageType": "message"
                }]
            });
            fs::write(
                directory.path().join(name),
                serde_json::to_vec(&bundle).unwrap(),
            )
            .unwrap();
        }

        let (path, bundle) = newest_bundle(directory.path(), "2026-09-07").unwrap();
        assert_eq!(path.file_name().unwrap(), "duplicate.json");
        assert_eq!(bundle.teams.len(), 2);
        assert!(bundle.sources.teams);
        assert!(bundle
            .teams
            .iter()
            .any(|message| message.chat_id == "chat-2"));
    }

    #[test]
    fn missing_day_reports_the_latest_exported_day() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("previous.json"),
            r#"{"schemaVersion":2,"exportedAt":"2026-09-11T23:33:00Z","targetDate":"2026-09-11","sources":{"calendar":true,"mail":true,"teams":true},"calendar":[],"mail":[],"teams":[]}"#,
        )
        .unwrap();

        let error = newest_bundle(directory.path(), "2026-09-12").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("2026-09-11"));
        assert!(message.contains("flujo no ha entregado"));
    }
}
