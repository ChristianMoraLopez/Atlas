mod auth;
mod bridge;
mod connector_installer;
mod diagnostics;
mod error;
mod evidence_validation;
mod excel;
mod graph;
mod models;
mod ollama;
mod sharepoint;
mod state;

use crate::{
    error::{AppError, Result},
    models::{
        AppStatus, ExportResult, ExtractionResult, Interaction, MicrosoftConfig, SourceKind,
        SourceMode, TrackerDestination, TrackerDestinationKind, UserProfile,
    },
    state::AppState,
};
use std::{collections::HashSet, path::Path};
use tauri::Manager;

async fn build_status(state: &AppState) -> Result<AppStatus> {
    let settings = state.read_settings()?;
    let microsoft_config = state.microsoft_config()?;
    let (ollama_running, ollama_model_available, local_ai_error) =
        state.local_ai.status(&state.http).await;
    let token_available = match auth::has_token() {
        Ok(value) => value,
        Err(error) => {
            diagnostics::error("auth/status", &error.to_string());
            false
        }
    };
    let configured = match settings.source_mode {
        SourceMode::MicrosoftGraph => {
            !microsoft_config.client_id.is_empty() && !microsoft_config.tenant_id.is_empty()
        }
        SourceMode::PowerAutomateFolder => settings
            .bridge_folder
            .as_deref()
            .is_some_and(|folder| Path::new(folder).is_dir()),
    };
    Ok(AppStatus {
        configured,
        signed_in: match settings.source_mode {
            SourceMode::MicrosoftGraph => token_available && settings.account.is_some(),
            SourceMode::PowerAutomateFolder => configured,
        },
        source_mode: settings.source_mode,
        bridge_folder: settings.bridge_folder,
        microsoft_config,
        account: settings.account,
        profile: settings.profile,
        ollama_running,
        ollama_model_available,
        ollama_model: ollama::BUNDLED_MODEL.into(),
        local_ai_error,
        destination: settings.destination,
        auto_sync: settings.auto_sync,
        log_path: diagnostics::path(),
    })
}

#[tauri::command]
async fn get_app_status(state: tauri::State<'_, AppState>) -> Result<AppStatus> {
    build_status(&state).await
}

#[tauri::command]
async fn sign_in(state: tauri::State<'_, AppState>) -> Result<AppStatus> {
    let include_files = state
        .read_settings()?
        .destination
        .is_some_and(|destination| destination.kind == TrackerDestinationKind::SharePoint);
    let account = auth::sign_in(&state, include_files).await?;
    state.update_settings(|settings| settings.account = Some(account))?;
    build_status(&state).await
}

fn normalize_microsoft_config(client_id: String, tenant_id: String) -> Result<MicrosoftConfig> {
    let client_id = client_id.trim();
    let tenant_id = tenant_id.trim();
    let client_id = uuid::Uuid::parse_str(client_id).map_err(|_| {
        AppError::Message(
            "Application (client) ID must be a valid GUID from Microsoft Entra.".into(),
        )
    })?;
    let tenant_id = uuid::Uuid::parse_str(tenant_id).map_err(|_| {
        AppError::Message("Directory (tenant) ID must be a valid GUID from Microsoft Entra.".into())
    })?;
    if client_id.is_nil() || tenant_id.is_nil() {
        return Err(AppError::Message(
            "Replace the all-zero placeholders with the real Application ID and Tenant ID from Microsoft Entra.".into(),
        ));
    }
    Ok(MicrosoftConfig {
        client_id: client_id.to_string(),
        tenant_id: tenant_id.to_string(),
    })
}

#[tauri::command]
async fn save_microsoft_config(
    state: tauri::State<'_, AppState>,
    client_id: String,
    tenant_id: String,
) -> Result<AppStatus> {
    let config = normalize_microsoft_config(client_id, tenant_id)?;
    let changed = state.microsoft_config()? != config;
    if changed {
        auth::clear_token(&state)?;
    }
    state.update_settings(|settings| {
        settings.source_mode = SourceMode::MicrosoftGraph;
        settings.microsoft_config = Some(config);
        if changed {
            settings.account = None;
        }
    })?;
    if changed {
        state
            .verified_sources
            .lock()
            .map_err(|_| AppError::Message("Provenance cache lock was poisoned".into()))?
            .clear();
    }
    build_status(&state).await
}

