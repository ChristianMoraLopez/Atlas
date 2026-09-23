mod auth;
mod automation;
mod bridge;
mod connector_installer;
mod diagnostics;
mod error;
mod evidence_validation;
mod excel;
mod graph;
mod models;
mod ollama;
mod qa;
mod qa_engine;
mod sharepoint;
mod state;
mod tracker_writer;

use crate::{
    error::{AppError, Context, Result},
    models::{
        AiInstructions, AppRole, AppStatus, AutomationMode, ExportResult, ExtractionResult,
        Interaction, QaCase, QaConfig, QaExportResult, SourceKind, SourceMode, TrackerDestination,
        TrackerDestinationKind, UserProfile,
    },
    state::AppState,
};
use std::{collections::HashSet, path::Path, sync::atomic::Ordering};
use tauri::{Emitter, Manager};

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CachedDayPreview {
    source_mode: SourceMode,
    result: ExtractionResult,
    #[serde(default)]
    verified_sources: Vec<Interaction>,
}

fn day_preview_path(state: &AppState, date: &str) -> std::path::PathBuf {
    state
        .config_dir
        .join("days")
        .join(date)
        .join("preview.json")
}

fn cache_day_preview(
    state: &AppState,
    date: &str,
    source_mode: SourceMode,
    result: &ExtractionResult,
) -> Result<()> {
    let path = day_preview_path(state, date);
    let verified_sources = result
        .interactions
        .iter()
        .filter(|item| item.source_kind != SourceKind::Manual)
        .cloned()
        .collect();
    write_day_preview(
        &path,
        &CachedDayPreview {
            source_mode,
            result: result.clone(),
            verified_sources,
        },
    )
}

fn write_day_preview(path: &Path, cached: &CachedDayPreview) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Message("Atlas could not prepare the day cache path".into()))?;
    std::fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, serde_json::to_vec_pretty(cached)?)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

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
    let manager = settings.app_role == Some(AppRole::Manager);
    // Each role has its own personalized Power Automate solution; the role
    // decides which installation must be complete. A manager already signed
    // in with Microsoft Graph keeps using Graph.
    let bridge_mode = if manager {
        !(settings.source_mode == SourceMode::MicrosoftGraph && settings.account.is_some())
    } else {
        settings.source_mode == SourceMode::PowerAutomateFolder
    };
    let (installer, folder) = if manager {
        (&state.qa_connector_installer, settings.qa_bridge_folder.as_deref())
    } else {
        (&state.connector_installer, settings.bridge_folder.as_deref())
    };
    let installer_session = installer.snapshot()?.session;
    let connector_ready = installer_session.phase == connector_installer::Phase::Completed;
    let configured = if bridge_mode {
        folder.is_some_and(|folder| Path::new(folder).is_dir()) && connector_ready
    } else {
        !microsoft_config.client_id.is_empty() && !microsoft_config.tenant_id.is_empty()
    };
    Ok(AppStatus {
        configured,
        signed_in: if bridge_mode {
            configured
        } else {
            token_available && settings.account.is_some()
        },
        source_mode: if bridge_mode {
            SourceMode::PowerAutomateFolder
        } else {
            settings.source_mode
        },
        bridge_folder: settings.bridge_folder,
        account: settings.account,
        profile: settings.profile,
        ollama_running,
        ollama_model_available,
        ollama_model: ollama::BUNDLED_MODEL.into(),
        local_ai_error,
        destination: settings.destination,
        auto_sync: settings.auto_sync,
        auto_sync_time: settings.auto_sync_time,
        automation_mode: settings.automation_mode,
        language: settings.language,
        ai_instructions: settings.ai_instructions,
        scheduled_launch: state.background_launch,
        log_path: diagnostics::path(),
        app_role: settings.app_role,
        qa_bridge_folder: settings.qa_bridge_folder,
        solution_owner: installer_session.owner_name,
    })
}

