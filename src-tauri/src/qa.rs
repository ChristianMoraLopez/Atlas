//! QA Audit module: pulls mail evidence from the manager mailbox for the
//! configured auditees, evaluates each conversation against the QA rubric
//! with the bundled local model, and writes cumulative per-analyst Excel
//! workbooks (one sheet per audit date). Fully additive: it never touches
//! the interactions tracker pipeline.

use crate::{
    diagnostics,
    error::{AppError, Context, Result},
    models::{QaAuditee, QaCase, QaConfig, QaEvidenceRef, QaExtractionResult, QaExportResult, QaWatchStatus},
    state::AppState,
};
use base64::Engine;
use chrono::{DateTime, Datelike, Duration, Utc};
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fs, io::Read, path::{Path, PathBuf}};
use url::Url;

const GRAPH_ROOT: &str = "https://graph.microsoft.com/v1.0";
const MAX_CONVERSATIONS_PER_AUDITEE: usize = 10;
const MAX_CONVERSATIONS_HISTORICAL: usize = 50;
const MAX_MESSAGES_PER_CONVERSATION: usize = 40;
const MAX_ATTACHMENT_BYTES: i64 = 8 * 1024 * 1024;
const BODY_BUDGET: usize = 1200;
const ATTACHMENT_BUDGET: usize = 1500;
const TRANSCRIPT_BUDGET: usize = 6000;