#[tauri::command]
fn connector_installer_status(
    state: tauri::State<'_, AppState>,
) -> Result<connector_installer::Snapshot> {
    state.connector_installer.snapshot()
}

#[tauri::command]
async fn connector_installer_action(
    state: tauri::State<'_, AppState>,
    action: connector_installer::Action,
) -> Result<connector_installer::Snapshot> {
    // Package generation and bounded inbox reads stay off the UI thread.
    let installer = state.connector_installer.clone();
    tauri::async_runtime::spawn_blocking(move || installer.action(action))
        .await
        .map_err(|_| AppError::Message("Atlas connector: worker_failed".into()))?
}

#[tauri::command]
fn connector_installer_open_portal(state: tauri::State<'_, AppState>) -> Result<()> {
    state.connector_installer.open_portal()
}

#[tauri::command]
fn connector_installer_show_package(state: tauri::State<'_, AppState>) -> Result<()> {
    state.connector_installer.show_package()
}

fn normalize_bridge_folder(value: String) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::Message(
            "Choose the locally synced Power Automate inbox folder.".into(),
        ));
    }
    let path = Path::new(value);
    if !path.is_absolute() || !path.is_dir() {
        return Err(AppError::Message(
            "The Power Automate inbox must be an existing local folder.".into(),
        ));
    }
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
async fn save_power_automate_folder(
    state: tauri::State<'_, AppState>,
    folder: String,
) -> Result<AppStatus> {
    let folder = normalize_bridge_folder(folder)?;
    auth::clear_token(&state)?;
    state.update_settings(|settings| {
        settings.source_mode = SourceMode::PowerAutomateFolder;
        settings.bridge_folder = Some(folder);
        settings.account = None;
        if settings
            .destination
            .as_ref()
            .is_some_and(|destination| destination.kind == TrackerDestinationKind::SharePoint)
        {
            settings.destination = None;
        }
    })?;
    state
        .verified_sources
        .lock()
        .map_err(|_| AppError::Message("Provenance cache lock was poisoned".into()))?
        .clear();
    diagnostics::info("settings", "Power Automate inbox mode enabled");
    build_status(&state).await
}

#[tauri::command]
fn sign_out(state: tauri::State<'_, AppState>) -> Result<()> {
    auth::clear_token(&state)?;
    state.update_settings(|settings| settings.account = None)?;
    state
        .verified_sources
        .lock()
        .map_err(|_| AppError::Message("Provenance cache lock was poisoned".into()))?
        .clear();
    Ok(())
}

#[tauri::command]
fn save_profile(state: tauri::State<'_, AppState>, mut profile: UserProfile) -> Result<()> {
    profile.login_id = profile.login_id.trim().to_string();
    profile.full_name = profile.full_name.trim().to_string();
    profile.area = profile.area.trim().to_string();
    profile.team_lead = profile.team_lead.trim().to_string();
    profile.circana_manager = profile.circana_manager.trim().to_string();
    if profile.login_id.is_empty() || profile.full_name.is_empty() {
        return Err(AppError::Message(
            "Login ID and full name are required.".into(),
        ));
    }
    if profile.area.is_empty() {
        profile.area = "Manufacturing".into();
    }
    state.update_settings(|settings| settings.profile = Some(profile))
}

fn normalize_local_destination(value: String, existing: bool) -> Result<TrackerDestination> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::Message("Choose an Excel workbook first.".into()));
    }
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(AppError::Message(
            "Choose an absolute path for the Excel workbook.".into(),
        ));
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if existing && !matches!(extension.as_str(), "xlsx" | "xlsm") {
        return Err(AppError::Message(
            "An existing tracker must be an .xlsx or .xlsm workbook.".into(),
        ));
    }
    if !existing && extension != "xlsx" {
        return Err(AppError::Message(
            "A new tracker must use the .xlsx extension.".into(),
        ));
    }
    if existing && !path.is_file() {
        return Err(AppError::Message(
            "The selected existing workbook no longer exists.".into(),
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        AppError::Message("The workbook destination has no parent folder.".into())
    })?;
    if !parent.is_dir() {
        return Err(AppError::Message(
            "The workbook destination folder does not exist.".into(),
        ));
    }
    Ok(TrackerDestination {
        kind: if existing {
            TrackerDestinationKind::LocalExisting
        } else {
            TrackerDestinationKind::LocalNew
        },
        value: value.to_string(),
    })
}

