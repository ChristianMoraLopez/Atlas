//! QA Audit rubric: evaluates one mail conversation against the team QA
//! framework with the bundled local model, and writes cumulative
//! per-analyst Excel workbooks (one sheet per ISO week of the request).
//! Mailbox collection, scheduling and the case store live in `qa_engine`.
//! Fully additive: it never touches the interactions tracker pipeline.

use crate::{
    diagnostics,
    error::{AppError, Context, Result},
    models::{QaAuditee, QaCase, QaConfig, QaEvidenceRef},
    state::AppState,
};
use chrono::{DateTime, Datelike, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const BODY_BUDGET: usize = 1200;
const TRANSCRIPT_BUDGET: usize = 6000;
const QA_OUTPUT_TOKENS: u32 = 1400;
const QA_RETRY_OUTPUT_TOKENS: u32 = 1800;

pub(crate) struct MessageEvidence {
    pub id: String,
    pub author: String,
    pub when: DateTime<Utc>,
    pub subject: String,
    pub text: String,
}

pub(crate) struct ConversationEvidence {
    pub conversation_id: String,
    pub subject: String,
    pub messages: Vec<MessageEvidence>,
}

pub(crate) fn subject_matches(subject: &str, keywords: &[String]) -> bool {
    let lower = subject.to_lowercase();
    let mut active = keywords
        .iter()
        .map(|k| k.trim().to_lowercase())
        .filter(|k| !k.is_empty())
        .peekable();
    if active.peek().is_none() {
        return true;
    }
    active.any(|k| lower.contains(&k))
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

fn qa_json_schema(note_limit: u32) -> serde_json::Value {
    let criterion = |description: &str| {
        serde_json::json!({ "type": "string", "enum": ["Y", "N", "N/A"], "description": description })
    };
    let note = serde_json::json!({ "type": "string", "maxLength": note_limit });
    serde_json::json!({
        "type": "object",
        "properties": {
            "request_id": { "type": "string", "maxLength": 160 },
            "request_date": { "type": "string", "maxLength": 10 },
            "request_source": { "type": "string", "enum": ["Email", "IRIS"] },
            "initial_response": criterion("Y/N/N/A initial response evaluation"),
            "initial_response_notes": note,
            "customer_sentiment": { "type": "string", "enum": ["Positive", "Neutral", "Negative"] },
            "customer_sentiment_notes": note,
            "adherence": criterion("Y/N/N/A adherence evaluation"),
            "adherence_notes": note,
            "status": criterion("Y/N/N/A status communication evaluation"),
            "status_notes": note,
            "update_follow_up": criterion("Y/N/N/A update and follow-up evaluation"),
            "update_follow_up_notes": note,
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
    }
    out
}

fn build_prompt(auditee: &QaAuditee, transcript: &str, note_limit: u32) -> String {
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
- Every note must cite concrete evidence from the conversation (a short quote or a timestamp) \
and stay under {note_limit} characters. Never invent facts. If the evidence is insufficient for \
a criterion, mark it \"N/A\" and say why in its notes.\n\n\
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

fn valid_request_date(value: &str) -> bool {
    chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").is_ok()
}

pub(crate) fn case_id(conversation_id: &str, analyst_email: &str) -> String {
    let digest = hex::encode(Sha256::digest(
        format!("{}|{}", conversation_id, analyst_email.to_lowercase()).as_bytes(),
    ));
    format!("qa:{}", &digest[..16])
}

async fn run_model(
    state: &AppState,
    model: &str,
    prompt: String,
    note_limit: u32,
    tokens: u32,
) -> Result<QaAiOutput> {
    let raw = state
        .local_ai
        .generate_structured(&state.http, model, prompt, qa_json_schema(note_limit), tokens)
        .await?;
    serde_json::from_str(&raw).map_err(|error| {
        diagnostics::error(
            "qa/analysis",
            &format!(
                "Bundled Local AI returned invalid QA JSON ({} bytes): {error}",
                raw.len()
            ),
        );
        AppError::Message("Bundled Local AI did not return valid structured JSON".into())
    })
}

pub(crate) async fn evaluate_conversation(
    state: &AppState,
    model: &str,
    auditee: &QaAuditee,
    conversation: &ConversationEvidence,
) -> Result<QaCase> {
    let transcript = build_transcript(conversation);
    diagnostics::info(
        "qa/analysis",
        &format!(
            "Evaluating QA conversation for {} ({} messages)",
            auditee.email,
            conversation.messages.len()
        ),
    );
    // A truncated structured answer is retried once with shorter notes and a
    // larger output budget before the conversation is reported as failed.
    let output = match run_model(
        state,
        model,
        build_prompt(auditee, &transcript, 300),
        300,
        QA_OUTPUT_TOKENS,
    )
    .await
    {
        Ok(output) => output,
        Err(_) => {
            run_model(
                state,
                model,
                build_prompt(auditee, &transcript, 150),
                150,
                QA_RETRY_OUTPUT_TOKENS,
            )
            .await?
        }
    };
    let first_date = conversation
        .messages
        .first()
        .map(|m| m.when.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let request_date = if valid_request_date(&output.request_date) {
        output.request_date.trim().to_string()
    } else {
        first_date
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
        case_id: case_id(&conversation.conversation_id, &auditee.email),
        analyst_name: auditee.name.trim().to_string(),
        analyst_email: auditee.email.trim().to_lowercase(),
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
        new_evidence: false,
    })
}

// ---------------------------------------------------------------------------
// Excel export (cumulative per analyst, one sheet per request week)
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

/// ISO week label (e.g. "2026-W39") used for the per-week Excel sheets.
fn week_label(date: &str) -> Option<String> {
    chrono::NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d")
        .ok()
        .map(|d| {
            let week = d.iso_week();
            format!("{}-W{:02}", week.year(), week.week())
        })
}

/// Cases are filed under the week the customer request happened, so a
/// historical import spreads over the weeks it really covers.
pub fn case_week(case: &QaCase) -> String {
    week_label(&case.request_date)
        .or_else(|| week_label(&case.audit_date))
        .unwrap_or_else(|| "Undated".into())
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

pub fn workbook_path(config: &QaConfig, analyst_name: &str) -> Result<PathBuf> {
    let root = config
        .output_folder
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            AppError::Message("Configure the QA output folder before exporting.".into())
        })?;
    let name = sanitize_name(analyst_name);
    Ok(Path::new(root).join(&name).join(format!("QA_{name}.xlsx")))
}

/// Rewrites the listed week sheets of one analyst workbook from `cases`
/// (every exportable case of that analyst). Weeks without cases lose their
/// sheet; other sheets and workbooks are untouched.
pub fn export_analyst(
    config: &QaConfig,
    analyst_name: &str,
    cases: &[&QaCase],
    weeks: &BTreeSet<String>,
) -> Result<(PathBuf, usize)> {
    let file = workbook_path(config, analyst_name)?;
    let dir = file
        .parent()
        .ok_or_else(|| AppError::Message("The QA workbook has no folder.".into()))?;
    fs::create_dir_all(dir).context("Unable to create the QA analyst folder")?;
    let existed = file.is_file();
    let mut book = if existed {
        umya_spreadsheet::reader::xlsx::read(&file).context(
            "The existing QA workbook could not be opened. Close it in Excel and try again.",
        )?
    } else {
        umya_spreadsheet::new_file()
    };
    let mut by_week: BTreeMap<String, Vec<&QaCase>> = BTreeMap::new();
    for case in cases {
        by_week.entry(case_week(case)).or_default().push(case);
    }
    if existed {
        let untouched = book
            .get_sheet_collection()
            .iter()
            .filter(|sheet| !weeks.contains(sheet.get_name()))
            .count();
        let refilled = weeks.iter().filter(|week| by_week.contains_key(*week)).count();
        if untouched == 0 && refilled == 0 {
            fs::remove_file(&file).context("Unable to remove the empty QA workbook")?;
            return Ok((file, 0));
        }
    }
    let mut written = 0usize;
    let mut first_sheet_of_new_book = !existed;
    for week in weeks {
        let week = week.as_str();
        if book.get_sheet_by_name(week).is_some() {
            book.remove_sheet_by_name(week)
                .context("Unable to refresh the QA sheet for the audit week")?;
        }
        let Some(rows) = by_week.get_mut(week) else {
            continue;
        };
        rows.sort_by(|a, b| {
            a.request_date
                .cmp(&b.request_date)
                .then(a.request_id.cmp(&b.request_id))
        });
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
        write_sheet(sheet, config, rows);
        written += rows.len();
    }
    if !existed && first_sheet_of_new_book {
        // Nothing to write into a brand new workbook.
        return Ok((file, 0));
    }
    let temp = file.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    umya_spreadsheet::writer::xlsx::write(&book, &temp)
        .context("Unable to save the QA workbook. Close it in Excel and try again.")?;
    if let Err(error) = fs::rename(&temp, &file) {
        let _ = fs::remove_file(&temp);
        return Err(AppError::Message(format!(
            "Unable to replace the QA workbook. Close it in Excel and try again: {error}"
        )));
    }
    Ok((file, written))
}

/// Layout version of the QA workbooks; bumping it rewrites every sheet once.
pub const EXCEL_LAYOUT: u32 = 2;

/// Each case is a vertical block: field names in column A, values in
/// column B (wrapped so long notes stay readable), cases one below another.
fn write_sheet(sheet: &mut umya_spreadsheet::Worksheet, config: &QaConfig, cases: &[&QaCase]) {
    let mut row = 1u32;
    for (index, case) in cases.iter().enumerate() {
        let title = sheet.get_cell_mut((1, row));
        title.set_value(format!("Case {} of {}", index + 1, cases.len()));
        title.get_style_mut().get_font_mut().set_bold(true);
        let heading = sheet.get_cell_mut((2, row));
        heading.set_value(case.request_id.clone());
        heading.get_style_mut().get_font_mut().set_bold(true);
        row += 1;
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
        for (field, value) in QA_HEADERS.iter().zip(values.iter()) {
            let label = sheet.get_cell_mut((1, row));
            label.set_value(field.to_string());
            label.get_style_mut().get_font_mut().set_bold(true);
            let cell = sheet.get_cell_mut((2, row));
            cell.set_value(value.clone());
            cell.get_style_mut().get_alignment_mut().set_wrap_text(true);
            row += 1;
        }
        // A blank line separates the cases.
        row += 1;
    }
    sheet.get_column_dimension_mut("A").set_width(36.0);
    sheet.get_column_dimension_mut("B").set_width(110.0);
}

pub fn output_folder(config: &QaConfig) -> Option<PathBuf> {
    config.output_folder.as_deref().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(request_date: &str, audit_date: &str) -> QaCase {
        QaCase {
            case_id: "qa:1".into(),
            analyst_name: "Ana Test".into(),
            analyst_email: "ana@example.com".into(),
            audit_date: audit_date.into(),
            request_id: "QA request".into(),
            request_date: request_date.into(),
            request_source: "Email".into(),
            initial_response: "Y".into(),
            initial_response_notes: String::new(),
            customer_sentiment: "Neutral".into(),
            customer_sentiment_notes: String::new(),
            adherence: "Y".into(),
            adherence_notes: String::new(),
            status: "Y".into(),
            status_notes: String::new(),
            update_follow_up: "Y".into(),
            update_follow_up_notes: String::new(),
            auto_fail: "none".into(),
            evidence: Vec::new(),
            selected: true,
            reviewed: false,
            new_evidence: false,
        }
    }

    #[test]
    fn historical_cases_are_filed_under_their_request_week() {
        assert_eq!(case_week(&case("2025-01-06", "2026-09-23")), "2025-W02");
        assert_eq!(case_week(&case("not a date", "2026-09-23")), "2026-W39");
    }

    #[test]
    fn empty_keyword_list_matches_every_subject() {
        assert!(subject_matches("Anything", &[]));
        assert!(subject_matches("Weekly qa review", &["QA".into()]));
        assert!(!subject_matches("Invoice", &["QA".into()]));
    }

    #[test]
    fn export_rewrites_only_requested_weeks() {
        let dir = tempfile::tempdir().unwrap();
        let config = QaConfig {
            output_folder: Some(dir.path().to_string_lossy().into()),
            ..QaConfig::default()
        };
        let first = case("2026-09-01", "2026-09-23");
        let second = case("2026-09-15", "2026-09-23");
        let all = vec![&first, &second];
        let weeks: BTreeSet<String> = all.iter().map(|c| case_week(c)).collect();
        let (file, written) = export_analyst(&config, "Ana Test", &all, &weeks).unwrap();
        assert_eq!(written, 2);
        let book = umya_spreadsheet::reader::xlsx::read(&file).unwrap();
        assert!(book.get_sheet_by_name("2026-W36").is_some());
        assert!(book.get_sheet_by_name("2026-W38").is_some());
        let only_first: BTreeSet<String> = [case_week(&first)].into_iter().collect();
        let (_, written) = export_analyst(&config, "Ana Test", &all, &only_first).unwrap();
        assert_eq!(written, 1);
        let book = umya_spreadsheet::reader::xlsx::read(&file).unwrap();
        assert!(book.get_sheet_by_name("2026-W38").is_some());
        // Cases are vertical blocks: field names in A, values in B.
        let book = umya_spreadsheet::reader::xlsx::read(&file).unwrap();
        let sheet = book.get_sheet_by_name("2026-W38").unwrap();
        assert_eq!(sheet.get_value((1, 1)), "Case 1 of 1");
        assert_eq!(sheet.get_value((1, 2)), "Audit date");
        assert_eq!(sheet.get_value((1, 5)), "Request ID");
        assert_eq!(sheet.get_value((2, 5)), "QA request");
        assert_eq!(sheet.get_value((1, 18)), "QA Auto fail?");
        assert_eq!(sheet.get_value((2, 18)), "No Autofail");
        // Every week lost its cases: the workbook is removed, not left empty.
        let none: Vec<&QaCase> = Vec::new();
        let (_, written) = export_analyst(&config, "Ana Test", &none, &weeks).unwrap();
        assert_eq!(written, 0);
        assert!(!file.exists());
    }
}
