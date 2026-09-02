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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub configured: bool,
    pub signed_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<UserProfile>,
    pub ollama_running: bool,
    pub ollama_model_available: bool,
    pub ollama_model: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub profile: Option<UserProfile>,
    pub account: Option<AccountInfo>,
    #[serde(default = "default_model")]
    pub ollama_model: String,
}

fn default_model() -> String {
    "qwen2.5:3b".into()
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