/// Atlas keeps running in the tray when an automatic job depends on it.
fn resident(state: &AppState) -> bool {
    state.read_settings().is_ok_and(|settings| match settings.app_role {
        Some(AppRole::Cs) => {
            settings.auto_sync && settings.destination.is_some() && settings.profile.is_some()
        }
        Some(AppRole::Manager) => true,
        None => false,
    })
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.foreground_requested.store(true, Ordering::SeqCst);
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
async fn save_app_role(
    state: tauri::State<'_, AppState>,
    role: Option<String>,
) -> Result<AppStatus> {
    let parsed = match role.as_deref() {
        None => None,
        Some("cs") => Some(AppRole::Cs),
        Some("manager") => Some(AppRole::Manager),
        Some(other) => {
            return Err(AppError::Message(format!("Unknown Atlas role: {other}")));
        }
    };
    state.update_settings(|settings| {
        settings.app_role = parsed;
    })?;
    build_status(&state).await
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

fn installer_for(
    state: &AppState,
    role: Option<&str>,
) -> Result<std::sync::Arc<connector_installer::Installer>> {
    match role.unwrap_or("cs") {
        "cs" | "tracker" => Ok(state.connector_installer.clone()),
        "manager" | "qa" => Ok(state.qa_connector_installer.clone()),
        other => Err(AppError::Message(format!("Unknown Atlas role: {other}"))),
    }
}

#[tauri::command]
fn connector_installer_status(
    state: tauri::State<'_, AppState>,
    role: Option<String>,
) -> Result<connector_installer::Snapshot> {
    installer_for(&state, role.as_deref())?.snapshot()
}

#[tauri::command]
async fn connector_installer_action(
    state: tauri::State<'_, AppState>,
    role: Option<String>,
    action: connector_installer::Action,
) -> Result<connector_installer::Snapshot> {
    // Package generation and bounded inbox reads stay off the UI thread.
    let installer = installer_for(&state, role.as_deref())?;
    tauri::async_runtime::spawn_blocking(move || installer.action(action))
        .await
        .map_err(|_| AppError::Message("Atlas connector: worker_failed".into()))?
}

#[tauri::command]
fn connector_installer_open_portal(
    state: tauri::State<'_, AppState>,
    role: Option<String>,
) -> Result<()> {
    installer_for(&state, role.as_deref())?.open_portal()
}

#[tauri::command]
fn connector_installer_show_package(
    state: tauri::State<'_, AppState>,
    role: Option<String>,
) -> Result<()> {
    installer_for(&state, role.as_deref())?.show_package()
}

#[tauri::command]
fn connector_installer_open_inbox(
    state: tauri::State<'_, AppState>,
    role: Option<String>,
) -> Result<()> {
    installer_for(&state, role.as_deref())?.open_inbox()
}

#[tauri::command]
async fn save_qa_bridge_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    folder: String,
) -> Result<AppStatus> {
    let folder = normalize_bridge_folder(folder)?;
    state.update_settings(|settings| {
        settings.source_mode = SourceMode::PowerAutomateFolder;
        settings.qa_bridge_folder = Some(folder);
    })?;
    diagnostics::info("settings", "Power Automate QA connector enabled");
    if let Err(error) = automation::configure(true, automation::DEFAULT_DAILY_TIME) {
        diagnostics::error("automation/setup", &error.to_string());
    }
    if let Err(error) = qa_engine::sync_watch_list(&state) {
        diagnostics::error("qa/watch", &error.to_string());
    }
    tauri::async_runtime::spawn(qa_engine::tick(app));
    build_status(&state).await
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
        if settings.destination.as_ref().is_some_and(|destination| {
            matches!(
                destination.kind,
                TrackerDestinationKind::SharePoint | TrackerDestinationKind::SharePointFlow
            )
        }) {
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

#[tauri::command]
async fn save_ai_instructions(
    state: tauri::State<'_, AppState>,
    presets: Vec<String>,
    custom: String,
) -> Result<AppStatus> {
    let mut known: Vec<String> = Vec::new();
    for preset in &presets {
        let id = preset.trim();
        if ollama::AI_PRESETS.iter().any(|entry| entry.id == id)
            && !known.iter().any(|saved| saved == id)
        {
            known.push(id.to_string());
        }
    }
    let custom = ollama::sanitize_custom_instructions(&custom);
    state.update_settings(|settings| {
        settings.ai_instructions = AiInstructions {
            presets: known,
            custom,
        };
    })?;
    diagnostics::info("settings", "Saved AI user instructions");
    build_status(&state).await
}

#[tauri::command]
async fn save_language(state: tauri::State<'_, AppState>, language: String) -> Result<AppStatus> {
    let language = language.trim().to_ascii_lowercase();
    if language != "es" && language != "en" {
        return Err(AppError::Message("Unsupported interface language.".into()));
    }
    state.update_settings(|settings| settings.language = language)?;
    build_status(&state).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AiPromptInfo {
    core_template: String,
    presets: Vec<AiPresetInfo>,
    instructions: AiInstructions,
    active_rules: Vec<String>,
    last_submission: Option<ollama::AiSubmission>,
}

#[derive(serde::Serialize)]
struct AiPresetInfo {
    id: &'static str,
    rule: &'static str,
}

#[tauri::command]
fn get_ai_prompt_info(state: tauri::State<'_, AppState>) -> Result<AiPromptInfo> {
    let settings = state.read_settings()?;
    let last_submission = state
        .last_ai_submission
        .lock()
        .map_err(|_| AppError::Message("AI submission lock was poisoned".into()))?
        .clone();
    Ok(AiPromptInfo {
        core_template: ollama::core_prompt_template().into(),
        presets: ollama::AI_PRESETS
            .iter()
            .map(|preset| AiPresetInfo {
                id: preset.id,
                rule: preset.rule,
            })
            .collect(),
        active_rules: ollama::active_user_rules(&settings.ai_instructions),
        instructions: settings.ai_instructions,
        last_submission,
    })
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
        local_path: None,
        writer_package_path: None,
    })
}

#[tauri::command]
async fn save_tracker_destination(
    state: tauri::State<'_, AppState>,
    destination: TrackerDestination,
    auto_sync: bool,
    auto_sync_time: String,
    automation_mode: AutomationMode,
) -> Result<AppStatus> {
    let current = state.read_settings()?;
    let source_mode = current.source_mode;
    let auto_sync_time = automation::normalize_time(&auto_sync_time)?;
    let destination = match destination.kind {
        TrackerDestinationKind::LocalExisting => {
            normalize_local_destination(destination.value, true)?
        }
        TrackerDestinationKind::LocalNew => normalize_local_destination(destination.value, false)?,
        TrackerDestinationKind::SharePoint => {
            let value = sharepoint::normalize_url(&destination.value)?;
            let local_path = if source_mode == SourceMode::PowerAutomateFolder {
                Some(sharepoint::resolve_synced_copy(
                    &value,
                    destination.local_path.as_deref(),
                    current.bridge_folder.as_deref(),
                )?)
            } else {
                let account = auth::sign_in(&state, true).await?;
                state.update_settings(|settings| settings.account = Some(account))?;
                let token = auth::access_token(&state).await?;
                sharepoint::validate(&state, &token, &value).await?;
                None
            };
            TrackerDestination {
                kind: TrackerDestinationKind::SharePoint,
                value,
                local_path,
                writer_package_path: None,
            }
        }
        TrackerDestinationKind::SharePointFlow => {
            if source_mode != SourceMode::PowerAutomateFolder {
                return Err(AppError::Message(
                    "The Power Automate writer is available with the Power Automate inbox source."
                        .into(),
                ));
            }
            let value = sharepoint::normalize_url(&destination.value)?;
            let _ = sharepoint::workbook_name(&value)?;
            let bridge_folder = current.bridge_folder.as_deref().ok_or_else(|| {
                AppError::Message("Configure the Power Automate inbox folder first.".into())
            })?;
            let tracker_session = state.connector_installer.snapshot()?.session;
            let prepared = tracker_writer::prepare(
                &state.config_dir,
                bridge_folder,
                &value,
                &tracker_session.installation_id,
                tracker_session.owner_name.as_deref(),
            )?;
            TrackerDestination {
                kind: TrackerDestinationKind::SharePointFlow,
                value,
                local_path: None,
                writer_package_path: Some(prepared.package_path),
            }
        }
    };
    automation::configure(auto_sync, &auto_sync_time)?;
    diagnostics::info(
        "settings",
        match &destination.kind {
            TrackerDestinationKind::SharePoint | TrackerDestinationKind::SharePointFlow => {
                "Saved SharePoint tracker destination"
            }
            _ => "Saved local tracker destination",
        },
    );
    state.update_settings(|settings| {
        settings.destination = Some(destination);
        settings.auto_sync = auto_sync;
        settings.auto_sync_time = auto_sync_time;
        settings.automation_mode = automation_mode;
    })?;
    build_status(&state).await
}

#[tauri::command]
fn log_frontend_error(context: String, message: String) {
    diagnostics::error(&format!("frontend/{context}"), &message);
}

#[tauri::command]
fn background_frontend_ready(state: tauri::State<'_, AppState>) {
    state
        .frontend_ready
        .store(true, std::sync::atomic::Ordering::SeqCst);
    diagnostics::info(
        "automation/frontend",
        "The background event listener is ready",
    );
}

#[tauri::command]
fn complete_scheduled_launch(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    run_key: String,
    successful: bool,
    needs_attention: bool,
) -> Result<()> {
    if successful {
        automation::complete(&state.config_dir, &run_key)?;
    }
    // Always release the run so a deferred or failed attempt can be retried
    // by the scheduler after its cooldown.
    state.automation_pending.store(false, Ordering::SeqCst);
    let foreground = state.foreground_requested.load(Ordering::SeqCst);
    if run_key.trim().is_empty() {
        // Startup acknowledgement without a run: surface incomplete setup.
        if needs_attention && state.background_launch && !foreground {
            show_main_window(&app);
        }
        return Ok(());
    }
    let hidden_run = state.hidden_run.swap(false, Ordering::SeqCst);
    if !hidden_run || foreground {
        return Ok(());
    }
    if needs_attention || !successful {
        show_main_window(&app);
    } else {
        // Stay resident in the tray for the next run; free the AI memory.
        state.local_ai.stop();
    }
    Ok(())
}

#[tauri::command]
fn open_log_file() -> Result<()> {
    let path = diagnostics::path();
    if !Path::new(&path).is_file() {
        return Err(AppError::Message("The Atlas log has not been created yet.".into()));
    }
    open::that_detached(&path)
        .map_err(|error| AppError::Message(format!("Windows could not open the log: {error}")))
}

#[tauri::command]
fn open_external_url(url: String) -> Result<()> {
    let parsed = url::Url::parse(url.trim())
        .map_err(|_| AppError::Message("Atlas can only open valid web links.".into()))?;
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AppError::Message("Atlas only opens secure https links.".into()));
    }
    open::that_detached(parsed.as_str())
        .map_err(|error| AppError::Message(format!("Windows could not open the link: {error}")))
}

