use crate::{
    diagnostics,
    error::{Context, Result},
    models::{
        CachedAccessToken, Interaction, MicrosoftConfig, Settings, TrackerDestination,
        TrackerDestinationKind,
    },
    ollama::ManagedRuntime,
};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

pub struct AppState {
    build_microsoft_config: MicrosoftConfig,
    pub http: reqwest::Client,
    pub settings_path: PathBuf,
    pub settings: Mutex<Settings>,
    pub verified_sources: Mutex<HashMap<String, Interaction>>,
    pub cached_access_token: Mutex<Option<CachedAccessToken>>,
    pub local_ai: ManagedRuntime,
    pub connector_installer: Arc<crate::connector_installer::Installer>,
}

impl AppState {
    pub fn new(config_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&config_dir)
            .context("Unable to create the application settings directory")?;
        let settings_path = config_dir.join("settings.json");
        let mut settings = if settings_path.exists() {
            let bytes = fs::read(&settings_path).context("Unable to read local settings")?;
            match serde_json::from_slice(&bytes) {
                Ok(settings) => settings,
                Err(error) => {
                    let backup = settings_path
                        .with_file_name(format!("settings.invalid-{}.json", uuid::Uuid::new_v4()));
                    let recovery = fs::rename(&settings_path, &backup)
                        .map(|_| format!(" A backup was saved at {}.", backup.display()))
                        .unwrap_or_else(|move_error| {
                            format!(" The invalid file could not be moved: {move_error}.")
                        });
                    diagnostics::error(
                        "settings",
                        &format!("Invalid settings were reset: {error}.{recovery}"),
                    );
                    Settings::default()
                }
            }
        } else {
            Settings::default()
        };
        if settings.destination.is_none() {
            let one_drive = std::env::var("OneDriveCommercial")
                .or_else(|_| std::env::var("OneDrive"))
                .ok()
                .map(PathBuf::from)
                .filter(|path| path.is_dir());
            if let Some(root) = one_drive {
                let path = root.join("Tracker_Circana.xlsx");
                settings.destination = Some(TrackerDestination {
                    kind: if path.is_file() {
                        TrackerDestinationKind::LocalExisting
                    } else {
                        TrackerDestinationKind::LocalNew
                    },
                    value: path.to_string_lossy().into_owned(),
                });
                fs::write(&settings_path, serde_json::to_vec_pretty(&settings)?)
                    .context("Unable to save the default OneDrive tracker destination")?;
            }
        }
        let http = reqwest::Client::builder()
            .user_agent("Atlas-Circana-Tracker/0.2")
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            build_microsoft_config: MicrosoftConfig {
                client_id: option_env!("CIRCANA_AZURE_CLIENT_ID")
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                tenant_id: option_env!("CIRCANA_AZURE_TENANT_ID")
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("organizations")
                    .trim()
                    .to_string(),
            },
            http,
            settings_path,
            settings: Mutex::new(settings),
            verified_sources: Mutex::new(HashMap::new()),
            cached_access_token: Mutex::new(None),
            local_ai: ManagedRuntime::discover(config_dir.join("logs").join("local-ai.log")),
            connector_installer: Arc::new(crate::connector_installer::Installer::new(
                config_dir.join("connector-installer"),
            )),
        })
    }

    pub fn read_settings(&self) -> Result<Settings> {
        self.settings
            .lock()
            .map(|v| v.clone())
            .map_err(|_| crate::error::AppError::Message("Settings lock was poisoned".into()))
    }

    pub fn microsoft_config(&self) -> Result<MicrosoftConfig> {
        Ok(self.build_microsoft_config.clone())
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
