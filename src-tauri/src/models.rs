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
pub enum TrackerDestinationKind {
    LocalExisting,
    LocalNew,
    SharePoint,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrackerDestination {
    pub kind: TrackerDestinationKind,
    pub value: String,
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
    pub microsoft_config: MicrosoftConfig,
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
    pub log_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub profile: Option<UserProfile>,
    pub account: Option<AccountInfo>,
    #[serde(default)]
    pub microsoft_config: Option<MicrosoftConfig>,
    #[serde(default)]
    pub destination: Option<TrackerDestination>,
    #[serde(default = "default_auto_sync")]
    pub auto_sync: bool,
}

fn default_auto_sync() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            profile: None,
            account: None,
            microsoft_config: None,
            destination: None,
            auto_sync: default_auto_sync(),
        }
    }
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

#[derive(Debug, Serialize)]
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
}
