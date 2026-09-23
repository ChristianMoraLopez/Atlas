//! QA engine for the manager role. It keeps a local store of the QA mail
//! found in the manager mailbox, plans a historical import in monthly
//! windows, keeps it current with twice-daily and live-triggered syncs,
//! evaluates changed conversations with the bundled local AI and writes the
//! per-analyst workbooks.
//!
//! Mail arrives either from the personalized AtlasQA Power Automate flows
//! (request/response files in `<OneDrive>\AtlasBridge\qa`) or directly from
//! Microsoft Graph when Atlas runs in Graph mode. Both paths execute the same
//! windowed searches, so the store and the evaluation are source-agnostic.

use crate::{
    auth, diagnostics,
    error::{AppError, Context, Result},
    models::{AppRole, QaAuditee, QaCase, QaConfig, QaExportResult, SourceMode},
    ollama, qa,
    state::AppState,
};
use chrono::{DateTime, Datelike, Days, Local, Months, NaiveDate, NaiveTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};
use url::Url;

pub const QA_CONTRACT: &str = "atlas-qa-v1";
const LIVE_CONTRACT: &str = "atlas-qa-live-v1";
pub const QA_FOLDERS: [&str; 2] = ["Inbox", "Sent Items"];
const QUERY_TOP: u32 = 200;
const QUERIES_PER_REQUEST: usize = 8;
const REQUEST_TIMEOUT_MINUTES: i64 = 25;
const EVALUATION_BUDGET: Duration = Duration::from_secs(240);
const MAX_TEXT_CHARS: usize = 4000;
const MAX_CONVERSATION_MESSAGES: usize = 40;
const EMPTY_MONTHS_TO_STOP: usize = 6;
const MIN_HISTORY_MONTHS: u32 = 12;
const MAX_WINDOW_ATTEMPTS: u32 = 3;
const MAX_EVALUATION_ATTEMPTS: u32 = 3;
const MAX_WARNINGS: usize = 12;
const GRAPH_ROOT: &str = "https://graph.microsoft.com/v1.0";

// ---------------------------------------------------------------------------
// Bridge file contract (shared with the connector installer)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeQuery {
    pub id: String,
    pub folder: String,
    pub search: String,
    pub top: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRequest {
    pub contract: String,
    pub installation_id: String,
    pub request_id: String,
    pub created_at: String,
    pub queries: Vec<BridgeQuery>,
}

pub fn requests_dir(root: &Path) -> PathBuf {
    root.join("requests")
}

pub fn inbox_dir(root: &Path) -> PathBuf {
    root.join("inbox")
}

fn live_dir(root: &Path) -> PathBuf {
    inbox_dir(root).join("live")
}

pub fn request_file(root: &Path) -> PathBuf {
    requests_dir(root).join("qa-request.txt")
}

pub fn watch_file(root: &Path) -> PathBuf {
    requests_dir(root).join("qa-watch.txt")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Message("Atlas could not resolve a QA bridge folder.".into()))?;
    fs::create_dir_all(parent).context("Unable to create the QA bridge folder")?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    fs::write(&temporary, bytes).context("Unable to write a QA bridge file")?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(AppError::Message(format!(
            "Unable to replace a QA bridge file: {error}"
        )));
    }
    Ok(())
}

/// Creates the bridge layout without touching existing requests or answers.
pub fn prepare_folders(root: &Path) -> Result<()> {
    for dir in [requests_dir(root), inbox_dir(root), live_dir(root)] {
        fs::create_dir_all(&dir).context("Unable to create the QA bridge folders")?;
    }
    for file in [request_file(root), watch_file(root)] {
        if !file.exists() {
            write_atomic(&file, b"{}")?;
        }
    }
    Ok(())
}

pub fn write_request(root: &Path, request: &BridgeRequest) -> Result<()> {
    write_atomic(&request_file(root), &serde_json::to_vec(request)?)
}

fn watch_payload(installation_id: &str, auditees: &[String]) -> Value {
    serde_json::json!({
        "contract": QA_CONTRACT,
        "installationId": installation_id,
        "auditees": auditees,
    })
}

/// Writes the watch list read by the live flow. Returns whether it changed.
pub fn write_watch(root: &Path, installation_id: &str, auditees: &[String]) -> Result<bool> {
    let desired = watch_payload(installation_id, auditees);
    let current = fs::read(watch_file(root))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    if current.as_ref() == Some(&desired) {
        return Ok(false);
    }
    write_atomic(&watch_file(root), &serde_json::to_vec(&desired)?)?;
    Ok(true)
}

fn new_request_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        Utc::now().format("%Y%m%d%H%M%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..6]
    )
}

/// A one-message query that proves the Outlook read, the request file and
/// the OneDrive write of this personalized installation.
pub fn ping_request(installation_id: &str) -> BridgeRequest {
    let today = Local::now().date_naive();
    let since = today.checked_sub_days(Days::new(30)).unwrap_or(today);
    BridgeRequest {
        contract: QA_CONTRACT.into(),
        installation_id: installation_id.into(),
        request_id: new_request_id("ping"),
        created_at: Utc::now().to_rfc3339(),
        queries: vec![BridgeQuery {
            id: "ping".into(),
            folder: "Inbox".into(),
            search: format!("received>={}", since.format("%Y-%m-%d")),
            top: 1,
        }],
    }
}

fn answer_name(installation_id: &str, request_id: &str, query_id: &str) -> String {
    format!("atlas-qa-{installation_id}-{request_id}-{query_id}.json")
}

fn is_cloud_placeholder(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if let Ok(metadata) = fs::metadata(path) {
            return metadata.file_attributes() & (0x1000 | 0x40000 | 0x400000) != 0;
        }
    }
    let _ = path;
    false
}

#[derive(Debug, PartialEq, Eq)]
pub enum PingState {
    Answered,
    Failed,
    Waiting,
    NotDownloaded,
}

/// Installer verification: has the flow answered this installation's ping?
pub fn ping_state(root: &Path, installation_id: &str, request_id: &str) -> PingState {
    let inbox = inbox_dir(root);
    let done = inbox.join(answer_name(installation_id, request_id, "done"));
    let page = inbox.join(answer_name(installation_id, request_id, "ping"));
    if !done.exists() && !page.exists() {
        return PingState::Waiting;
    }
    if is_cloud_placeholder(&page) || is_cloud_placeholder(&done) {
        return PingState::NotDownloaded;
    }
    match read_page(&page) {
        Some(page) if page.status == "ok" => PingState::Answered,
        Some(_) => PingState::Failed,
        None if done.exists() => PingState::Failed,
        None => PingState::Waiting,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BridgePage {
    #[serde(default)]
    contract: String,
    #[serde(default)]
    installation_id: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    count: Option<u32>,
    #[serde(default)]
    messages: Vec<BridgeMessage>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BridgeMessage {
    #[serde(default)]
    id: String,
    #[serde(default)]
    conversation_id: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    received_date_time: String,
    #[serde(default)]
    from: Value,
    #[serde(default)]
    to_recipients: Value,
    #[serde(default)]
    cc_recipients: Value,
    #[serde(default)]
    has_attachments: Value,
    #[serde(default)]
    body_preview: String,
    #[serde(default)]
    body: String,
}

fn read_page(path: &Path) -> Option<BridgePage> {
    let bytes = fs::read(path).ok()?;
    let page: BridgePage = serde_json::from_slice(&bytes).ok()?;
    (page.contract == QA_CONTRACT || page.contract == LIVE_CONTRACT).then_some(page)
}

// ---------------------------------------------------------------------------
// Mail normalization
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaMail {
    pub id: String,
    pub conversation_id: String,
    pub subject: String,
    pub received: String,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub text: String,
    #[serde(default)]
    pub has_attachments: bool,
}

fn value_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(value_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("; "),
        Value::Object(map) => {
            if let Some(inner) = map.get("emailAddress") {
                return value_text(inner);
            }
            let name = map.get("name").and_then(Value::as_str).unwrap_or("");
            let address = map.get("address").and_then(Value::as_str).unwrap_or("");
            match (name.is_empty(), address.is_empty()) {
                (true, true) => String::new(),
                (true, false) => address.into(),
                (false, true) => name.into(),
                (false, false) => format!("{name} <{address}>"),
            }
        }
        other => other.to_string(),
    }
}

fn value_bool(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::String(text) => text.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn email_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"[A-Za-z0-9._%+'\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}").expect("valid regex")
    })
}