#[tauri::command]
async fn save_tracker_destination(
    state: tauri::State<'_, AppState>,
    destination: TrackerDestination,
    auto_sync: bool,
) -> Result<AppStatus> {
    let source_mode = state.read_settings()?.source_mode;
    let destination = match destination.kind {
        TrackerDestinationKind::LocalExisting => {
            normalize_local_destination(destination.value, true)?
        }
        TrackerDestinationKind::LocalNew => normalize_local_destination(destination.value, false)?,
        TrackerDestinationKind::SharePoint => {
            if source_mode == SourceMode::PowerAutomateFolder {
                return Err(AppError::Message(
                    "Direct SharePoint access is unavailable in Power Automate Inbox mode. Choose the locally synced copy of the workbook instead.".into(),
                ));
            }
            let value = sharepoint::normalize_url(&destination.value)?;
            let account = auth::sign_in(&state, true).await?;
            state.update_settings(|settings| settings.account = Some(account))?;
            let token = auth::access_token(&state).await?;
            sharepoint::validate(&state, &token, &value).await?;
            TrackerDestination {
                kind: TrackerDestinationKind::SharePoint,
                value,
            }
        }
    };
    diagnostics::info(
        "settings",
        match &destination.kind {
            TrackerDestinationKind::SharePoint => "Saved SharePoint tracker destination",
            _ => "Saved local tracker destination",
        },
    );
    state.update_settings(|settings| {
        settings.destination = Some(destination);
        settings.auto_sync = auto_sync;
    })?;
    build_status(&state).await
}

#[tauri::command]
fn log_frontend_error(context: String, message: String) {
    diagnostics::error(&format!("frontend/{context}"), &message);
}

#[tauri::command]
async fn extract_interactions(
    state: tauri::State<'_, AppState>,
    date: String,
    include_email: bool,
    include_teams: bool,
    timezone: String,
) -> Result<ExtractionResult> {
    let settings = state.read_settings()?;
    if settings.profile.is_none() {
        return Err(AppError::Message(
            "Complete your profile before extracting interactions.".into(),
        ));
    }
    let result = match settings.source_mode {
        SourceMode::MicrosoftGraph => {
            let token = auth::access_token(&state).await?;
            if include_teams {
                state.local_ai.ensure_ready(&state.http).await?;
            }
            graph::extract(
                &state,
                &token,
                &date,
                &timezone,
                include_email,
                include_teams,
                ollama::BUNDLED_MODEL,
            )
            .await?
        }
        SourceMode::PowerAutomateFolder => {
            let folder = settings.bridge_folder.ok_or_else(|| {
                AppError::Message("Configure the Power Automate inbox folder first.".into())
            })?;
            bridge::extract(
                &state,
                Path::new(&folder),
                &date,
                &timezone,
                include_email,
                include_teams,
            )
            .await?
        }
    };
    let mut cache = state
        .verified_sources
        .lock()
        .map_err(|_| AppError::Message("Provenance cache lock was poisoned".into()))?;
    cache.clear();
    for item in &result.interactions {
        if item.source_kind != SourceKind::Manual {
            cache.insert(item.source_id.clone(), item.clone());
        }
    }
    Ok(result)
}

fn validate_provenance(state: &AppState, interactions: &[Interaction]) -> Result<()> {
    let cache = state
        .verified_sources
        .lock()
        .map_err(|_| AppError::Message("Provenance cache lock was poisoned".into()))?;
    let mut ids = HashSet::new();
    for item in interactions.iter().filter(|v| v.selected) {
        if !ids.insert(item.source_id.as_str()) {
            return Err(AppError::Message(format!(
                "Duplicate source ID in export request: {}",
                item.source_id
            )));
        }
        if item.source_kind == SourceKind::Manual {
            continue;
        }
        let trusted = cache.get(&item.source_id).ok_or_else(|| {
            AppError::Message(format!(
                "Rejected '{}': it was not returned by the configured evidence source during this extraction.",
                item.evidence_label
            ))
        })?;
        if trusted.source_kind != item.source_kind {
            return Err(AppError::Message(
                "Rejected an interaction whose source kind does not match its verified evidence."
                    .into(),
            ));
        }
        if matches!(item.source_kind, SourceKind::Calendar | SourceKind::Email) {
            let immutable_changed = trusted.interaction_type != item.interaction_type
                || trusted.reception_date_time != item.reception_date_time
                || trusted.interaction_date_time != item.interaction_date_time
                || trusted.resolution_date_time != item.resolution_date_time
                || trusted.client_type != item.client_type
                || trusted.end_client != item.end_client
                || trusted.status != item.status
                || trusted.resolution_type != item.resolution_type;
            if immutable_changed {
                return Err(AppError::Message(format!("Rejected '{}': calendar/mail evidence fields were altered. Re-extract the day and try again.", item.evidence_label)));
            }
        }
    }
    Ok(())
}