// ---------------------------------------------------------------------------
// Graph payloads
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Page<T> {
    value: Vec<T>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
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
struct ItemBody {
    content: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QaMailMessage {
    id: String,
    subject: Option<String>,
    conversation_id: Option<String>,
    received_date_time: Option<String>,
    from: Option<Recipient>,
    body: Option<ItemBody>,
    has_attachments: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentMeta {
    id: String,
    name: Option<String>,
    content_type: Option<String>,
    size: Option<i64>,
    #[serde(rename = "@odata.type")]
    odata_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentFull {
    content_bytes: Option<String>,
}

// ---------------------------------------------------------------------------
// HTTP helpers (mirrors the safety rules in graph.rs)
// ---------------------------------------------------------------------------

fn qa_url(path: &str, pairs: &[(&str, String)]) -> Result<Url> {
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

fn escape_filter(value: &str) -> String {
    value.replace('\'', "''")
}

fn parse_time(value: &str) -> Result<DateTime<Utc>> {
    crate::graph::parse_graph_time(value)
}

fn html_to_text(html: &str) -> String {
    let tags = Regex::new(r"(?s)<[^>]*>").unwrap();
    html_escape::decode_html_entities(&tags.replace_all(html, " "))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Evidence collection
// ---------------------------------------------------------------------------

struct MessageEvidence {
    id: String,
    author: String,
    when: DateTime<Utc>,
    subject: String,
    text: String,
    attachments: Vec<(String, String)>,
}

struct ConversationEvidence {
    conversation_id: String,
    subject: String,
    messages: Vec<MessageEvidence>,
}

fn subject_matches(subject: &str, keywords: &[String]) -> bool {
    let lower = subject.to_ascii_lowercase();
    keywords
        .iter()
        .map(|k| k.trim().to_ascii_lowercase())
        .filter(|k| !k.is_empty())
        .any(|k| lower.contains(&k))
}

async fn list_auditee_conversations(
    state: &AppState,
    token: &str,
    auditee: &QaAuditee,
    start: Option<DateTime<Utc>>,
    cap: usize,
    keywords: &[String],
) -> Result<Vec<String>> {
    let filter = match start {
        Some(start) => format!(
            "from/emailAddress/address eq '{}' and receivedDateTime ge {}",
            escape_filter(&auditee.email),
            start.to_rfc3339()
        ),
        None => format!(
            "from/emailAddress/address eq '{}'",
            escape_filter(&auditee.email)
        ),
    };
    let mut url = qa_url(
        "/me/messages",
        &[
            ("$filter", filter),
            ("$select", "id,subject,conversationId,receivedDateTime".into()),
            ("$orderby", "receivedDateTime desc".into()),
            ("$top", "50".into()),
        ],
    )?;
    let mut conversations = Vec::new();
    let mut seen = std::collections::HashSet::new();
    loop {
        let page: Page<QaMailMessage> = graph_get(state, token, url).await?;
        for message in page.value {
            // Only QA-related topics are audited (e.g. subject mentions "QA").
            if !subject_matches(message.subject.as_deref().unwrap_or(""), keywords) {
                continue;
            }
            if let Some(id) = message.conversation_id {
                if seen.insert(id.clone()) {
                    conversations.push(id);
                }
            }
        }
        if conversations.len() >= cap {
            break;
        }
        match page.next_link {
            Some(next) => url = validate_next_link(&next)?,
            None => break,
        }
    }
    conversations.truncate(cap);
    Ok(conversations)
}

async fn fetch_conversation(
    state: &AppState,
    token: &str,
    conversation_id: &str,
    start: Option<DateTime<Utc>>,
) -> Result<ConversationEvidence> {
    let filter = match start {
        Some(start) => format!(
            "conversationId eq '{}' and receivedDateTime ge {}",
            escape_filter(conversation_id),
            start.to_rfc3339()
        ),
        None => format!("conversationId eq '{}'", escape_filter(conversation_id)),
    };
    let mut url = qa_url(
        "/me/messages",
        &[
            ("$filter", filter),
            (
                "$select",
                "id,subject,conversationId,receivedDateTime,from,body,hasAttachments".into(),
            ),
            ("$orderby", "receivedDateTime asc".into()),
            ("$top", "50".into()),
        ],
    )?;
    let mut raw = Vec::new();
    loop {
        let page: Page<QaMailMessage> = graph_get(state, token, url).await?;
        raw.extend(page.value);
        if raw.len() >= MAX_MESSAGES_PER_CONVERSATION {
            break;
        }
        match page.next_link {
            Some(next) => url = validate_next_link(&next)?,
            None => break,
        }
    }
    raw.truncate(MAX_MESSAGES_PER_CONVERSATION);
    let mut messages = Vec::new();
    let mut subject = String::new();
    for message in raw {
        let Some(received) = message.received_date_time.as_deref() else {
            continue;
        };
        let when = parse_time(received)?;
        let message_subject = message.subject.unwrap_or_else(|| "No subject".into());
        if subject.is_empty() {
            subject = message_subject.clone();
        }
        let author = message
            .from
            .as_ref()
            .map(|r| {
                let name = r.email_address.name.clone().unwrap_or_default();
                let address = r.email_address.address.clone().unwrap_or_default();
                if name.is_empty() {
                    address
                } else {
                    format!("{name} <{address}>")
                }
            })
            .unwrap_or_else(|| "Unknown sender".into());
        let text = message
            .body
            .and_then(|b| b.content)
            .map(|html| html_to_text(&html))
            .unwrap_or_default();
        let mut attachments = Vec::new();
        if message.has_attachments.unwrap_or(false) {
            attachments = fetch_attachment_texts(state, token, &message.id)
                .await
                .unwrap_or_else(|error| {
                    diagnostics::error(
                        "qa/attachments",
                        &format!("Skipped attachments of a message: {error}"),
                    );
                    Vec::new()
                });
        }
        messages.push(MessageEvidence {
            id: message.id,
            author,
            when,
            subject: message_subject,
            text,
            attachments,
        });
    }
    Ok(ConversationEvidence {
        conversation_id: conversation_id.into(),
        subject,
        messages,
    })
}

async fn fetch_attachment_texts(
    state: &AppState,
    token: &str,
    message_id: &str,
) -> Result<Vec<(String, String)>> {
    let encoded_id: String =
        url::form_urlencoded::byte_serialize(message_id.as_bytes()).collect();
    let url = qa_url(
        &format!("/me/messages/{encoded_id}/attachments"),
        &[
            ("$select", "id,name,contentType,size".into()),
            ("$top", "20".into()),
        ],
    )?;
    let page: Page<AttachmentMeta> = graph_get(state, token, url).await?;
    let mut texts = Vec::new();
    for meta in page.value {
        if meta
            .odata_type
            .as_deref()
            .is_some_and(|kind| kind != "#microsoft.graph.fileAttachment")
        {
            continue;
        }
        let name = meta.name.unwrap_or_else(|| "attachment".into());
        if meta.size.unwrap_or(0) > MAX_ATTACHMENT_BYTES {
            texts.push((name, "[Attachment too large to read]".into()));
            continue;
        }
        if !supported_attachment(&name, meta.content_type.as_deref()) {
            continue;
        }
        let attachment_id: String =
            url::form_urlencoded::byte_serialize(meta.id.as_bytes()).collect();
        let url = qa_url(
            &format!("/me/messages/{encoded_id}/attachments/{attachment_id}"),
            &[("$select", "contentBytes".into())],
        )?;
        let full: AttachmentFull = graph_get(state, token, url).await?;
        let Some(bytes_b64) = full.content_bytes else {
            continue;
        };
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(bytes_b64) else {
            continue;
        };
        if let Some(text) = extract_attachment_text(&name, &bytes) {
            let trimmed: String = text.chars().take(ATTACHMENT_BUDGET).collect();
            if !trimmed.trim().is_empty() {
                texts.push((name, trimmed));
            }
        } else {
            texts.push((name, "[Attachment format could not be read as text]".into()));
        }
    }
    Ok(texts)
}

fn supported_attachment(name: &str, content_type: Option<&str>) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".pdf")
        || lower.ends_with(".docx")
        || lower.ends_with(".txt")
        || lower.ends_with(".csv")
        || lower.ends_with(".md")
        || content_type
            .map(|kind| kind.starts_with("text/"))
            .unwrap_or(false)
}

fn extract_attachment_text(name: &str, bytes: &[u8]) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".pdf") {
        return pdf_extract::extract_text_from_mem(bytes).ok();
    }
    if lower.ends_with(".docx") {
        return docx_text(bytes);
    }
    Some(String::from_utf8_lossy(bytes).into_owned())
}

fn docx_text(bytes: &[u8]) -> Option<String> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).ok()?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .ok()?
        .read_to_string(&mut xml)
        .ok()?;
    let xml = xml.replace("</w:p>", "\n");
    let tags = Regex::new(r"<[^>]+>").unwrap();
    let text = html_escape::decode_html_entities(&tags.replace_all(&xml, "")).into_owned();
    Some(text)
}