pub fn addresses(value: &str) -> Vec<String> {
    email_pattern()
        .find_iter(value)
        .map(|found| found.as_str().to_lowercase())
        .collect()
}

pub fn html_to_text(html: &str) -> String {
    static STYLE: OnceLock<Regex> = OnceLock::new();
    static TAGS: OnceLock<Regex> = OnceLock::new();
    let style = STYLE.get_or_init(|| {
        Regex::new(r"(?is)<(style|script|head)[^>]*>.*?</(style|script|head)>").expect("valid regex")
    });
    let tags = TAGS.get_or_init(|| Regex::new(r"(?s)<[^>]*>").expect("valid regex"));
    let without_blocks = style.replace_all(html, " ");
    html_escape::decode_html_entities(&tags.replace_all(&without_blocks, " "))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_message(message: BridgeMessage) -> Option<QaMail> {
    if message.id.trim().is_empty() {
        return None;
    }
    let received = crate::graph::parse_graph_time(&message.received_date_time).ok()?;
    let text = if message.body.trim().is_empty() {
        message.body_preview.trim().to_string()
    } else {
        html_to_text(&message.body)
    };
    Some(QaMail {
        id: message.id.trim().to_string(),
        conversation_id: message.conversation_id.trim().to_string(),
        subject: message.subject.trim().to_string(),
        received: received.to_rfc3339(),
        from: value_text(&message.from),
        to: value_text(&message.to_recipients),
        cc: value_text(&message.cc_recipients),
        text: text.chars().take(MAX_TEXT_CHARS).collect(),
        has_attachments: value_bool(&message.has_attachments),
    })
}

// ---------------------------------------------------------------------------
// Persistent state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalStatus {
    #[default]
    Idle,
    Running,
    Done,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QaWindow {
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub folder: String,
    #[serde(default)]
    pub historical: bool,
    #[serde(default)]
    pub attempts: u32,
}

impl QaWindow {
    fn month_label(&self) -> String {
        self.start.format("%Y-%m").to_string()
    }

    fn split(&self) -> Option<(QaWindow, QaWindow)> {
        let days = (self.end - self.start).num_days();
        if days < 1 {
            return None;
        }
        let middle = self.start.checked_add_days(Days::new((days / 2) as u64))?;
        let second_start = middle.checked_add_days(Days::new(1))?;
        Some((
            QaWindow {
                end: middle,
                attempts: 0,
                ..self.clone()
            },
            QaWindow {
                start: second_start,
                attempts: 0,
                ..self.clone()
            },
        ))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingRequest {
    request_id: String,
    created_at: String,
    windows: BTreeMap<String, QaWindow>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncState {
    #[serde(default)]
    historical_status: HistoricalStatus,
    #[serde(default)]
    historical_auditees: Vec<String>,
    #[serde(default)]
    historical_next_month: Option<NaiveDate>,
    #[serde(default)]
    historical_oldest_allowed: Option<NaiveDate>,
    #[serde(default)]
    historical_months_planned: u32,
    #[serde(default)]
    month_totals: BTreeMap<String, u32>,
    #[serde(default)]
    historical_started_at: Option<String>,
    #[serde(default)]
    historical_completed_at: Option<String>,
    #[serde(default)]
    historical_queue: Vec<QaWindow>,
    #[serde(default)]
    incremental_queue: Vec<QaWindow>,
    #[serde(default)]
    incremental_target: Option<NaiveDate>,
    #[serde(default)]
    last_incremental_until: Option<NaiveDate>,
    #[serde(default)]
    last_incremental_at: Option<String>,
    #[serde(default)]
    slots_done: Vec<String>,
    #[serde(default)]
    incremental_requested: bool,
    #[serde(default)]
    pending: Option<PendingRequest>,
    #[serde(default)]
    consecutive_timeouts: u32,
    #[serde(default)]
    last_live_signal_at: Option<String>,
    #[serde(default)]
    last_error: Option<String>,
    #[serde(default)]
    warnings: Vec<String>,
    #[serde(default)]
    last_export_at: Option<String>,
    #[serde(default)]
    last_export_error: Option<String>,
    #[serde(default)]
    message_count: usize,
    #[serde(default)]
    conversation_count: usize,
    #[serde(default)]
    pending_evaluation: usize,
}

impl SyncState {
    fn warn(&mut self, message: String) {
        diagnostics::error("qa/engine", &message);
        self.warnings.retain(|existing| existing != &message);
        self.warnings.push(message);
        if self.warnings.len() > MAX_WARNINGS {
            let excess = self.warnings.len() - MAX_WARNINGS;
            self.warnings.drain(..excess);
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct MailStore {
    #[serde(default)]
    messages: BTreeMap<String, QaMail>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CaseRecord {
    case: QaCase,
    fingerprint: String,
    conversation_id: String,
    #[serde(default)]
    exported: bool,
    #[serde(default)]
    evaluated_at: String,
    #[serde(default)]
    last_message_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FailureRecord {
    fingerprint: String,
    attempts: u32,
    error: String,
}

#[derive(Default, Serialize, Deserialize)]
struct CaseStore {
    #[serde(default)]
    cases: BTreeMap<String, CaseRecord>,
    #[serde(default)]
    failures: BTreeMap<String, FailureRecord>,
}

fn qa_dir(state: &AppState) -> PathBuf {
    state.config_dir.join("qa")
}

fn load_json<T: Default + for<'de> Deserialize<'de>>(path: &Path) -> T {
    let Ok(bytes) = fs::read(path) else {
        return T::default();
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            let backup = path.with_extension(format!("invalid-{}.json", uuid::Uuid::new_v4().simple()));
            let _ = fs::rename(path, &backup);
            diagnostics::error(
                "qa/store",
                &format!(
                    "Ignored an unreadable QA file {} ({error}); a backup was kept at {}",
                    path.display(),
                    backup.display()
                ),
            );
            T::default()
        }
    }
}

fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    write_atomic(path, &serde_json::to_vec(value)?)
}

fn with_files<T>(state: &AppState, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    let _guard = state
        .qa_files
        .lock()
        .map_err(|_| AppError::Message("QA store lock was poisoned".into()))?;
    operation()
}

fn sync_path(state: &AppState) -> PathBuf {
    qa_dir(state).join("sync.json")
}

fn mail_path(state: &AppState) -> PathBuf {
    qa_dir(state).join("mail-store.json")
}

fn cases_path(state: &AppState) -> PathBuf {
    qa_dir(state).join("cases.json")
}

fn update_sync<T>(state: &AppState, change: impl FnOnce(&mut SyncState) -> T) -> Result<T> {
    with_files(state, || {
        let path = sync_path(state);
        let mut sync: SyncState = load_json(&path);
        let result = change(&mut sync);
        save_json(&path, &sync)?;
        Ok(result)
    })
}

fn update_cases<T>(state: &AppState, change: impl FnOnce(&mut CaseStore) -> Result<T>) -> Result<T> {
    with_files(state, || {
        let path = cases_path(state);
        let mut store: CaseStore = load_json(&path);
        let result = change(&mut store)?;
        save_json(&path, &store)?;
        Ok(result)
    })
}

// ---------------------------------------------------------------------------
// Planning (pure, unit tested)
// ---------------------------------------------------------------------------

fn month_start(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

fn plan_month(start: NaiveDate, today: NaiveDate) -> Vec<QaWindow> {
    let next = start.checked_add_months(Months::new(1)).unwrap_or(start);
    let last_day = next.pred_opt().unwrap_or(start).min(today);
    QA_FOLDERS
        .iter()
        .map(|folder| QaWindow {
            start,
            end: last_day,
            folder: (*folder).into(),
            historical: true,
            attempts: 0,
        })
        .collect()
}

fn incremental_windows(from: NaiveDate, to: NaiveDate) -> Vec<QaWindow> {
    let mut windows = Vec::new();
    let mut start = from.min(to);
    while start <= to {
        let end = start
            .checked_add_days(Days::new(30))
            .unwrap_or(to)
            .min(to);
        for folder in QA_FOLDERS {
            windows.push(QaWindow {
                start,
                end,
                folder: folder.into(),
                historical: false,
                attempts: 0,
            });
        }
        let Some(next) = end.checked_add_days(Days::new(1)) else {
            break;
        };
        start = next;
    }
    windows
}

fn kql_word(value: &str) -> String {
    value
        .chars()
        .filter(|c| !matches!(c, '"' | '(' | ')' | ':' | '\\'))
        .collect()
}

/// KQL for the Outlook search: subject keywords, audited participants and an
/// inclusive date window. No nested quotes so the same text works through the
/// Power Automate connector and Graph `$search`.
pub fn search_query(
    keywords: &[String],
    emails: &[String],
    start: NaiveDate,
    end: NaiveDate,
) -> String {
    let group = |terms: Vec<String>| match terms.len() {
        0 => None,
        1 => terms.into_iter().next(),
        _ => Some(format!("({})", terms.join(" OR "))),
    };
    let keyword_terms = keywords
        .iter()
        .filter_map(|keyword| {
            let words: Vec<String> = keyword
                .split_whitespace()
                .map(kql_word)
                .filter(|word| !word.is_empty())
                .map(|word| format!("subject:{word}"))
                .collect();
            match words.len() {
                0 => None,
                1 => words.into_iter().next(),
                _ => Some(format!("({})", words.join(" AND "))),
            }
        })
        .collect();
    let participant_terms = emails
        .iter()
        .map(|email| kql_word(email.trim()))
        .filter(|email| email.contains('@'))
        .map(|email| format!("participants:{email}"))
        .collect();
    let mut parts: Vec<String> = [group(keyword_terms), group(participant_terms)]
        .into_iter()
        .flatten()
        .collect();
    parts.push(format!(
        "received>={} AND received<={}",
        start.format("%Y-%m-%d"),
        end.format("%Y-%m-%d")
    ));
    parts.join(" AND ")
}

fn start_historical(sync: &mut SyncState, auditees: Vec<String>, months: u32, today: NaiveDate) {
    let current = month_start(today);
    let months = months.clamp(1, 120);
    sync.historical_status = HistoricalStatus::Running;
    sync.historical_auditees = auditees;
    sync.historical_next_month = Some(current);
    sync.historical_oldest_allowed = current.checked_sub_months(Months::new(months - 1));
    sync.historical_months_planned = 0;
    sync.month_totals.clear();
    sync.historical_queue.clear();
    sync.historical_started_at = Some(Utc::now().to_rfc3339());
    sync.historical_completed_at = None;
    if sync.last_incremental_until.is_none() {
        sync.last_incremental_until = Some(today);
    }
}

fn open_months(sync: &SyncState) -> HashSet<String> {
    let mut open: HashSet<String> = sync
        .historical_queue
        .iter()
        .map(QaWindow::month_label)
        .collect();
    if let Some(pending) = &sync.pending {
        open.extend(
            pending
                .windows
                .values()
                .filter(|window| window.historical)
                .map(QaWindow::month_label),
        );
    }
    open
}

/// The import walks back newest-first, so completed months form the newest
/// block. Six empty completed months older than any QA activity, or a full
/// empty year when nothing was found at all, mean there is no older history.
fn history_exhausted(sync: &SyncState) -> bool {
    let open = open_months(sync);
    // BTreeMap order is oldest first.
    let completed: Vec<u32> = sync
        .month_totals
        .iter()
        .filter(|(month, _)| !open.contains(*month))
        .map(|(_, total)| *total)
        .collect();
    let empty_at_old_end = completed.iter().take_while(|total| **total == 0).count();
    if empty_at_old_end == completed.len() {
        completed.len() >= MIN_HISTORY_MONTHS as usize
    } else {
        empty_at_old_end >= EMPTY_MONTHS_TO_STOP
    }
}

fn plan_historical(sync: &mut SyncState, today: NaiveDate) {
    if sync.historical_status != HistoricalStatus::Running {
        return;
    }
    let still_planning = sync.historical_next_month.is_some() || !sync.historical_queue.is_empty();
    if still_planning && history_exhausted(sync) {
        // Drop the queued older months; they are beyond the real history.
        let in_flight: HashSet<String> = sync
            .pending
            .iter()
            .flat_map(|pending| pending.windows.values())
            .filter(|window| window.historical)
            .map(QaWindow::month_label)
            .collect();
        for window in sync.historical_queue.drain(..) {
            let label = window.month_label();
            if !in_flight.contains(&label) {
                sync.month_totals.remove(&label);
            }
        }
        sync.historical_next_month = None;
        return;
    }
    while sync.historical_queue.len() < QUERIES_PER_REQUEST * 2 {
        let Some(next) = sync.historical_next_month else {
            break;
        };
        let below_horizon = sync
            .historical_oldest_allowed
            .is_some_and(|oldest| next < oldest);
        if below_horizon {
            sync.historical_next_month = None;
            break;
        }
        sync.historical_queue.extend(plan_month(next, today));
        sync.historical_months_planned += 1;
        sync.month_totals.entry(next.format("%Y-%m").to_string()).or_insert(0);
        sync.historical_next_month = next.checked_sub_months(Months::new(1));
    }
}

fn historical_finished(sync: &SyncState) -> bool {
    sync.historical_status == HistoricalStatus::Running
        && sync.historical_next_month.is_none()
        && sync.historical_queue.is_empty()
        && !sync
            .pending
            .as_ref()
            .is_some_and(|pending| pending.windows.values().any(|w| w.historical))
}

fn slot_reached(value: &str, now: NaiveTime) -> bool {
    NaiveTime::parse_from_str(value.trim(), "%H:%M").is_ok_and(|time| now >= time)
}

fn plan_incremental(
    sync: &mut SyncState,
    config: &QaConfig,
    today: NaiveDate,
    now: NaiveTime,
) {
    let mut due = sync.incremental_requested;
    for (slot, time) in [
        ("morning", config.check_morning.as_str()),
        ("afternoon", config.check_afternoon.as_str()),
    ] {
        let key = format!("{today}-{slot}");
        if slot_reached(time, now) && !sync.slots_done.contains(&key) {
            sync.slots_done.push(key);
            due = true;
        }
    }
    if sync.slots_done.len() > 8 {
        let excess = sync.slots_done.len() - 8;
        sync.slots_done.drain(..excess);
    }
    if !due {
        return;
    }
    sync.incremental_requested = false;
    let incremental_pending = !sync.incremental_queue.is_empty()
        || sync
            .pending
            .as_ref()
            .is_some_and(|pending| pending.windows.values().any(|w| !w.historical));
    if incremental_pending {
        // The running sync already reaches today; ask again once it drains.
        sync.incremental_requested = sync.incremental_target != Some(today);
        return;
    }
    let from = match sync.last_incremental_until {
        Some(until) => until.checked_sub_days(Days::new(1)).unwrap_or(until),
        None => today
            .checked_sub_days(Days::new(config.lookback_days.clamp(1, 90) as u64))
            .unwrap_or(today),
    };
    sync.incremental_queue = incremental_windows(from, today);
    sync.incremental_target = Some(today);
}

fn next_windows(sync: &mut SyncState) -> Vec<QaWindow> {
    let mut windows = Vec::new();
    while windows.len() < QUERIES_PER_REQUEST && !sync.incremental_queue.is_empty() {
        windows.push(sync.incremental_queue.remove(0));
    }
    while windows.len() < QUERIES_PER_REQUEST && !sync.historical_queue.is_empty() {
        windows.push(sync.historical_queue.remove(0));
    }
    windows
}

fn requeue(sync: &mut SyncState, window: QaWindow) {
    if window.historical {
        sync.historical_queue.insert(0, window);
    } else {
        sync.incremental_queue.insert(0, window);
    }
}

/// Applies one query result to the plan. Saturated windows are split so the
/// connector's page limit never silently drops mail.
fn apply_window_result(sync: &mut SyncState, window: QaWindow, outcome: Option<u32>) {
    match outcome {
        Some(count) => {
            if window.historical {
                *sync.month_totals.entry(window.month_label()).or_insert(0) += count;
            }
            if count >= QUERY_TOP {
                match window.split() {
                    Some((first, second)) => {
                        requeue(sync, second);
                        requeue(sync, first);
                    }
                    None => sync.warn(format!(
                        "More than {QUERY_TOP} QA messages in {} on {}; only the first {QUERY_TOP} were read.",
                        window.folder, window.start
                    )),
                }
            }
        }
        None => {
            let attempts = window.attempts + 1;
            if attempts < MAX_WINDOW_ATTEMPTS {
                requeue(
                    sync,
                    QaWindow {
                        attempts,
                        ..window
                    },
                );
            } else {
                sync.warn(format!(
                    "Outlook could not be searched for {} between {} and {} after {MAX_WINDOW_ATTEMPTS} attempts.",
                    window.folder, window.start, window.end
                ));
            }
        }
    }
}

fn close_incremental_if_drained(sync: &mut SyncState) {
    let incremental_open = !sync.incremental_queue.is_empty()
        || sync
            .pending
            .as_ref()
            .is_some_and(|pending| pending.windows.values().any(|w| !w.historical));
    if !incremental_open {
        if let Some(target) = sync.incremental_target.take() {
            sync.last_incremental_until = Some(target);
            sync.last_incremental_at = Some(Utc::now().to_rfc3339());
        }
    }
}

// ---------------------------------------------------------------------------
// Sources
// ---------------------------------------------------------------------------

enum Source {
    Bridge {
        root: PathBuf,
        installation_id: String,
    },
    Graph,
}

fn resolve_source(state: &AppState) -> Result<Option<Source>> {
    let settings = state.read_settings()?;
    match settings.source_mode {
        SourceMode::MicrosoftGraph => Ok(settings.account.is_some().then_some(Source::Graph)),
        SourceMode::PowerAutomateFolder => {
            let Some(folder) = settings.qa_bridge_folder else {
                return Ok(None);
            };
            let root = PathBuf::from(folder);
            if !root.is_dir() {
                return Ok(None);
            }
            let session = state.qa_connector_installer.snapshot()?.session;
            if session.phase != crate::connector_installer::Phase::Completed {
                return Ok(None);
            }
            Ok(Some(Source::Bridge {
                root,
                installation_id: session.installation_id,
            }))
        }
    }
}

fn watched_emails(config: &QaConfig) -> Vec<String> {
    if !config.watch_enabled {
        return Vec::new();
    }
    let mut emails: Vec<String> = config
        .auditees
        .iter()
        .filter(|auditee| auditee.watched)
        .map(|auditee| auditee.email.trim().to_lowercase())
        .filter(|email| email.contains('@'))
        .collect();
    emails.sort();
    emails.dedup();
    emails
}

fn all_emails(config: &QaConfig) -> Vec<String> {
    let mut emails: Vec<String> = config
        .auditees
        .iter()
        .map(|auditee| auditee.email.trim().to_lowercase())
        .filter(|email| email.contains('@'))
        .collect();
    emails.sort();
    emails.dedup();
    emails
}

fn ingest_messages(store: &mut MailStore, messages: Vec<BridgeMessage>) -> usize {
    let mut added = 0usize;
    for mail in messages.into_iter().filter_map(normalize_message) {
        if store.messages.insert(mail.id.clone(), mail).is_none() {
            added += 1;
        }
    }
    added
}

/// Reads live signals written by the watch flow. Returns true when at least
/// one audited analyst wrote to the manager.
fn ingest_live(root: &Path, installation_id: &str, store: &mut MailStore) -> bool {
    let Ok(entries) = fs::read_dir(live_dir(root)) else {
        return false;
    };
    let prefix = format!("atlas-qa-live-{installation_id}-");
    let mut signalled = false;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(&prefix) || !name.ends_with(".json") {
            continue;
        }
        let Some(page) = read_page(&path) else {
            continue;
        };
        if page.installation_id == installation_id {
            ingest_messages(store, page.messages);
            signalled = true;
        }
        let _ = fs::remove_file(&path);
    }
    signalled
}

fn build_request(
    installation_id: &str,
    config: &QaConfig,
    sync: &SyncState,
    windows: Vec<QaWindow>,
) -> (BridgeRequest, PendingRequest) {
    let request_id = new_request_id("qa");
    let all = all_emails(config);
    let mut queries = Vec::new();
    let mut pending = BTreeMap::new();
    for (index, window) in windows.into_iter().enumerate() {
        let id = format!("q{}", index + 1);
        let emails = if window.historical {
            &sync.historical_auditees
        } else {
            &all
        };
        queries.push(BridgeQuery {
            id: id.clone(),
            folder: window.folder.clone(),
            search: search_query(&config.subject_keywords, emails, window.start, window.end),
            top: QUERY_TOP,
        });
        pending.insert(id, window);
    }
    let created_at = Utc::now().to_rfc3339();
    (
        BridgeRequest {
            contract: QA_CONTRACT.into(),
            installation_id: installation_id.into(),
            request_id: request_id.clone(),
            created_at: created_at.clone(),
            queries,
        },
        PendingRequest {
            request_id,
            created_at,
            windows: pending,
        },
    )
}

/// Collects the flow's answer for the pending request, if complete.
fn collect_answer(
    root: &Path,
    installation_id: &str,
    pending: &PendingRequest,
    store: &mut MailStore,
) -> Option<Vec<(QaWindow, Option<u32>)>> {
    let inbox = inbox_dir(root);
    let done = inbox.join(answer_name(installation_id, &pending.request_id, "done"));
    if !done.exists() || is_cloud_placeholder(&done) {
        return None;
    }
    let mut results = Vec::new();
    for (query_id, window) in &pending.windows {
        let path = inbox.join(answer_name(installation_id, &pending.request_id, query_id));
        let outcome = match read_page(&path) {
            Some(page) if page.status == "ok" && page.installation_id == installation_id => {
                let count = page.count.unwrap_or(page.messages.len() as u32);
                ingest_messages(store, page.messages);
                Some(count)
            }
            _ => None,
        };
        let _ = fs::remove_file(&path);
        results.push((window.clone(), outcome));
    }
    let _ = fs::remove_file(&done);
    Some(results)
}

#[derive(Deserialize)]
struct GraphPage {
    value: Vec<Value>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

async fn graph_window(
    state: &AppState,
    token: &str,
    window: &QaWindow,
    search: &str,
    store: &mut MailStore,
) -> Result<u32> {
    let folder = if window.folder == "Sent Items" {
        "sentitems"
    } else {
        "inbox"
    };
    let mut url = Url::parse(&format!("{GRAPH_ROOT}/me/mailFolders/{folder}/messages"))
        .context("Invalid Microsoft Graph URL")?;
    url.query_pairs_mut()
        .append_pair("$search", &format!("\"{search}\""))
        .append_pair(
            "$select",
            "id,conversationId,subject,receivedDateTime,from,toRecipients,ccRecipients,body,bodyPreview,hasAttachments",
        )
        .append_pair("$top", "50");
    let mut count = 0u32;
    loop {
        let response = state
            .http
            .get(url.clone())
            .bearer_auth(token)
            .header("Prefer", "outlook.body-content-type=\"text\"")
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Message(format!(
                "Microsoft Graph returned {status}: {}",
                body.chars().take(300).collect::<String>()
            )));
        }
        let page: GraphPage = response
            .json()
            .await
            .context("Microsoft Graph returned invalid mail data")?;
        let messages = page
            .value
            .into_iter()
            .map(|item| BridgeMessage {
                id: item["id"].as_str().unwrap_or("").into(),
                conversation_id: item["conversationId"].as_str().unwrap_or("").into(),
                subject: item["subject"].as_str().unwrap_or("").into(),
                received_date_time: item["receivedDateTime"].as_str().unwrap_or("").into(),
                from: item["from"].clone(),
                to_recipients: item["toRecipients"].clone(),
                cc_recipients: item["ccRecipients"].clone(),
                has_attachments: item["hasAttachments"].clone(),
                body_preview: item["bodyPreview"].as_str().unwrap_or("").into(),
                body: item["body"]["content"].as_str().unwrap_or("").into(),
            })
            .collect::<Vec<_>>();
        count += messages.len() as u32;
        ingest_messages(store, messages);
        if count >= QUERY_TOP {
            break;
        }
        match page.next_link {
            Some(next) => {
                let next = Url::parse(&next).context("Microsoft Graph returned an invalid page")?;
                if next.scheme() != "https" || next.host_str() != Some("graph.microsoft.com") {
                    return Err(AppError::Message(
                        "Microsoft Graph returned an unsafe paging destination.".into(),
                    ));
                }
                url = next;
            }
            None => break,
        }
    }
    Ok(count)
}

// ---------------------------------------------------------------------------
// Conversations and evaluation
// ---------------------------------------------------------------------------

struct Candidate {
    case_id: String,
    auditee: QaAuditee,
    conversation_id: String,
    fingerprint: String,
    last_message_at: String,
    messages: Vec<QaMail>,
}

fn conversation_key(mail: &QaMail) -> String {
    if mail.conversation_id.is_empty() {
        let subject = mail.subject.to_lowercase();
        let trimmed = subject
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .trim_start_matches("re:")
            .trim_start_matches("fw:")
            .trim_start_matches("rv:")
            .trim();
        format!("subject:{trimmed}")
    } else {
        mail.conversation_id.clone()
    }
}

fn fingerprint(messages: &[&QaMail]) -> String {
    let mut ids: Vec<&str> = messages.iter().map(|m| m.id.as_str()).collect();
    ids.sort_unstable();
    hex::encode(Sha256::digest(ids.join("|").as_bytes()))
}

/// Finds conversations that need an evaluation and flags reviewed cases
/// that received new mail. Returns (candidates, conversation count).
fn find_candidates(
    config: &QaConfig,
    mail: &MailStore,
    cases: &mut CaseStore,
) -> (Vec<Candidate>, usize) {
    let mut conversations: BTreeMap<String, Vec<&QaMail>> = BTreeMap::new();
    for message in mail.messages.values() {
        conversations
            .entry(conversation_key(message))
            .or_default()
            .push(message);
    }
    let mut candidates = Vec::new();
    let mut relevant = 0usize;
    for (key, mut messages) in conversations {
        if !messages
            .iter()
            .any(|m| qa::subject_matches(&m.subject, &config.subject_keywords))
        {
            continue;
        }
        messages.sort_by(|a, b| a.received.cmp(&b.received));
        let participants: HashSet<String> = messages
            .iter()
            .flat_map(|m| {
                addresses(&m.from)
                    .into_iter()
                    .chain(addresses(&m.to))
                    .chain(addresses(&m.cc))
            })
            .collect();
        let print = fingerprint(&messages);
        let last = messages.last().map(|m| m.received.clone()).unwrap_or_default();
        for auditee in &config.auditees {
            let email = auditee.email.trim().to_lowercase();
            if !participants.contains(&email) {
                continue;
            }
            relevant += 1;
            let case_id = qa::case_id(&key, &email);
            if let Some(record) = cases.cases.get_mut(&case_id) {
                if record.fingerprint == print {
                    continue;
                }
                if record.case.reviewed {
                    // Never overwrite the manager's review; flag it instead.
                    record.case.new_evidence = true;
                    record.fingerprint = print.clone();
                    record.last_message_at = last.clone();
                    continue;
                }
            }
            if cases
                .failures
                .get(&case_id)
                .is_some_and(|f| f.fingerprint == print && f.attempts >= MAX_EVALUATION_ATTEMPTS)
            {
                continue;
            }
            let start = messages.len().saturating_sub(MAX_CONVERSATION_MESSAGES);
            candidates.push(Candidate {
                case_id,
                auditee: auditee.clone(),
                conversation_id: key.clone(),
                fingerprint: print.clone(),
                last_message_at: last.clone(),
                messages: messages[start..].iter().map(|m| (*m).clone()).collect(),
            });
        }
    }
    candidates.sort_by(|a, b| b.last_message_at.cmp(&a.last_message_at));
    (candidates, relevant)
}

fn conversation_evidence(candidate: &Candidate) -> qa::ConversationEvidence {
    let messages = candidate
        .messages
        .iter()
        .filter_map(|m| {
            Some(qa::MessageEvidence {
                id: m.id.clone(),
                author: if m.from.is_empty() {
                    "Unknown sender".into()
                } else {
                    m.from.clone()
                },
                when: DateTime::parse_from_rfc3339(&m.received)
                    .ok()?
                    .with_timezone(&Utc),
                subject: if m.subject.is_empty() {
                    "No subject".into()
                } else {
                    m.subject.clone()
                },
                text: m.text.clone(),
            })
        })
        .collect::<Vec<_>>();
    qa::ConversationEvidence {
        conversation_id: candidate.conversation_id.clone(),
        subject: messages
            .first()
            .map(|m| m.subject.clone())
            .unwrap_or_else(|| "No subject".into()),
        messages,
    }
}

fn set_activity(state: &AppState, activity: Option<String>) {
    if let Ok(mut slot) = state.qa_activity.lock() {
        *slot = activity;
    }
}

async fn evaluate(app: &tauri::AppHandle, state: &AppState, config: &QaConfig) -> Result<()> {
    let mail: MailStore = with_files(state, || Ok(load_json(&mail_path(state))))?;
    let (candidates, conversations) = update_cases(state, |cases| {
        Ok(find_candidates(config, &mail, cases))
    })?;
    drop(mail);
    let total = candidates.len();
    update_sync(state, |sync| {
        sync.conversation_count = conversations;
        sync.pending_evaluation = total;
    })?;
    if candidates.is_empty() {
        return Ok(());
    }
    let started = Instant::now();
    for (index, candidate) in candidates.into_iter().enumerate() {
        if started.elapsed() > EVALUATION_BUDGET {
            break;
        }
        set_activity(
            state,
            Some(format!("evaluating:{}:{}", index + 1, total)),
        );
        let evidence = conversation_evidence(&candidate);
        if evidence.messages.is_empty() {
            continue;
        }
        let outcome =
            qa::evaluate_conversation(state, ollama::BUNDLED_MODEL, &candidate.auditee, &evidence)
                .await;
        let runtime_down = matches!(&outcome, Err(error) if error.to_string().contains("Local AI runtime"));
        update_cases(state, |cases| {
            match &outcome {
                Ok(case) => {
                    cases.failures.remove(&candidate.case_id);
                    cases.cases.insert(
                        candidate.case_id.clone(),
                        CaseRecord {
                            case: case.clone(),
                            fingerprint: candidate.fingerprint.clone(),
                            conversation_id: candidate.conversation_id.clone(),
                            exported: false,
                            evaluated_at: Utc::now().to_rfc3339(),
                            last_message_at: candidate.last_message_at.clone(),
                        },
                    );
                }
                Err(error) => {
                    let entry = cases
                        .failures
                        .entry(candidate.case_id.clone())
                        .or_insert(FailureRecord {
                            fingerprint: candidate.fingerprint.clone(),
                            attempts: 0,
                            error: String::new(),
                        });
                    if entry.fingerprint != candidate.fingerprint {
                        entry.fingerprint = candidate.fingerprint.clone();
                        entry.attempts = 0;
                    }
                    entry.attempts += 1;
                    entry.error = error.to_string();
                }
            }
            Ok(())
        })?;
        update_sync(state, |sync| {
            sync.pending_evaluation = sync.pending_evaluation.saturating_sub(1);
            if let Err(error) = &outcome {
                sync.warn(format!(
                    "Local AI could not evaluate a conversation of {}: {error}",
                    candidate.auditee.name.trim()
                ));
            }
        })?;
        let _ = app.emit("atlas-qa-updated", ());
        if runtime_down {
            break;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

fn export_pending(state: &AppState, config: &QaConfig, full: bool) -> Result<QaExportResult> {
    update_cases(state, |store| {
        if full {
            for record in store.cases.values_mut() {
                record.exported = false;
            }
        }
        let mut dirty: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for record in store.cases.values().filter(|r| !r.exported) {
            dirty
                .entry(record.case.analyst_email.clone())
                .or_default()
                .insert(qa::case_week(&record.case));
        }
        let mut files = Vec::new();
        let mut written = 0usize;
        for (email, weeks) in &dirty {
            let analyst_cases: Vec<&QaCase> = store
                .cases
                .values()
                .filter(|r| &r.case.analyst_email == email && r.case.selected)
                .map(|r| &r.case)
                .collect();
            let name = store
                .cases
                .values()
                .filter(|r| &r.case.analyst_email == email)
                .map(|r| r.case.analyst_name.clone())
                .next_back()
                .unwrap_or_else(|| email.clone());
            let (file, rows) = qa::export_analyst(config, &name, &analyst_cases, weeks)?;
            files.push(file.display().to_string());
            written += rows;
        }
        for record in store.cases.values_mut() {
            record.exported = true;
        }
        Ok(QaExportResult { files, written })
    })
}

// ---------------------------------------------------------------------------
// Tick
// ---------------------------------------------------------------------------

/// One scheduler step. Safe to call often: overlapping calls return at once.
pub async fn tick(app: tauri::AppHandle) {
    let state = app.state::<AppState>();
    let Ok(_guard) = state.qa_engine.try_lock() else {
        return;
    };
    if let Err(error) = tick_inner(&app, &state).await {
        let message = error.to_string();
        diagnostics::error("qa/engine", &message);
        let _ = update_sync(&state, |sync| sync.last_error = Some(message));
    }
    set_activity(&state, None);
    let _ = app.emit("atlas-qa-updated", ());
}

async fn tick_inner(app: &tauri::AppHandle, state: &AppState) -> Result<()> {
    let settings = state.read_settings()?;
    if settings.app_role != Some(AppRole::Manager) {
        return Ok(());
    }
    let config = settings.qa;
    let Some(source) = resolve_source(state)? else {
        return Ok(());
    };
    let now = Local::now();
    let today = now.date_naive();

    // 1. Mail collection.
    set_activity(state, Some("syncing".into()));
    match &source {
        Source::Bridge {
            root,
            installation_id,
        } => {
            prepare_folders(root)?;
            if write_watch(root, installation_id, &watched_emails(&config))? {
                diagnostics::info("qa/engine", "Updated the analyst watch list for Power Automate");
            }
            with_files(state, || {
                let path = mail_path(state);
                let mut store: MailStore = load_json(&path);
                let signalled = ingest_live(root, installation_id, &mut store);
                let mut sync: SyncState = load_json(&sync_path(state));
                if signalled {
                    diagnostics::info("qa/engine", "An audited analyst wrote to the manager; syncing now");
                    sync.incremental_requested = true;
                    sync.last_live_signal_at = Some(Utc::now().to_rfc3339());
                }
                if let Some(pending) = sync.pending.clone() {
                    if let Some(results) = collect_answer(root, installation_id, &pending, &mut store) {
                        sync.pending = None;
                        sync.consecutive_timeouts = 0;
                        sync.last_error = None;
                        for (window, outcome) in results {
                            apply_window_result(&mut sync, window, outcome);
                        }
                    } else {
                        let age = DateTime::parse_from_rfc3339(&pending.created_at)
                            .map(|created| Utc::now() - created.with_timezone(&Utc))
                            .unwrap_or_else(|_| chrono::Duration::zero());
                        if age > chrono::Duration::minutes(REQUEST_TIMEOUT_MINUTES) {
                            sync.pending = None;
                            sync.consecutive_timeouts += 1;
                            for window in pending.windows.into_values().rev() {
                                requeue(&mut sync, window);
                            }
                            sync.warn(format!(
                                "Power Automate did not answer within {REQUEST_TIMEOUT_MINUTES} minutes. Check that 'Atlas QA - Export mailbox evidence' is on and that OneDrive is syncing."
                            ));
                        }
                    }
                }
                maybe_start_historical(&mut sync, &config, today);
                plan_incremental(&mut sync, &config, today, now.time());
                plan_historical(&mut sync, today);
                if sync.pending.is_none() {
                    let windows = next_windows(&mut sync);
                    if !windows.is_empty() {
                        let (request, pending) =
                            build_request(installation_id, &config, &sync, windows);
                        write_request(root, &request)?;
                        diagnostics::info(
                            "qa/engine",
                            &format!(
                                "Requested {} mailbox windows from Power Automate ({})",
                                request.queries.len(),
                                request.request_id
                            ),
                        );
                        sync.pending = Some(pending);
                    }
                }
                close_incremental_if_drained(&mut sync);
                finish_historical(state, &mut sync)?;
                sync.message_count = store.messages.len();
                save_json(&path, &store)?;
                save_json(&sync_path(state), &sync)
            })?;
        }
        Source::Graph => {
            let token = auth::access_token(state).await?;
            let (windows, sync_snapshot) = with_files(state, || {
                let mut sync: SyncState = load_json(&sync_path(state));
                maybe_start_historical(&mut sync, &config, today);
                plan_incremental(&mut sync, &config, today, now.time());
                plan_historical(&mut sync, today);
                let windows = next_windows(&mut sync);
                save_json(&sync_path(state), &sync)?;
                Ok((windows, sync))
            })?;
            let all = all_emails(&config);
            let mut results = Vec::new();
            let mut store: MailStore = with_files(state, || Ok(load_json(&mail_path(state))))?;
            for window in windows {
                let emails = if window.historical {
                    &sync_snapshot.historical_auditees
                } else {
                    &all
                };
                let search =
                    search_query(&config.subject_keywords, emails, window.start, window.end);
                let outcome = match graph_window(state, &token, &window, &search, &mut store).await {
                    Ok(count) => Some(count),
                    Err(error) => {
                        diagnostics::error("qa/graph", &error.to_string());
                        None
                    }
                };
                results.push((window, outcome));
            }
            with_files(state, || {
                let mut sync: SyncState = load_json(&sync_path(state));
                for (window, outcome) in results {
                    apply_window_result(&mut sync, window, outcome);
                }
                close_incremental_if_drained(&mut sync);
                finish_historical(state, &mut sync)?;
                sync.message_count = store.messages.len();
                save_json(&mail_path(state), &store)?;
                save_json(&sync_path(state), &sync)
            })?;
        }
    }
    let _ = app.emit("atlas-qa-updated", ());

    // 2. Evaluation of new or changed conversations.
    if !config.auditees.is_empty() {
        evaluate(app, state, &config).await?;
    }

    // 3. Excel.
    if config.auto_export && config.output_folder.is_some() {
        set_activity(state, Some("exporting".into()));
        let outcome = export_pending(state, &config, false);
        update_sync(state, |sync| match &outcome {
            Ok(result) => {
                if result.written > 0 || !result.files.is_empty() {
                    sync.last_export_at = Some(Utc::now().to_rfc3339());
                    diagnostics::info(
                        "qa/export",
                        &format!(
                            "Wrote {} QA rows into {} workbook(s)",
                            result.written,
                            result.files.len()
                        ),
                    );
                }
                sync.last_export_error = None;
            }
            Err(error) => {
                sync.last_export_error = Some(error.to_string());
                diagnostics::error("qa/export", &error.to_string());
            }
        })?;
    }
    Ok(())
}

fn maybe_start_historical(sync: &mut SyncState, config: &QaConfig, today: NaiveDate) {
    if sync.historical_status == HistoricalStatus::Running {
        return;
    }
    let pending: Vec<String> = config
        .auditees
        .iter()
        .filter(|auditee| !auditee.historical_done)
        .map(|auditee| auditee.email.trim().to_lowercase())
        .filter(|email| email.contains('@'))
        .collect();
    if pending.is_empty() {
        return;
    }
    diagnostics::info(
        "qa/engine",
        &format!(
            "Starting the historical QA import for {} analyst(s)",
            pending.len()
        ),
    );
    start_historical(sync, pending, config.history_months, today);
}

fn finish_historical(state: &AppState, sync: &mut SyncState) -> Result<()> {
    if !historical_finished(sync) {
        return Ok(());
    }
    sync.historical_status = HistoricalStatus::Done;
    sync.historical_completed_at = Some(Utc::now().to_rfc3339());
    let done: HashSet<String> = sync.historical_auditees.iter().cloned().collect();
    diagnostics::info(
        "qa/engine",
        &format!(
            "Historical QA import finished after {} month(s)",
            sync.historical_months_planned
        ),
    );
    state.update_settings(|settings| {
        for auditee in &mut settings.qa.auditees {
            if done.contains(&auditee.email.trim().to_lowercase()) {
                auditee.historical_done = true;
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QaEngineStatus {
    pub source: &'static str,
    pub connected: bool,
    pub historical_status: HistoricalStatus,
    pub historical_months_done: u32,
    pub historical_months_planned: u32,
    pub historical_horizon_months: u32,
    pub historical_oldest_month: Option<String>,
    pub historical_started_at: Option<String>,
    pub historical_completed_at: Option<String>,
    pub pending_request_at: Option<String>,
    pub queued_windows: usize,
    pub last_incremental_at: Option<String>,
    pub next_check: Option<String>,
    pub messages: usize,
    pub conversations: usize,
    pub pending_evaluation: usize,
    pub cases: usize,
    pub needs_review: usize,
    pub reviewed: usize,
    pub new_evidence: usize,
    pub unexported: usize,
    pub activity: Option<String>,
    pub last_error: Option<String>,
    pub warnings: Vec<String>,
    pub last_export_at: Option<String>,
    pub last_export_error: Option<String>,
    pub last_live_signal_at: Option<String>,
    pub consecutive_timeouts: u32,
}

fn next_check(config: &QaConfig, now: DateTime<Local>) -> Option<String> {
    let time = now.time();
    let mut slots: Vec<NaiveTime> = [&config.check_morning, &config.check_afternoon]
        .iter()
        .filter_map(|value| NaiveTime::parse_from_str(value.trim(), "%H:%M").ok())
        .collect();
    slots.sort();
    let first = *slots.first()?;
    Some(match slots.into_iter().find(|slot| *slot > time) {
        Some(slot) => slot.format("%H:%M").to_string(),
        None => format!("+1 {}", first.format("%H:%M")),
    })
}

pub fn status(state: &AppState) -> Result<QaEngineStatus> {
    let settings = state.read_settings()?;
    let source = resolve_source(state)?;
    let (sync, cases): (SyncState, CaseStore) = with_files(state, || {
        Ok((load_json(&sync_path(state)), load_json(&cases_path(state))))
    })?;
    let open = open_months(&sync);
    let months_done = sync
        .month_totals
        .keys()
        .filter(|month| !open.contains(*month))
        .count() as u32;
    let records = cases.cases.values();
    let (mut needs_review, mut reviewed, mut new_evidence, mut unexported) = (0, 0, 0, 0);
    for record in records {
        if record.case.reviewed {
            reviewed += 1;
        } else {
            needs_review += 1;
        }
        if record.case.new_evidence {
            new_evidence += 1;
        }
        if !record.exported {
            unexported += 1;
        }
    }
    Ok(QaEngineStatus {
        source: match (&source, settings.source_mode) {
            (Some(Source::Bridge { .. }), _) => "power_automate",
            (Some(Source::Graph), _) => "microsoft_graph",
            (None, _) => "not_configured",
        },
        connected: source.is_some(),
        historical_status: sync.historical_status,
        historical_months_done: months_done,
        historical_months_planned: sync.historical_months_planned,
        historical_horizon_months: settings.qa.history_months,
        historical_oldest_month: sync.month_totals.keys().next().cloned(),
        historical_started_at: sync.historical_started_at,
        historical_completed_at: sync.historical_completed_at,
        pending_request_at: sync.pending.as_ref().map(|p| p.created_at.clone()),
        queued_windows: sync.historical_queue.len() + sync.incremental_queue.len(),
        last_incremental_at: sync.last_incremental_at,
        next_check: next_check(&settings.qa, Local::now()),
        messages: sync.message_count,
        conversations: sync.conversation_count,
        pending_evaluation: sync.pending_evaluation,
        cases: cases.cases.len(),
        needs_review,
        reviewed,
        new_evidence,
        unexported,
        activity: state.qa_activity.lock().ok().and_then(|slot| slot.clone()),
        last_error: sync.last_error,
        warnings: sync.warnings,
        last_export_at: sync.last_export_at,
        last_export_error: sync.last_export_error,
        last_live_signal_at: sync.last_live_signal_at,
        consecutive_timeouts: sync.consecutive_timeouts,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QaCaseEntry {
    #[serde(flatten)]
    pub case: QaCase,
    pub exported: bool,
    pub last_message_at: String,
}

pub fn list_cases(state: &AppState) -> Result<Vec<QaCaseEntry>> {
    let store: CaseStore = with_files(state, || Ok(load_json(&cases_path(state))))?;
    let mut entries: Vec<QaCaseEntry> = store
        .cases
        .into_values()
        .map(|record| QaCaseEntry {
            case: record.case,
            exported: record.exported,
            last_message_at: record.last_message_at,
        })
        .collect();
    entries.sort_by(|a, b| b.last_message_at.cmp(&a.last_message_at));
    Ok(entries)
}

pub fn update_case(state: &AppState, mut case: QaCase) -> Result<()> {
    update_cases(state, |store| {
        let record = store
            .cases
            .get_mut(&case.case_id)
            .ok_or_else(|| AppError::Message("This QA case no longer exists.".into()))?;
        // Identity and evidence always come from the stored evaluation.
        case.analyst_email = record.case.analyst_email.clone();
        case.analyst_name = record.case.analyst_name.clone();
        case.evidence = record.case.evidence.clone();
        if case.reviewed {
            case.new_evidence = false;
        }
        record.case = case;
        record.exported = false;
        Ok(())
    })
}

/// Drops the manager's review so the conversation is evaluated again.
pub fn reevaluate_case(state: &AppState, case_id: &str) -> Result<()> {
    update_cases(state, |store| {
        store.failures.remove(case_id);
        store
            .cases
            .remove(case_id)
            .map(|_| ())
            .ok_or_else(|| AppError::Message("This QA case no longer exists.".into()))
    })
}

pub fn request_sync(state: &AppState) -> Result<()> {
    update_sync(state, |sync| {
        sync.incremental_requested = true;
        sync.last_error = None;
    })
}

/// Marks every analyst as pending so the next tick imports the full history.
pub fn restart_historical(state: &AppState) -> Result<()> {
    state.update_settings(|settings| {
        for auditee in &mut settings.qa.auditees {
            auditee.historical_done = false;
        }
    })?;
    update_sync(state, |sync| {
        sync.historical_status = HistoricalStatus::Idle;
        sync.historical_queue.clear();
        sync.historical_next_month = None;
    })
}

pub fn export_now(state: &AppState, full: bool) -> Result<QaExportResult> {
    let config = state.read_settings()?.qa;
    let result = export_pending(state, &config, full);
    update_sync(state, |sync| match &result {
        Ok(_) => {
            sync.last_export_at = Some(Utc::now().to_rfc3339());
            sync.last_export_error = None;
        }
        Err(error) => sync.last_export_error = Some(error.to_string()),
    })?;
    result
}

/// Keeps the live watch list in step with the saved configuration.
pub fn sync_watch_list(state: &AppState) -> Result<()> {
    if let Some(Source::Bridge {
        root,
        installation_id,
    }) = resolve_source(state)?
    {
        prepare_folders(&root)?;
        write_watch(&root, &installation_id, &watched_emails(&state.read_settings()?.qa))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn search_query_combines_keywords_participants_and_dates() {
        let query = search_query(
            &["QA".into(), "Quality review".into()],
            &["ana@example.com".into(), "luis@example.com".into()],
            date("2026-08-01"),
            date("2026-08-31"),
        );
        assert_eq!(
            query,
            "(subject:QA OR (subject:Quality AND subject:review)) AND (participants:ana@example.com OR participants:luis@example.com) AND received>=2026-08-01 AND received<=2026-08-31"
        );
        assert!(!query.contains('"'));
    }

    #[test]
    fn historical_plan_walks_back_month_by_month_until_the_horizon() {
        let mut sync = SyncState::default();
        let today = date("2026-09-23");
        start_historical(&mut sync, vec!["ana@example.com".into()], 3, today);
        plan_historical(&mut sync, today);
        assert_eq!(sync.historical_months_planned, 3);
        assert_eq!(sync.historical_queue.len(), 6);
        assert_eq!(sync.historical_queue[0].start, date("2026-09-01"));
        assert_eq!(sync.historical_queue[0].end, today);
        assert_eq!(sync.historical_queue[5].start, date("2026-07-01"));
        assert_eq!(sync.historical_queue[5].end, date("2026-07-31"));
        assert!(sync.historical_next_month.is_none());
        assert_eq!(sync.last_incremental_until, Some(today));
    }

    #[test]
    fn saturated_windows_split_and_failures_retry_then_warn() {
        let mut sync = SyncState::default();
        let window = QaWindow {
            start: date("2026-08-01"),
            end: date("2026-08-31"),
            folder: "Inbox".into(),
            historical: true,
            attempts: 0,
        };
        apply_window_result(&mut sync, window.clone(), Some(QUERY_TOP));
        assert_eq!(sync.historical_queue.len(), 2);
        assert_eq!(sync.historical_queue[0].end, date("2026-08-16"));
        assert_eq!(sync.historical_queue[1].start, date("2026-08-17"));
        let mut failing = SyncState::default();
        apply_window_result(&mut failing, window.clone(), None);
        assert_eq!(failing.historical_queue[0].attempts, 1);
        let exhausted = QaWindow {
            attempts: MAX_WINDOW_ATTEMPTS - 1,
            ..window
        };
        apply_window_result(&mut failing, exhausted, None);
        assert_eq!(failing.historical_queue.len(), 1);
        assert_eq!(failing.warnings.len(), 1);
    }

    #[test]
    fn empty_old_months_stop_the_historical_import() {
        let mut sync = SyncState::default();
        let today = date("2026-09-23");
        start_historical(&mut sync, vec!["ana@example.com".into()], 60, today);
        let mut rounds = 0;
        while sync.historical_status == HistoricalStatus::Running && rounds < 200 {
            plan_historical(&mut sync, today);
            for window in next_windows(&mut sync) {
                apply_window_result(&mut sync, window, Some(0));
            }
            if historical_finished(&sync) {
                sync.historical_status = HistoricalStatus::Done;
            }
            rounds += 1;
        }
        assert_eq!(sync.historical_status, HistoricalStatus::Done);
        assert!(sync.historical_months_planned < 60);
        assert!(sync.historical_months_planned >= MIN_HISTORY_MONTHS);
    }

    #[test]
    fn twice_daily_slots_request_one_incremental_each() {
        let config = QaConfig::default();
        let mut sync = SyncState {
            last_incremental_until: Some(date("2026-09-20")),
            ..SyncState::default()
        };
        let today = date("2026-09-23");
        plan_incremental(&mut sync, &config, today, NaiveTime::from_hms_opt(8, 0, 0).unwrap());
        assert!(sync.incremental_queue.is_empty());
        plan_incremental(&mut sync, &config, today, NaiveTime::from_hms_opt(9, 5, 0).unwrap());
        assert_eq!(sync.incremental_queue.len(), 2);
        assert_eq!(sync.incremental_queue[0].start, date("2026-09-19"));
        for window in next_windows(&mut sync) {
            apply_window_result(&mut sync, window, Some(3));
        }
        close_incremental_if_drained(&mut sync);
        assert_eq!(sync.last_incremental_until, Some(today));
        plan_incremental(&mut sync, &config, today, NaiveTime::from_hms_opt(9, 30, 0).unwrap());
        assert!(sync.incremental_queue.is_empty());
        plan_incremental(&mut sync, &config, today, NaiveTime::from_hms_opt(15, 1, 0).unwrap());
        assert_eq!(sync.incremental_queue.len(), 2);
    }

    #[test]
    fn bridge_answers_are_ingested_and_cleaned_up() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        prepare_folders(root).unwrap();
        let config = QaConfig {
            auditees: vec![QaAuditee {
                name: "Ana".into(),
                email: "Ana@Example.com".into(),
                custom_rules: String::new(),
                watched: true,
                historical_done: false,
            }],
            ..QaConfig::default()
        };
        let sync = SyncState::default();
        let windows = incremental_windows(date("2026-09-20"), date("2026-09-23"));
        let (request, pending) = build_request("inst-1", &config, &sync, windows);
        write_request(root, &request).unwrap();
        let written: BridgeRequest =
            serde_json::from_slice(&fs::read(request_file(root)).unwrap()).unwrap();
        assert_eq!(written.queries.len(), 2);
        assert!(written.queries[0].search.contains("participants:ana@example.com"));
        let inbox = inbox_dir(root);
        let mut store = MailStore::default();
        assert!(collect_answer(root, "inst-1", &pending, &mut store).is_none());
        fs::write(
            inbox.join(answer_name("inst-1", &request.request_id, "q1")),
            r#"{"contract":"atlas-qa-v1","installationId":"inst-1","status":"ok","count":1,"messages":[{"id":"m1","conversationId":"c1","subject":"QA follow-up","receivedDateTime":"2026-09-21T10:00:00Z","from":"ana@example.com","toRecipients":"client@example.com","ccRecipients":"","body":"<html><style>p{}</style><p>Hello&nbsp;there</p></html>"}]}"#,
        )
        .unwrap();
        fs::write(
            inbox.join(answer_name("inst-1", &request.request_id, "q2")),
            r#"{"contract":"atlas-qa-v1","installationId":"inst-1","status":"failed","count":0,"messages":[]}"#,
        )
        .unwrap();
        fs::write(
            inbox.join(answer_name("inst-1", &request.request_id, "done")),
            r#"{"contract":"atlas-qa-v1","status":"done"}"#,
        )
        .unwrap();
        let results = collect_answer(root, "inst-1", &pending, &mut store).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].1, Some(1));
        assert_eq!(results[1].1, None);
        assert_eq!(store.messages["m1"].text, "Hello there");
        assert_eq!(fs::read_dir(&inbox).unwrap().flatten().filter(|e| e.path().is_file()).count(), 0);
    }

    #[test]
    fn ping_answer_proves_the_installation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        prepare_folders(root).unwrap();
        let ping = ping_request("inst-9");
        assert_eq!(ping_state(root, "inst-9", &ping.request_id), PingState::Waiting);
        fs::write(
            inbox_dir(root).join(answer_name("inst-9", &ping.request_id, "ping")),
            r#"{"contract":"atlas-qa-v1","installationId":"inst-9","status":"ok","count":1,"messages":[]}"#,
        )
        .unwrap();
        fs::write(
            inbox_dir(root).join(answer_name("inst-9", &ping.request_id, "done")),
            r#"{"contract":"atlas-qa-v1","status":"done"}"#,
        )
        .unwrap();
        assert_eq!(ping_state(root, "inst-9", &ping.request_id), PingState::Answered);
    }

    #[test]
    fn watch_list_is_rewritten_only_when_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        prepare_folders(dir.path()).unwrap();
        let emails = vec!["ana@example.com".to_string()];
        assert!(write_watch(dir.path(), "inst", &emails).unwrap());
        assert!(!write_watch(dir.path(), "inst", &emails).unwrap());
        assert!(write_watch(dir.path(), "inst", &[]).unwrap());
    }

    #[test]
    fn conversations_are_matched_to_audited_participants() {
        let config = QaConfig {
            auditees: vec![QaAuditee {
                name: "Ana".into(),
                email: "ana@example.com".into(),
                custom_rules: String::new(),
                watched: true,
                historical_done: true,
            }],
            ..QaConfig::default()
        };
        let mut mail = MailStore::default();
        for (id, conversation, subject, from, to) in [
            ("m1", "c1", "QA request", "Client <client@example.com>", "Ana <ana@example.com>"),
            ("m2", "c1", "RE: QA request", "ana@example.com", "client@example.com"),
            ("m3", "c2", "Lunch", "ana@example.com", "client@example.com"),
            ("m4", "c3", "QA other team", "bob@example.com", "carl@example.com"),
        ] {
            mail.messages.insert(
                id.into(),
                QaMail {
                    id: id.into(),
                    conversation_id: conversation.into(),
                    subject: subject.into(),
                    received: format!("2026-09-2{}T10:00:00+00:00", &id[1..]),
                    from: from.into(),
                    to: to.into(),
                    cc: String::new(),
                    text: "text".into(),
                    has_attachments: false,
                },
            );
        }
        let mut cases = CaseStore::default();
        let (candidates, relevant) = find_candidates(&config, &mail, &mut cases);
        assert_eq!(relevant, 1);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].messages.len(), 2);
        assert_eq!(candidates[0].case_id, qa::case_id("c1", "ana@example.com"));
    }
}
