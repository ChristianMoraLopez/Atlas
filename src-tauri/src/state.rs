use crate::{
    error::{Context, Result},
    models::{Interaction, Settings},
};
use std::{collections::HashMap, fs, path::PathBuf, sync::Mutex};

#[derive(Clone)]
pub struct AzureConfig {
    pub client_id: String,
    pub tenant_id: String,
}

pub struct AppState {
    pub azure: AzureConfig,
    pub http: reqwest::Client,
    pub settings_path: PathBuf,
    pub settings: Mutex<Settings>,
    pub verified_sources: Mutex<HashMap<String, Interaction>>,
}

impl AppState {
    pub fn new(config_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&config_dir)
            .context("Unable to create the application settings directory")?;
        let settings_path = config_dir.join("settings.json");
        let settings = if settings_path.exists() {
            serde_json::from_slice(
                &fs::read(&settings_path).context("Unable to read local settings")?,
            )
            .context("Local settings are invalid")?
        } else {
            Settings::default()
        };
        let http = reqwest::Client::builder()
            .user_agent("Atlas-Circana-Tracker/0.1")
            .build()?;
        Ok(Self {
            azure: AzureConfig {
                client_id: option_env!("CIRCANA_AZURE_CLIENT_ID")
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                tenant_id: option_env!("CIRCANA_AZURE_TENANT_ID")
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            },
            http,
            settings_path,
            settings: Mutex::new(settings),
            verified_sources: Mutex::new(HashMap::new()),
        })
    }

    pub fn read_settings(&self) -> Result<Settings> {
        self.settings
            .lock()
            .map(|v| v.clone())
            .map_err(|_| crate::error::AppError::Message("Settings lock was poisoned".into()))
    }

    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) -> Result<()> {
        let mut guard = self
            .settings
            .lock()
            .map_err(|_| crate::error::AppError::Message("Settings lock was poisoned".into()))?;
        f(&mut guard);
        let json = serde_json::to_vec_pretty(&*guard)?;
        fs::write(&self.settings_path, json).context("Unable to save local settings")
    }
}