// ---------------------------------------------------------------------------
// AI evaluation
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
struct QaAiOutput {
    #[serde(default)]
    request_id: String,
    #[serde(default)]
    request_date: String,
    #[serde(default)]
    request_source: String,
    #[serde(default)]
    initial_response: String,
    #[serde(default)]
    initial_response_notes: String,
    #[serde(default)]
    customer_sentiment: String,
    #[serde(default)]
    customer_sentiment_notes: String,
    #[serde(default)]
    adherence: String,
    #[serde(default)]
    adherence_notes: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    status_notes: String,
    #[serde(default)]
    update_follow_up: String,
    #[serde(default)]
    update_follow_up_notes: String,
    #[serde(default)]
    auto_fail: String,
}

fn qa_json_schema() -> serde_json::Value {
    let criterion = |description: &str| {
        serde_json::json!({ "type": "string", "enum": ["Y", "N", "N/A"], "description": description })
    };
    serde_json::json!({
        "type": "object",
        "properties": {
            "request_id": { "type": "string", "maxLength": 200 },
            "request_date": { "type": "string", "maxLength": 10 },
            "request_source": { "type": "string", "enum": ["Email", "IRIS"] },
            "initial_response": criterion("Y/N/N/A initial response evaluation"),
            "initial_response_notes": { "type": "string", "maxLength": 500 },
            "customer_sentiment": { "type": "string", "enum": ["Positive", "Neutral", "Negative"] },
            "customer_sentiment_notes": { "type": "string", "maxLength": 500 },
            "adherence": criterion("Y/N/N/A adherence evaluation"),
            "adherence_notes": { "type": "string", "maxLength": 500 },
            "status": criterion("Y/N/N/A status communication evaluation"),
            "status_notes": { "type": "string", "maxLength": 500 },
            "update_follow_up": criterion("Y/N/N/A update and follow-up evaluation"),
            "update_follow_up_notes": { "type": "string", "maxLength": 500 },
            "auto_fail": {
                "type": "string",
                "enum": ["none", "no_initial_response", "missed_priority", "no_status_updates", "non_adherence_sops"]
            }
        },
        "required": [
            "request_id", "request_date", "request_source",
            "initial_response", "initial_response_notes",
            "customer_sentiment", "customer_sentiment_notes",
            "adherence", "adherence_notes",
            "status", "status_notes",
            "update_follow_up", "update_follow_up_notes",
            "auto_fail"
        ],
        "additionalProperties": false
    })
}

