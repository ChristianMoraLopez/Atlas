use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub login_id: String,
    pub full_name: String,
    pub area: String,
    pub team_lead: String,
    pub circana_manager: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub display_name: String,
    pub email: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MicrosoftConfig {
    pub client_id: String,
    pub tenant_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppRole {
    /// Customer service agent: daily interactions tracker.
    Cs,
    /// Team manager: QA audits of the team's conversations.
    Manager,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceMode {
    #[default]
    MicrosoftGraph,
    PowerAutomateFolder,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrackerDestinationKind {
    LocalExisting,
    LocalNew,
    SharePoint,
    SharePointFlow,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationMode {
    #[default]
    StartupPreviousWorkday,
    DailyTime,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiInstructions {
    #[serde(default)]
    pub presets: Vec<String>,
    #[serde(default)]
    pub custom: String,
}

impl Default for AiInstructions {
    fn default() -> Self {
        Self {
            presets: Vec::new(),
            custom: String::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrackerDestination {
    pub kind: TrackerDestinationKind,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writer_package_path: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CachedAccessToken {
    pub value: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub configured: bool,
    pub signed_in: bool,
    pub source_mode: SourceMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridge_folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<UserProfile>,
    pub ollama_running: bool,
    pub ollama_model_available: bool,
    pub ollama_model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_ai_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<TrackerDestination>,
    pub auto_sync: bool,
    pub auto_sync_time: String,
    pub automation_mode: AutomationMode,
    pub language: String,
    pub ai_instructions: AiInstructions,
    pub scheduled_launch: bool,
    pub log_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_role: Option<AppRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qa_bridge_folder: Option<String>,
    /// Name captured for the personalized Power Automate solution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solution_owner: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub profile: Option<UserProfile>,
    pub account: Option<AccountInfo>,
    #[serde(default)]
    pub source_mode: SourceMode,
    #[serde(default)]
    pub bridge_folder: Option<String>,
    #[serde(default)]
    pub microsoft_config: Option<MicrosoftConfig>,
    #[serde(default)]
    pub destination: Option<TrackerDestination>,
    #[serde(default = "default_auto_sync")]
    pub auto_sync: bool,
    #[serde(default = "default_auto_sync_time")]
    pub auto_sync_time: String,
    #[serde(default)]
    pub automation_mode: AutomationMode,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub ai_instructions: AiInstructions,
    #[serde(default)]
    pub qa: QaConfig,
    #[serde(default)]
    pub app_role: Option<AppRole>,
    /// Local OneDrive folder synced with the personalized AtlasQA flows
    /// (`<OneDrive>\AtlasBridge\qa`).
    #[serde(default)]
    pub qa_bridge_folder: Option<String>,
}

fn default_auto_sync() -> bool {
    true
}

fn default_auto_sync_time() -> String {
    crate::automation::DEFAULT_DAILY_TIME.into()
}

fn default_language() -> String {
    "es".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            profile: None,
            account: None,
            source_mode: SourceMode::default(),
            bridge_folder: None,
            microsoft_config: None,
            destination: None,
            auto_sync: default_auto_sync(),
            auto_sync_time: default_auto_sync_time(),
            automation_mode: AutomationMode::default(),
            language: default_language(),
            ai_instructions: AiInstructions::default(),
            qa: QaConfig::default(),
            app_role: None,
            qa_bridge_folder: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRequest {
    pub date: String,
    pub run_key: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Calendar,
    Email,
    TeamsChat,
    Manual,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Interaction {
    pub source_kind: SourceKind,
    pub source_id: String,
    pub interaction_type: String,
    pub reception_date_time: String,
    pub interaction_date_time: String,
    pub resolution_date_time: Option<String>,
    pub client_type: String,
    pub end_client: String,
    pub status: String,
    pub resolution_type: String,
    pub category: String,
    pub subcategory: String,
    pub priority: String,
    pub incident_number: String,
    pub comments: String,
    pub selected: bool,
    pub reviewed: bool,
    pub manual_authored: bool,
    pub ai_suggested: bool,
    pub evidence_label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionResult {
    pub interactions: Vec<Interaction>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub inserted: usize,
    pub updated: usize,
    pub skipped: usize,
    #[serde(default)]
    pub queued: bool,
}

// ---------------------------------------------------------------------------
// QA Audit module models
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QaAuditee {
    pub name: String,
    pub email: String,
    /// Per-person manager rules injected into the AI prompt for this auditee
    /// (e.g. "response-time ranges do not apply, this CSA works ticket-based").
    #[serde(default)]
    pub custom_rules: String,
    /// Whether new mail from this person triggers a QA check notification.
    #[serde(default = "default_true")]
    pub watched: bool,
    /// False until the full historical audit for this person has run.
    #[serde(default)]
    pub historical_done: bool,
}

fn default_true() -> bool {
    true
}

fn default_qa_lookback_days() -> u32 {
    7
}

fn default_qa_vertical() -> String {
    "320 - CLIENT SERVICE BEAUTY, HEALTH & WELLNESS".into()
}

fn default_qa_check_morning() -> String {
    "09:00".into()
}

fn default_qa_check_afternoon() -> String {
    "15:00".into()
}

fn default_qa_history_months() -> u32 {
    36
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QaConfig {
    #[serde(default)]
    pub auditees: Vec<QaAuditee>,
    #[serde(default)]
    pub output_folder: Option<String>,
    /// Days covered by the first incremental sync when no historical import
    /// has run yet.
    #[serde(default = "default_qa_lookback_days")]
    pub lookback_days: u32,
    #[serde(default = "default_qa_vertical")]
    pub vertical: String,
    /// When enabled, new mail from a watched analyst triggers an immediate
    /// sync and evaluation.
    #[serde(default = "default_true")]
    pub watch_enabled: bool,
    /// Twice-daily sync times (HH:MM, local time).
    #[serde(default = "default_qa_check_morning")]
    pub check_morning: String,
    #[serde(default = "default_qa_check_afternoon")]
    pub check_afternoon: String,
    /// Subject keywords: only conversations whose topic mentions one of
    /// these (case-insensitive) are audited. Default: ["QA"].
    #[serde(default = "default_qa_subject_keywords")]
    pub subject_keywords: Vec<String>,
    /// Oldest month the historical import may reach. The import also stops
    /// earlier after six consecutive empty months.
    #[serde(default = "default_qa_history_months")]
    pub history_months: u32,
    /// Write evaluated cases into the per-analyst workbooks automatically.
    #[serde(default = "default_true")]
    pub auto_export: bool,
}

fn default_qa_subject_keywords() -> Vec<String> {
    vec!["QA".into()]
}

impl Default for QaConfig {
    fn default() -> Self {
        Self {
            auditees: Vec::new(),
            output_folder: None,
            lookback_days: default_qa_lookback_days(),
            vertical: default_qa_vertical(),
            watch_enabled: true,
            check_morning: default_qa_check_morning(),
            check_afternoon: default_qa_check_afternoon(),
            subject_keywords: default_qa_subject_keywords(),
            history_months: default_qa_history_months(),
            auto_export: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QaEvidenceRef {
    pub message_id: String,
    pub label: String,
    pub excerpt: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QaCase {
    pub case_id: String,
    pub analyst_name: String,
    pub analyst_email: String,
    pub audit_date: String,
    pub request_id: String,
    pub request_date: String,
    pub request_source: String,
    pub initial_response: String,
    pub initial_response_notes: String,
    pub customer_sentiment: String,
    pub customer_sentiment_notes: String,
    pub adherence: String,
    pub adherence_notes: String,
    pub status: String,
    pub status_notes: String,
    pub update_follow_up: String,
    pub update_follow_up_notes: String,
    pub auto_fail: String,
    pub evidence: Vec<QaEvidenceRef>,
    pub selected: bool,
    pub reviewed: bool,
    /// The conversation received new messages after the manager reviewed it.
    #[serde(default)]
    pub new_evidence: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QaExportResult {
    pub files: Vec<String>,
    pub written: usize,
}