#[tauri::command]
fn reveal_tracker_folder(state: tauri::State<'_, AppState>) -> Result<()> {
    let destination = state
        .read_settings()?
        .destination
        .ok_or_else(|| AppError::Message("Configure the tracker destination first.".into()))?;
    let path = destination
        .local_path
        .unwrap_or(destination.value);
    let path = Path::new(path.strip_prefix(r"\\?\").unwrap_or(&path)).to_path_buf();
    let folder = path
        .parent()
        .filter(|folder| folder.is_dir())
        .ok_or_else(|| AppError::Message("The tracker folder is not available.".into()))?;
    open::that_detached(folder)
        .map_err(|error| AppError::Message(format!("Windows could not open the folder: {error}")))
}

#[tauri::command]
fn show_tracker_writer_package(state: tauri::State<'_, AppState>) -> Result<()> {
    let destination = state
        .read_settings()?
        .destination
        .filter(|destination| destination.kind == TrackerDestinationKind::SharePointFlow)
        .ok_or_else(|| AppError::Message("Configure the SharePoint cloud writer first.".into()))?;
    let package = destination
        .writer_package_path
        .ok_or_else(|| AppError::Message("The tracker writer ZIP has not been prepared.".into()))?;
    let path = Path::new(&package);
    if !path.is_file() {
        return Err(AppError::Message(
            "The tracker writer ZIP is missing. Save the tracker setup again to recreate it."
                .into(),
        ));
    }
    open::that(path.parent().unwrap_or(path)).map_err(|error| {
        AppError::Message(format!(
            "Windows could not show the tracker writer ZIP: {error}"
        ))
    })
}

#[tauri::command]
fn open_power_automate_portal() -> Result<()> {
    open::that("https://make.powerautomate.com/").map_err(|error| {
        AppError::Message(format!("Windows could not open Power Automate: {error}"))
    })
}

#[tauri::command]
fn request_bridge_date(state: tauri::State<'_, AppState>, date: String) -> Result<()> {
    let settings = state.read_settings()?;
    if settings.source_mode != SourceMode::PowerAutomateFolder {
        return Err(AppError::Message(
            "A Power Automate date request is available only in inbox mode.".into(),
        ));
    }
    let folder = settings.bridge_folder.ok_or_else(|| {
        AppError::Message("Configure the Power Automate inbox folder first.".into())
    })?;
    bridge::request_date(Path::new(&folder), &date)
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
    let source_mode = settings.source_mode;
    if settings.profile.is_none() {
        return Err(AppError::Message(
            "Complete your profile before extracting interactions.".into(),
        ));
    }
    let mut result = match settings.source_mode {
        SourceMode::MicrosoftGraph => {
            let token = auth::access_token(&state).await?;
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
    drop(cache);
    cache_day_preview(&state, &date, source_mode, &result)?;
    Ok(result)
}

#[tauri::command]
fn load_cached_day(
    state: tauri::State<'_, AppState>,
    date: String,
) -> Result<Option<ExtractionResult>> {
    chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| AppError::Message("Choose a valid cached workday.".into()))?;
    let path = day_preview_path(&state, &date);
    if !path.is_file() {
        return Ok(None);
    }
    let cached: CachedDayPreview = match std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    {
        Some(value) => value,
        None => {
            diagnostics::error(
                "cache/day",
                &format!("Ignored an invalid saved preview at {}", path.display()),
            );
            return Ok(None);
        }
    };
    if cached.source_mode != state.read_settings()?.source_mode {
        return Ok(None);
    }
    let mut verified = state
        .verified_sources
        .lock()
        .map_err(|_| AppError::Message("Provenance cache lock was poisoned".into()))?;
    verified.clear();
    let trusted = if cached.verified_sources.is_empty() {
        &cached.result.interactions
    } else {
        &cached.verified_sources
    };
    for item in trusted
        .iter()
        .filter(|item| item.source_kind != SourceKind::Manual)
    {
        verified.insert(item.source_id.clone(), item.clone());
    }
    diagnostics::info(
        "cache/day",
        &format!("Restored the saved Atlas preview for {date}"),
    );
    Ok(Some(cached.result))
}

#[tauri::command]
fn save_day_preview(
    state: tauri::State<'_, AppState>,
    date: String,
    interactions: Vec<Interaction>,
    warnings: Vec<String>,
) -> Result<()> {
    chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| AppError::Message("Choose a valid workday to save the preview.".into()))?;
    let path = day_preview_path(&state, &date);
    let current_source_mode = state.read_settings()?.source_mode;
    let mut cached: CachedDayPreview = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| CachedDayPreview {
            source_mode: current_source_mode,
            result: ExtractionResult {
                interactions: Vec::new(),
                warnings: Vec::new(),
            },
            verified_sources: Vec::new(),
        });
    if cached.source_mode != current_source_mode {
        return Err(AppError::Message(
            "The saved preview belongs to a different evidence source.".into(),
        ));
    }
    for item in interactions
        .iter()
        .filter(|item| item.source_kind != SourceKind::Manual)
    {
        let trusted = cached
            .verified_sources
            .iter()
            .find(|value| value.source_id == item.source_id)
            .ok_or_else(|| {
                AppError::Message("Atlas refused to cache an unverified interaction.".into())
            })?;
        if trusted.source_kind != item.source_kind {
            return Err(AppError::Message(
                "Atlas refused to cache an interaction with altered provenance.".into(),
            ));
        }
    }
    cached.result = ExtractionResult {
        interactions,
        warnings,
    };
    write_day_preview(&path, &cached)
}

fn select_daily_tracker_rows(interactions: &mut [Interaction]) {
    for item in interactions.iter_mut() {
        item.selected = (item.interaction_type == "Meeting"
            && item.status == "Resolved"
            && item.resolution_date_time.is_some())
            || (item.interaction_type == "Task" && item.ai_suggested);
        if item.selected {
            item.reviewed = true;
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
    fn preselects_completed_meetings_and_all_inferred_work_tasks() {
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
        assert!(selected.iter().any(|row| row.source_id == "calendar-2"));
        assert!(!selected.iter().any(|row| row.source_id == "mail-1"));
        assert!(!selected.iter().any(|row| row.source_id == "teams-direct"));
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
                let path = destination.local_path.clone().ok_or_else(|| {
                    AppError::Message(
                        "Choose the locally synced copy of the SharePoint tracker first.".into(),
                    )
                })?;
                let mut result = tauri::async_runtime::spawn_blocking(move || {
                    excel::export(&path, true, &saved, &interactions)
                })
                .await
                .map_err(|e| AppError::Message(format!("Excel export task failed: {e}")))??;
                result.path = destination.value.clone();
                result
            } else {
                let token = auth::access_token(&state).await?;
                sharepoint::export(&state, &token, &destination.value, &saved, &interactions)
                    .await?
            }
        }
        TrackerDestinationKind::SharePointFlow => {
            let bridge_folder = settings.bridge_folder.as_deref().ok_or_else(|| {
                AppError::Message("Configure the Power Automate inbox folder first.".into())
            })?;
            tracker_writer::queue(
                bridge_folder,
                &destination.value,
                &date,
                &saved,
                &interactions,
            )?
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

// ---------------------------------------------------------------------------
// QA Audit commands (additive module; does not touch the tracker pipeline)
// ---------------------------------------------------------------------------

#[tauri::command]
fn qa_get_config(state: tauri::State<'_, AppState>) -> Result<QaConfig> {
    Ok(state.read_settings()?.qa)
}

#[tauri::command]
fn qa_save_config(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    mut config: QaConfig,
) -> Result<()> {
    let mut seen = HashSet::new();
    for auditee in &mut config.auditees {
        auditee.name = auditee.name.trim().to_string();
        auditee.email = auditee.email.trim().to_lowercase();
        if auditee.name.is_empty() || qa_engine::addresses(&auditee.email) != [auditee.email.clone()]
        {
            return Err(AppError::Message(
                "Each person on the QA list needs a name and a valid email.".into(),
            ));
        }
        if !seen.insert(auditee.email.clone()) {
            return Err(AppError::Message(format!(
                "{} appears twice on the QA list.",
                auditee.email
            )));
        }
    }
    for time in [&mut config.check_morning, &mut config.check_afternoon] {
        *time = automation::normalize_time(time)?;
    }
    config.history_months = config.history_months.clamp(1, 120);
    config.lookback_days = config.lookback_days.clamp(1, 90);
    config.subject_keywords = config
        .subject_keywords
        .iter()
        .map(|keyword| keyword.trim().to_string())
        .filter(|keyword| !keyword.is_empty())
        .collect();
    if let Some(folder) = config.output_folder.as_deref() {
        if !folder.trim().is_empty() {
            std::fs::create_dir_all(folder)
                .context("Unable to create the QA output folder")?;
        }
    }
    state.update_settings(|settings| {
        settings.qa = config;
    })?;
    if let Err(error) = qa_engine::sync_watch_list(&state) {
        diagnostics::error("qa/watch", &error.to_string());
    }
    tauri::async_runtime::spawn(qa_engine::tick(app));
    Ok(())
}

#[tauri::command]
fn qa_engine_status(state: tauri::State<'_, AppState>) -> Result<qa_engine::QaEngineStatus> {
    qa_engine::status(&state)
}

#[tauri::command]
fn qa_list_cases(state: tauri::State<'_, AppState>) -> Result<Vec<qa_engine::QaCaseEntry>> {
    qa_engine::list_cases(&state)
}

#[tauri::command]
fn qa_update_case(state: tauri::State<'_, AppState>, case: QaCase) -> Result<()> {
    qa_engine::update_case(&state, case)
}

#[tauri::command]
fn qa_reevaluate_case(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    case_id: String,
) -> Result<()> {
    qa_engine::reevaluate_case(&state, &case_id)?;
    tauri::async_runtime::spawn(qa_engine::tick(app));
    Ok(())
}

#[tauri::command]
fn qa_sync_now(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<()> {
    qa_engine::request_sync(&state)?;
    tauri::async_runtime::spawn(qa_engine::tick(app));
    Ok(())
}

#[tauri::command]
fn qa_restart_history(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<()> {
    qa_engine::restart_historical(&state)?;
    tauri::async_runtime::spawn(qa_engine::tick(app));
    Ok(())
}

#[tauri::command]
async fn qa_export_now(
    state: tauri::State<'_, AppState>,
    full: bool,
) -> Result<QaExportResult> {
    qa_engine::export_now(&state, full)
}

#[tauri::command]
fn qa_open_output_folder(state: tauri::State<'_, AppState>) -> Result<()> {
    let config = state.read_settings()?.qa;
    let folder = qa::output_folder(&config)
        .ok_or_else(|| AppError::Message("Configure the QA output folder first.".into()))?;
    if !folder.is_dir() {
        return Err(AppError::Message(
            "The configured QA output folder no longer exists.".into(),
        ));
    }
    open::that_detached(&folder)
        .map_err(|error| AppError::Message(format!("Windows could not open the folder: {error}")))
}

#[tauri::command]
fn open_tracker_destination(state: tauri::State<'_, AppState>) -> Result<()> {
    let destination = state
        .read_settings()?
        .destination
        .ok_or_else(|| AppError::Message("Configure the tracker destination first.".into()))?;
    let target = if matches!(
        destination.kind,
        TrackerDestinationKind::SharePoint | TrackerDestinationKind::SharePointFlow
    ) {
        destination.value
    } else {
        let value = destination
            .value
            .strip_prefix(r"\\?\")
            .unwrap_or(&destination.value)
            .to_string();
        if !Path::new(&value).is_file() {
            return Err(AppError::Message(
                "The configured tracker file no longer exists.".into(),
            ));
        }
        value
    };
    open::that_detached(&target)
        .map_err(|error| AppError::Message(format!("Windows could not open the tracker: {error}")))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let scheduled_launch = std::env::args().any(|arg| arg == "--atlas-daily-run");
    let startup_launch = std::env::args().any(|arg| arg == "--atlas-startup");
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let scheduled = args.iter().any(|arg| arg == "--atlas-daily-run");
            let startup = args.iter().any(|arg| arg == "--atlas-startup");
            if scheduled {
                let _ = app.emit("atlas-daily-run", ());
            } else if !startup {
                if let Some(state) = app.try_state::<AppState>() {
                    state
                        .foreground_requested
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                }
                let Some(window) = app.get_webview_window("main") else {
                    return;
                };
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let config_dir = app
                .path()
                .app_config_dir()
                .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
            diagnostics::init(&config_dir).map_err(|e| Box::<dyn std::error::Error>::from(e))?;
            diagnostics::install_panic_hook();
            let background_launch = scheduled_launch || startup_launch;
            let state = AppState::new(config_dir.clone(), background_launch)?;
            if resident(&state) {
                let time = state
                    .read_settings()
                    .map(|settings| settings.auto_sync_time)
                    .unwrap_or_else(|_| automation::DEFAULT_DAILY_TIME.into());
                if let Err(error) = automation::configure(true, &time) {
                    diagnostics::error("automation/setup", &error.to_string());
                }
            }
            let spanish = state
                .read_settings()
                .map(|settings| settings.language != "en")
                .unwrap_or(true);
            app.manage(state);
            build_tray(app, spanish)?;
            automation::start_background_scheduler(app.handle().clone(), config_dir);
            if scheduled_launch || startup_launch {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            diagnostics::info(
                "startup",
                &format!(
                    "Atlas v{} application state loaded (launch_mode={}, background_launch={background_launch}, executable={})",
                    env!("CARGO_PKG_VERSION"),
                    if startup_launch { "windows_startup" } else if scheduled_launch { "legacy_daily" } else { "interactive" },
                    std::env::current_exe().map(|path| path.display().to_string()).unwrap_or_else(|_| "unknown".into())
                ),
            );
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
            save_ai_instructions,
            save_language,
            get_ai_prompt_info,
            save_tracker_destination,
            log_frontend_error,
            background_frontend_ready,
            open_tracker_destination,
            show_tracker_writer_package,
            open_power_automate_portal,
            complete_scheduled_launch,
            request_bridge_date,
            extract_interactions,
            load_cached_day,
            save_day_preview,
            export_configured_tracker,
            open_log_file,
            open_external_url,
            reveal_tracker_folder,
            save_qa_bridge_folder,
            qa_get_config,
            qa_save_config,
            qa_engine_status,
            qa_list_cases,
            qa_update_case,
            qa_reevaluate_case,
            qa_sync_now,
            qa_restart_history,
            qa_export_now,
            qa_open_output_folder,
            save_app_role
        ])
        .build(tauri::generate_context!())
        .expect("error while building Atlas");
    app.run(|app_handle, event| match event {
        tauri::RunEvent::WindowEvent {
            event: tauri::WindowEvent::CloseRequested { api, .. },
            label,
            ..
        } => {
            let Some(state) = app_handle.try_state::<AppState>() else {
                return;
            };
            if resident(&state) {
                // Keep the scheduler alive: the tracker's 09:30 run and the
                // QA engine need Atlas running. The tray icon reopens or quits.
                api.prevent_close();
                if let Some(window) = app_handle.get_webview_window(&label) {
                    let _ = window.hide();
                }
                state.foreground_requested.store(false, Ordering::SeqCst);
                diagnostics::info("window", "Atlas keeps running in the notification area");
            } else {
                state.local_ai.stop();
            }
        }
        tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => {
            if let Some(state) = app_handle.try_state::<AppState>() {
                state.local_ai.stop();
            }
        }
        _ => {}
    });
}

fn build_tray(app: &mut tauri::App, spanish: bool) -> std::result::Result<(), Box<dyn std::error::Error>> {
    use tauri::{
        menu::{Menu, MenuItem},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };
    let open_item = MenuItem::with_id(
        app,
        "open",
        if spanish { "Abrir Atlas" } else { "Open Atlas" },
        true,
        None::<&str>,
    )?;
    let quit_item = MenuItem::with_id(
        app,
        "quit",
        if spanish { "Salir de Atlas" } else { "Quit Atlas" },
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&open_item, &quit_item])?;
    let mut builder = TrayIconBuilder::with_id("atlas")
        .tooltip("Atlas")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.local_ai.stop();
                }
                diagnostics::info("window", "Atlas was closed from the notification area");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