fn build_transcript(conversation: &ConversationEvidence) -> String {
    let mut out = String::new();
    for (index, message) in conversation.messages.iter().enumerate() {
        let body: String = message.text.chars().take(BODY_BUDGET).collect();
        let chunk = format!(
            "=== Message {} ===\nFrom: {}\nDate: {}\nSubject: {}\n{}\n\n",
            index + 1,
            message.author,
            message.when.format("%Y-%m-%d %H:%M UTC"),
            message.subject,
            body
        );
        if out.len() + chunk.len() > TRANSCRIPT_BUDGET {
            out.push_str("[Remaining messages truncated for length]\n");
            return out;
        }
        out.push_str(&chunk);
        for (name, text) in &message.attachments {
            let chunk = format!("--- Attachment of message {index}: {name} ---\n{text}\n\n", index = index + 1);
            if out.len() + chunk.len() > TRANSCRIPT_BUDGET {
                out.push_str("[Remaining attachments truncated for length]\n");
                return out;
            }
            out.push_str(&chunk);
        }
    }
    out
}

fn build_prompt(auditee: &QaAuditee, transcript: &str) -> String {
    let custom_rules = if auditee.custom_rules.trim().is_empty() {
        String::new()
    } else {
        format!(
            "MANAGER RULES FOR THIS SPECIFIC ANALYST (these override the default rubric when they conflict; apply them only to this analyst):\n{}\n\n",
            auditee.custom_rules.trim()
        )
    };
    format!(
        "You are a QA auditor for a client service team. Evaluate the email conversation below, \
which involves the customer service analyst {name} ({email}).\n\n\
RUBRIC (team QA audit framework):\n\
1. Initial Response: the analyst acknowledged the initial customer request within 4 hours, \
confirming they would begin working on the case. Y = initial response clear and timely; \
N = no response within the stipulated timeframe; N/A = not applicable.\n\
2. Customer Sentiment: emotional tone of the customer throughout the interaction. \
Positive = satisfaction, appreciation or gratitude; Neutral = polite but indifferent or factual; \
Negative = frustration, complaints or escalation risks.\n\
3. Adherence: the analyst stuck to the timeline they shared with the customer and followed \
through as expected. Y = all steps followed; N = missed a required step or deviated from SOPs; N/A.\n\
4. Status: the case status was clearly and proactively communicated to the customer. \
Y = clear and proactive; N = unclear, or provided late after customer prompting; N/A.\n\
5. Update/Follow-up: timely and relevant updates were shared based on case priority \
(Urgent: within 24 hours; Medium: within 72 hours; Non-urgent: within a week). \
Y = clear updates within the stipulated timeframes; N = updates not provided or the client \
had to follow up due to lack of timeliness; N/A.\n\n\
AUTO-FAIL (choose exactly one):\n\
- \"no_initial_response\": there was no initial response at all.\n\
- \"missed_priority\": missed or incorrect priority handling.\n\
- \"no_status_updates\": failure to provide status updates.\n\
- \"non_adherence_sops\": non-adherence to SOPs.\n\
- \"none\": no auto-fail applies.\n\n\
FIELD RULES:\n\
- request_id: the email subject, or the Incident ID if this is an IRIS ticket.\n\
- request_date: the date the analyst was first engaged for this request (YYYY-MM-DD).\n\
- request_source: \"IRIS\" if the conversation references an IRIS ticket or incident, otherwise \"Email\".\n\
- initial_response_notes: include the date and time the request was made and the date and time \
it was acknowledged.\n\
- update_follow_up_notes: document the follow-up timelines.\n\
- Every note must cite concrete evidence from the conversation (a short quote or a timestamp). \
Never invent facts. If the evidence is insufficient for a criterion, mark it \"N/A\" and say why \
in its notes.\n\n\
{custom_rules}\
CONVERSATION (chronological):\n{transcript}",
        name = auditee.name,
        email = auditee.email,
    )
}

fn normalize_yna(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" | "sí" | "si" => "Y".into(),
        "n" | "no" => "N".into(),
        _ => "N/A".into(),
    }
}

fn normalize_sentiment(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "positive" | "positivo" => "Positive".into(),
        "negative" | "negativo" => "Negative".into(),
        _ => "Neutral".into(),
    }
}

