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
        AppStatus, ExportResult, ExtractionResult, Interaction, SourceKind, SourceMode,
        TrackerDestination, TrackerDestinationKind, UserProfile,
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
    let connector_ready = state.connector_installer.snapshot()?.session.phase
        == connector_installer::Phase::Completed;
    let configured = match settings.source_mode {
        SourceMode::MicrosoftGraph => {
            !microsoft_config.client_id.is_empty() && !microsoft_config.tenant_id.is_empty()
        }
        SourceMode::PowerAutomateFolder => {
            settings
                .bridge_folder
                .as_deref()
                .is_some_and(|folder| Path::new(folder).is_dir())
                && connector_ready
        }
    };
    Ok(AppStatus {
        configured,
        signed_in: match settings.source_mode {
            SourceMode::MicrosoftGraph => token_available && settings.account.is_some(),
            SourceMode::PowerAutomateFolder => configured,
        },
        source_mode: settings.source_mode,
        bridge_folder: settings.bridge_folder,
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

#[tauri::command]
fn connector_installer_open_inbox(state: tauri::State<'_, AppState>) -> Result<()> {
    state.connector_installer.open_inbox()
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
    let default_tracker = Path::new(&folder)
        .parent()
        .and_then(Path::parent)
        .map(|root| root.join("Tracker_Circana.xlsx"));
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
        if settings.destination.is_none() {
            if let Some(path) = default_tracker {
                settings.destination = Some(TrackerDestination {
                    kind: if path.is_file() {
                        TrackerDestinationKind::LocalExisting
                    } else {
                        TrackerDestinationKind::LocalNew
                    },
                    value: path.to_string_lossy().into_owned(),
                });
            }
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
    let mut result = match settings.source_mode {
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
    select_daily_tracker_rows(&mut result.interactions);
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

fn select_daily_tracker_rows(interactions: &mut [Interaction]) {
    for item in interactions.iter_mut() {
        item.selected = false;
    }
    for interaction_type in ["Meeting", "E-Mail", "Task"] {
        let preferred = interactions.iter().position(|item| {
            item.interaction_type == interaction_type
                && match interaction_type {
                    "Meeting" => item.status == "Resolved" && item.resolution_date_time.is_some(),
                    "Task" => item.ai_suggested,
                    _ => true,
                }
        });
        let fallback = (interaction_type == "Task")
            .then(|| {
                interactions
                    .iter()
                    .position(|item| item.interaction_type == interaction_type)
            })
            .flatten();
        if let Some(index) = preferred.or(fallback) {
            interactions[index].selected = true;
            interactions[index].reviewed = true;
        }
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn interaction(
        source_kind: SourceKind,
        source_id: &str,
        interaction_type: &str,
    ) -> Interaction {
        Interaction {
            source_kind,
            source_id: source_id.into(),
            interaction_type: interaction_type.into(),
            reception_date_time: "2026-09-14T14:00:00Z".into(),
            interaction_date_time: "2026-09-14T14:00:00Z".into(),
            resolution_date_time: Some("2026-09-14T14:30:00Z".into()),
            client_type: "Circana".into(),
            end_client: String::new(),
            status: "Resolved".into(),
            resolution_type: "Processed & Resolved".into(),
            category: String::new(),
            subcategory: String::new(),
            priority: "Intermediate".into(),
            incident_number: String::new(),
            comments: "Verified interaction".into(),
            selected: true,
            reviewed: false,
            manual_authored: false,
            ai_suggested: false,
            evidence_label: "Verified interaction".into(),
        }
    }

    #[test]
    fn preselects_exactly_one_row_for_each_required_category() {
        let mut rows = vec![
            interaction(SourceKind::Calendar, "calendar-1", "Meeting"),
            interaction(SourceKind::Calendar, "calendar-2", "Meeting"),
            interaction(SourceKind::Email, "mail-1", "E-Mail"),
            interaction(SourceKind::TeamsChat, "teams-direct", "Task"),
            {
                let mut row = interaction(SourceKind::TeamsChat, "teams-ai", "Task");
                row.ai_suggested = true;
                row
            },
        ];

        select_daily_tracker_rows(&mut rows);

        let selected = rows.iter().filter(|row| row.selected).collect::<Vec<_>>();
        assert_eq!(selected.len(), 3);
        assert!(selected.iter().all(|row| row.reviewed));
        assert!(selected.iter().any(|row| row.source_id == "calendar-1"));
        assert!(selected.iter().any(|row| row.source_id == "mail-1"));
        assert!(selected.iter().any(|row| row.source_id == "teams-ai"));
    }

    #[test]
    fn does_not_log_a_meeting_before_it_has_finished() {
        let mut meeting = interaction(SourceKind::Calendar, "calendar-future", "Meeting");
        meeting.status = "In Progress".into();
        meeting.resolution_date_time = None;
        let mut rows = vec![meeting];

        select_daily_tracker_rows(&mut rows);

        assert!(!rows[0].selected);
    }
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
        if matches!(item.source_kind, SourceKind::Calendar | SourceKind::Email)
            || (item.source_kind == SourceKind::TeamsChat && !trusted.ai_suggested)
        {
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
    let selected: Vec<_> = interactions.iter().filter(|item| item.selected).collect();
    let has_meeting = selected
        .iter()
        .any(|item| item.interaction_type == "Meeting");
    let has_mail = selected
        .iter()
        .any(|item| item.interaction_type == "E-Mail");
    let has_task = selected.iter().any(|item| item.interaction_type == "Task");
    if selected.len() < 3 || !has_meeting || !has_mail || !has_task {
        return Err(AppError::Message(
            "Antes de guardar, registra al menos tres interacciones reales: una reunión/calendario, un correo y una tarea/Teams. Añade manualmente la categoría que falte."
                .into(),
        ));
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
    let app = tauri::Builder::default()
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
            connector_installer_open_inbox,
            save_power_automate_folder,
            sign_in,
            sign_out,
            save_profile,
            save_tracker_destination,
            log_frontend_error,
            extract_interactions,
            export_configured_tracker
        ])
        .build(tauri::generate_context!())
        .expect("error while building Atlas");
    app.run(|app_handle, event| {
        let closing = matches!(
            event,
            tauri::RunEvent::ExitRequested { .. }
                | tauri::RunEvent::Exit
                | tauri::RunEvent::WindowEvent {
                    event: tauri::WindowEvent::CloseRequested { .. },
                    ..
                }
        );
        if closing {
            if let Some(state) = app_handle.try_state::<AppState>() {
                state.local_ai.stop();
            }
        }
    });
}