#[tauri::command]
async fn export_configured_tracker(
    state: tauri::State<'_, AppState>,
    date: String,
    profile: UserProfile,
    interactions: Vec<Interaction>,
) -> Result<ExportResult> {
    chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| AppError::Message("Choose a valid export date.".into()))?;
    let settings = state.read_settings()?;
    let saved = settings
        .profile
        .ok_or_else(|| AppError::Message("Complete your profile before exporting.".into()))?;
    let destination = settings.destination.ok_or_else(|| {
        AppError::Message("Configure an Excel or SharePoint tracker before syncing.".into())
    })?;
    if saved != profile {
        return Err(AppError::Message(
            "Your profile changed before export. Refresh the workspace and try again.".into(),
        ));
    }
    validate_provenance(&state, &interactions)?;
    let result = match destination.kind {
        TrackerDestinationKind::LocalExisting | TrackerDestinationKind::LocalNew => {
            let existing = destination.kind == TrackerDestinationKind::LocalExisting;
            let path = destination.value.clone();
            let result = tauri::async_runtime::spawn_blocking(move || {
                excel::export(&path, existing, &saved, &interactions)
            })
            .await
            .map_err(|e| AppError::Message(format!("Excel export task failed: {e}")))??;
            if !existing {
                state.update_settings(|settings| {
                    if let Some(saved_destination) = settings.destination.as_mut() {
                        saved_destination.kind = TrackerDestinationKind::LocalExisting;
                    }
                })?;
            }
            result
        }
        TrackerDestinationKind::SharePoint => {
            if settings.source_mode == SourceMode::PowerAutomateFolder {
                return Err(AppError::Message(
                    "Direct SharePoint export is unavailable in Power Automate Inbox mode.".into(),
                ));
            }
            let token = auth::access_token(&state).await?;
            sharepoint::export(&state, &token, &destination.value, &saved, &interactions).await?
        }
    };
    diagnostics::info(
        "export",
        &format!(
            "Tracker sync completed: {} inserted, {} updated, {} skipped",
            result.inserted, result.updated, result.skipped
        ),
    );
    Ok(result)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_dir = app
                .path()
                .app_config_dir()
                .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
            diagnostics::init(&config_dir).map_err(|e| Box::<dyn std::error::Error>::from(e))?;
            diagnostics::install_panic_hook();
            app.manage(AppState::new(config_dir)?);
            diagnostics::info("startup", "Atlas application state loaded");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_status,
            connector_installer_status,
            connector_installer_action,
            connector_installer_open_portal,
            connector_installer_show_package,
            save_microsoft_config,
            save_power_automate_folder,
            sign_in,
            sign_out,
            save_profile,
            save_tracker_destination,
            log_frontend_error,
            extract_interactions,
            export_configured_tracker
        ])
        .run(tauri::generate_context!())
        .expect("error while running Atlas");
}

#[cfg(test)]
mod configuration_tests {
    use super::*;

    #[test]
    fn normalizes_valid_microsoft_identifiers() {
        let config = normalize_microsoft_config(
            " 11111111-1111-4111-8111-111111111111 ".into(),
            "22222222-2222-4222-8222-222222222222".into(),
        )
        .unwrap();
        assert_eq!(config.client_id, "11111111-1111-4111-8111-111111111111");
        assert_eq!(config.tenant_id, "22222222-2222-4222-8222-222222222222");
    }

    #[test]
    fn rejects_invalid_microsoft_identifiers() {
        assert!(normalize_microsoft_config("not-an-id".into(), "also-not-an-id".into()).is_err());
        assert!(normalize_microsoft_config(
            "00000000-0000-0000-0000-000000000000".into(),
            "00000000-0000-0000-0000-000000000000".into()
        )
        .is_err());
    }
}