fn normalize_auto_fail(value: &str) -> String {
    match value.trim().to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
        "no_initial_response" => "no_initial_response".into(),
        "missed_priority" | "missed_incorrect_priority_handling" => "missed_priority".into(),
        "no_status_updates" | "failure_to_provide_status_updates" => "no_status_updates".into(),
        "non_adherence_sops" | "non_adherence_to_sops" => "non_adherence_sops".into(),
        _ => "none".into(),
    }
}

async fn evaluate_conversation(
    state: &AppState,
    model: &str,
    auditee: &QaAuditee,
    conversation: &ConversationEvidence,
) -> Result<QaCase> {
    let transcript = build_transcript(conversation);
    let prompt = build_prompt(auditee, &transcript);
    diagnostics::info(
        "qa/analysis",
        &format!(
            "Evaluating QA conversation '{}' for {} ({} messages)",
            conversation.subject,
            auditee.email,
            conversation.messages.len()
        ),
    );
    let raw = state
        .local_ai
        .generate_structured(&state.http, model, prompt, qa_json_schema(), 900)
        .await?;
    let output: QaAiOutput = serde_json::from_str(&raw).map_err(|error| {
        diagnostics::error(
            "qa/analysis",
            &format!("Bundled Local AI returned invalid QA JSON: {error}"),
        );
        AppError::Message("Bundled Local AI did not return valid structured JSON".into())
    })?;
    let first_date = conversation
        .messages
        .first()
        .map(|m| m.when.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let request_date = if output.request_date.trim().is_empty() {
        first_date
    } else {
        output.request_date.trim().to_string()
    };
    let request_id = if output.request_id.trim().is_empty() {
        conversation.subject.clone()
    } else {
        output.request_id.trim().to_string()
    };
    let request_source = match output.request_source.trim().to_ascii_lowercase().as_str() {
        "iris" => "IRIS".into(),
        _ => "Email".into(),
    };
    let digest = hex::encode(Sha256::digest(
        format!("{}|{}", conversation.conversation_id, auditee.email).as_bytes(),
    ));
    let evidence = conversation
        .messages
        .iter()
        .map(|m| QaEvidenceRef {
            message_id: m.id.clone(),
            label: format!(
                "{} · {} · {}",
                m.when.format("%Y-%m-%d %H:%M UTC"),
                m.author,
                m.subject
            ),
            excerpt: m.text.chars().take(220).collect(),
        })
        .collect();
    Ok(QaCase {
        case_id: format!("qa:{}", &digest[..16]),
        analyst_name: auditee.name.clone(),
        analyst_email: auditee.email.clone(),
        audit_date: Utc::now().format("%Y-%m-%d").to_string(),
        request_id,
        request_date,
        request_source,
        initial_response: normalize_yna(&output.initial_response),
        initial_response_notes: output.initial_response_notes.trim().to_string(),
        customer_sentiment: normalize_sentiment(&output.customer_sentiment),
        customer_sentiment_notes: output.customer_sentiment_notes.trim().to_string(),
        adherence: normalize_yna(&output.adherence),
        adherence_notes: output.adherence_notes.trim().to_string(),
        status: normalize_yna(&output.status),
        status_notes: output.status_notes.trim().to_string(),
        update_follow_up: normalize_yna(&output.update_follow_up),
        update_follow_up_notes: output.update_follow_up_notes.trim().to_string(),
        auto_fail: normalize_auto_fail(&output.auto_fail),
        evidence,
        selected: true,
        reviewed: false,
    })
}

fn no_cases_case(auditee: &QaAuditee) -> QaCase {
    let digest = hex::encode(Sha256::digest(
        format!("no-cases|{}|{}", auditee.email, Utc::now().format("%Y-%m-%d")).as_bytes(),
    ));
    QaCase {
        case_id: format!("qa:{}", &digest[..16]),
        analyst_name: auditee.name.clone(),
        analyst_email: auditee.email.clone(),
        audit_date: Utc::now().format("%Y-%m-%d").to_string(),
        request_id: String::new(),
        request_date: String::new(),
        request_source: "Email".into(),
        initial_response: "N/A".into(),
        initial_response_notes: String::new(),
        customer_sentiment: "Neutral".into(),
        customer_sentiment_notes: String::new(),
        adherence: "N/A".into(),
        adherence_notes: String::new(),
        status: "N/A".into(),
        status_notes: String::new(),
        update_follow_up: "N/A".into(),
        update_follow_up_notes: String::new(),
        auto_fail: "no_cases".into(),
        evidence: Vec::new(),
        selected: false,
        reviewed: false,
    }
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

pub async fn extract(
    state: &AppState,
    token: &str,
    model: &str,
    historical: bool,
) -> Result<QaExtractionResult> {
    let config = state.read_settings()?.qa;
    if config.auditees.is_empty() {
        return Err(AppError::Message(
            "Add at least one person to the QA audit list first.".into(),
        ));
    }
    // Historical mode audits everything QA-related the person has ever sent
    // (only for people whose historical audit is still pending). Weekly mode
    // covers the configured lookback window for everyone.
    let auditees: Vec<&QaAuditee> = if historical {
        config.auditees.iter().filter(|a| !a.historical_done).collect()
    } else {
        config.auditees.iter().collect()
    };
    if auditees.is_empty() {
        return Err(AppError::Message(
            "Everyone on the list already has their historical audit. To redo one, mark it as pending from the person's menu.".into(),
        ));
    }
    let start = (!historical).then(|| Utc::now() - Duration::days(config.lookback_days.max(1) as i64));
    let cap = if historical {
        MAX_CONVERSATIONS_HISTORICAL
    } else {
        MAX_CONVERSATIONS_PER_AUDITEE
    };
    let mut cases = Vec::new();
    let mut warnings = Vec::new();
    for auditee in &auditees {
        let conversations =
            match list_auditee_conversations(state, token, auditee, start, cap, &config.subject_keywords).await {
                Ok(ids) => ids,
                Err(error) => {
                    warnings.push(format!(
                        "Could not search mail from {}: {error}",
                        auditee.email
                    ));
                    continue;
                }
            };
        if conversations.is_empty() {
            warnings.push(if historical {
                format!(
                    "No historical QA conversations found for {}.",
                    auditee.name
                )
            } else {
                format!(
                    "No QA cases found for {} in the last {} days.",
                    auditee.name, config.lookback_days
                )
            });
            cases.push(no_cases_case(auditee));
            continue;
        }
        if conversations.len() >= cap {
            warnings.push(format!(
                "{} has more than {cap} QA conversations; only the {cap} most recent were audited.",
                auditee.name
            ));
        }
        for conversation_id in conversations {
            let conversation =
                match fetch_conversation(state, token, &conversation_id, start).await {
                    Ok(value) => value,
                    Err(error) => {
                        warnings.push(format!(
                            "A conversation of {} could not be read: {error}",
                            auditee.email
                        ));
                        continue;
                    }
                };
            if conversation.messages.is_empty() {
                continue;
            }
            match evaluate_conversation(state, model, auditee, &conversation).await {
                Ok(case) => cases.push(case),
                Err(error) => warnings.push(format!(
                    "Local AI could not evaluate '{}' ({}): {error}",
                    conversation.subject, auditee.name
                )),
            }
        }
    }
    cases.sort_by(|a, b| {
        a.analyst_name
            .cmp(&b.analyst_name)
            .then(a.request_date.cmp(&b.request_date))
    });
    Ok(QaExtractionResult { cases, warnings })
}

/// Cheap mailbox watch: reports which watched auditees sent mail since the
/// last check, without running any AI analysis.
pub async fn check_new_mail(state: &AppState, token: &str) -> Result<QaWatchStatus> {
    let config = state.read_settings()?.qa;
    let since = config
        .last_mail_check
        .as_deref()
        .and_then(|value| parse_time(value).ok())
        .unwrap_or_else(|| Utc::now() - Duration::days(1));
    let mut new_senders = Vec::new();
    for auditee in config.auditees.iter().filter(|a| a.watched) {
        let filter = format!(
            "from/emailAddress/address eq '{}' and receivedDateTime gt {}",
            escape_filter(&auditee.email),
            since.to_rfc3339()
        );
        let url = qa_url(
            "/me/messages",
            &[
                ("$filter", filter),
                ("$select", "id".into()),
                ("$top", "1".into()),
            ],
        )?;
        let page: Page<QaMailMessage> = match graph_get(state, token, url).await {
            Ok(page) => page,
            Err(error) => {
                diagnostics::error(
                    "qa/watch",
                    &format!("Mailbox watch failed for {}: {error}", auditee.email),
                );
                continue;
            }
        };
        if !page.value.is_empty() {
            new_senders.push(auditee.name.clone());
        }
    }
    let checked_at = Utc::now();
    state.update_settings(|settings| {
        settings.qa.last_mail_check = Some(checked_at.to_rfc3339());
        for name in &new_senders {
            if !settings.qa.pending_watch.contains(name) {
                settings.qa.pending_watch.push(name.clone());
            }
        }
    })?;
    Ok(QaWatchStatus {
        new_senders,
        checked_at: checked_at.to_rfc3339(),
    })
}

// ---------------------------------------------------------------------------
// Last-run cache (so scheduled/background runs can be reviewed later)
// ---------------------------------------------------------------------------

fn last_run_path(state: &AppState) -> PathBuf {
    state.config_dir.join("qa-last-run.json")
}

pub fn save_last_run(
    state: &AppState,
    result: &QaExtractionResult,
    source: &str,
    historical: bool,
) -> Result<()> {
    let snapshot = crate::models::QaLastRun {
        ran_at: Utc::now().to_rfc3339(),
        source: source.into(),
        historical,
        result: result.clone(),
    };
    let path = last_run_path(state);
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temp, serde_json::to_vec_pretty(&snapshot)?)
        .context("Unable to save the QA run snapshot")?;
    fs::rename(&temp, &path).context("Unable to commit the QA run snapshot")?;
    Ok(())
}

pub fn load_last_run(state: &AppState) -> Result<Option<crate::models::QaLastRun>> {
    let path = last_run_path(state);
    if !path.is_file() {
        return Ok(None);
    }
    let snapshot = fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    if snapshot.is_none() {
        diagnostics::error(
            "qa/cache",
            &format!("Ignored an invalid QA run snapshot at {}", path.display()),
        );
    }
    Ok(snapshot)
}

/// ISO week label (e.g. "2026-W39") used for the per-week Excel sheets.
fn week_label(date: &str) -> String {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|d| {
            let week = d.iso_week();
            format!("{}-W{:02}", week.year(), week.week())
        })
        .unwrap_or_else(|_| date.to_string())
}

// ---------------------------------------------------------------------------
// Excel export (cumulative per analyst, one sheet per audit week)
// ---------------------------------------------------------------------------

pub const AUTO_FAIL_LABELS: [(&str, &str); 6] = [
    ("none", "No Autofail"),
    ("no_initial_response", "No initial response"),
    ("missed_priority", "Missed/Incorrect Priority Handling"),
    ("no_status_updates", "Failure to Provide Status Updates"),
    ("non_adherence_sops", "Non-Adherence to SOPs"),
    ("no_cases", "No Cases Available for audit"),
];

pub fn auto_fail_label(value: &str) -> String {
    AUTO_FAIL_LABELS
        .iter()
        .find(|(key, _)| *key == value)
        .map(|(_, label)| label.to_string())
        .unwrap_or_else(|| "No Autofail".into())
}

fn sanitize_name(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

const QA_HEADERS: [&str; 17] = [
    "Audit date",
    "Vertical/Team",
    "Analyst",
    "Request ID",
    "Request Date",
    "Request Source",
    "Initial Response",
    "Notes/Examples Initial response",
    "Customer Sentiment",
    "Customer sentiment Notes/Examples",
    "Adherence",
    "Adherence Notes/Examples",
    "Status",
    "Status Notes/Examples",
    "Update/Follow Up",
    "Update/Follow Up Notes/Examples",
    "QA Auto fail?",
];

pub fn export(config: &QaConfig, cases: &[QaCase]) -> Result<QaExportResult> {
    let root = config.output_folder.as_deref().ok_or_else(|| {
        AppError::Message("Configure the QA output folder before exporting.".into())
    })?;
    let selected: Vec<&QaCase> = cases.iter().filter(|case| case.selected).collect();
    if selected.is_empty() {
        return Err(AppError::Message(
            "Select at least one reviewed QA case before exporting.".into(),
        ));
    }
    let mut by_analyst: HashMap<&str, Vec<&QaCase>> = HashMap::new();
    for case in &selected {
        by_analyst
            .entry(case.analyst_email.as_str())
            .or_default()
            .push(case);
    }
    let mut files = Vec::new();
    let mut written = 0usize;
    for analyst_cases in by_analyst.values() {
        let name = sanitize_name(&analyst_cases[0].analyst_name);
        let dir = Path::new(root).join(&name);
        fs::create_dir_all(&dir).context("Unable to create the QA analyst folder")?;
        let file = dir.join(format!("QA_{name}.xlsx"));
        let mut book = if file.is_file() {
            umya_spreadsheet::reader::xlsx::read(&file)
                .context("The existing QA workbook could not be opened. Close it in Excel and try again.")?
        } else {
            umya_spreadsheet::new_file()
        };
        // Group this analyst's cases by ISO week; each week gets (or
        // replaces) its own sheet so same-week reruns stay idempotent.
        let mut by_week: HashMap<String, Vec<&QaCase>> = HashMap::new();
        for case in analyst_cases {
            by_week.entry(week_label(&case.audit_date)).or_default().push(case);
        }
        let mut weeks: Vec<String> = by_week.keys().cloned().collect();
        weeks.sort();
        let mut first_sheet_of_new_book = !file.is_file();
        for week in weeks {
            let week = week.as_str();
            if book.get_sheet_by_name(week).is_some() {
                book.remove_sheet_by_name(week)
                    .context("Unable to refresh the QA sheet for the audit week")?;
            }
            let reused_default = first_sheet_of_new_book
                && book.get_sheet_by_name("Sheet1").is_some()
                && book.get_sheet_collection().len() == 1;
            let sheet = if reused_default {
                let sheet = book.get_sheet_by_name_mut("Sheet1").ok_or_else(|| {
                    AppError::Message("Unable to prepare the QA worksheet.".into())
                })?;
                sheet.set_name(week);
                sheet
            } else {
                book.new_sheet(week)
                    .context("Unable to create the QA worksheet")?
            };
            first_sheet_of_new_book = false;
            write_sheet(sheet, config, &by_week[week]);
            written += by_week[week].len();
        }
        let temp = file.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        umya_spreadsheet::writer::xlsx::write(&book, &temp)
            .context("Unable to save the QA workbook. Close it in Excel and try again.")?;
        fs::rename(&temp, &file).context("Unable to commit the QA workbook")?;
        files.push(file.display().to_string());
    }
    diagnostics::info(
        "qa/export",
        &format!("QA export completed: {written} rows into {} workbook(s)", files.len()),
    );
    Ok(QaExportResult { files, written })
}

fn write_sheet(
    sheet: &mut umya_spreadsheet::Worksheet,
    config: &QaConfig,
    cases: &[&QaCase],
) {
    for (col, header) in QA_HEADERS.iter().enumerate() {
        let coordinate = format!("{}1", (b'A' + col as u8) as char);
        let cell = sheet.get_cell_mut(coordinate.as_str());
        cell.set_value(header.to_string());
        cell.get_style_mut().get_font_mut().set_bold(true);
    }
    for (row, case) in cases.iter().enumerate() {
        let values = [
            case.audit_date.clone(),
            config.vertical.clone(),
            case.analyst_name.clone(),
            case.request_id.clone(),
            case.request_date.clone(),
            case.request_source.clone(),
            case.initial_response.clone(),
            case.initial_response_notes.clone(),
            case.customer_sentiment.clone(),
            case.customer_sentiment_notes.clone(),
            case.adherence.clone(),
            case.adherence_notes.clone(),
            case.status.clone(),
            case.status_notes.clone(),
            case.update_follow_up.clone(),
            case.update_follow_up_notes.clone(),
            auto_fail_label(&case.auto_fail),
        ];
        for (col, value) in values.iter().enumerate() {
            let coordinate = format!("{}{}", (b'A' + col as u8) as char, row + 2);
            sheet.get_cell_mut(coordinate.as_str()).set_value(value.clone());
        }
    }
    for (col, width) in [12.0, 42.0, 24.0, 30.0, 12.0, 14.0, 16.0, 46.0, 18.0, 46.0, 11.0, 46.0, 9.0, 46.0, 16.0, 46.0, 30.0]
        .iter()
        .enumerate()
    {
        let letter = ((b'A' + col as u8) as char).to_string();
        sheet.get_column_dimension_mut(&letter).set_width(*width);
    }
}

pub fn output_folder(config: &QaConfig) -> Option<PathBuf> {
    config.output_folder.as_deref().map(PathBuf::from)
}
