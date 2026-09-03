mod auth;
mod error;
mod excel;
mod graph;
mod models;
mod ollama;
mod state;

use crate::{
    error::{AppError, Result},
    models::{
        AppStatus, ExportResult, ExtractionResult, Interaction, MicrosoftConfig, SourceKind,
        UserProfile,
    },
    state::AppState,
};
use std::collections::HashSet;
use tauri::Manager;

async fn build_status(state: &AppState) -> Result<AppStatus> {
    let settings = state.read_settings()?;
    let microsoft_config = state.microsoft_config()?;
    let (ollama_running, ollama_model_available) =
        ollama::status(&state.http, &settings.ollama_model).await;
    Ok(AppStatus {
        configured: !microsoft_config.client_id.is_empty()
            && !microsoft_config.tenant_id.is_empty(),
        signed_in: auth::has_token() && settings.account.is_some(),
        microsoft_config,
        account: settings.account,
        profile: settings.profile,
        ollama_running,
        ollama_model_available,
        ollama_model: settings.ollama_model,
    })
}

#[tauri::command]
async fn get_app_status(state: tauri::State<'_, AppState>) -> Result<AppStatus> {
    build_status(&state).await
}

#[tauri::command]
async fn sign_in(state: tauri::State<'_, AppState>) -> Result<AppStatus> {
    let account = auth::sign_in(&state).await?;
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
        auth::clear_token()?;
    }
    state.update_settings(|settings| {
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
fn sign_out(state: tauri::State<'_, AppState>) -> Result<()> {
    auth::clear_token()?;
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
fn set_ollama_model(state: tauri::State<'_, AppState>, model: String) -> Result<()> {
    let model = model.trim();
    if model.is_empty()
        || model.len() > 100
        || !model
            .chars()
            .all(|v| v.is_ascii_alphanumeric() || ":._-/".contains(v))
    {
        return Err(AppError::Message(
            "Enter a valid local Ollama model name.".into(),
        ));
    }
    state.update_settings(|settings| settings.ollama_model = model.to_string())
}

#[tauri::command]
async fn extract_interactions(
    state: tauri::State<'_, AppState>,
    date: String,
    include_email: bool,
    include_teams: bool,
    timezone: String,
) -> Result<ExtractionResult> {
    if state.read_settings()?.profile.is_none() {
        return Err(AppError::Message(
            "Complete your profile before extracting interactions.".into(),
        ));
    }
    let token = auth::access_token(&state).await?;
    let model = state.read_settings()?.ollama_model;
    let result = graph::extract(
        &state,
        &token,
        &date,
        &timezone,
        include_email,
        include_teams,
        &model,
    )
    .await?;
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
                "Rejected '{}': it was not returned by Microsoft Graph during this extraction.",
                item.evidence_label
            ))
        })?;
        if trusted.source_kind != item.source_kind {
            return Err(AppError::Message(
                "Rejected an interaction whose source kind does not match its Graph evidence."
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
async fn export_tracker(
    state: tauri::State<'_, AppState>,
    path: String,
    existing: bool,
    date: String,
    profile: UserProfile,
    interactions: Vec<Interaction>,
) -> Result<ExportResult> {
    chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| AppError::Message("Choose a valid export date.".into()))?;
    let saved = state
        .read_settings()?
        .profile
        .ok_or_else(|| AppError::Message("Complete your profile before exporting.".into()))?;
    if saved != profile {
        return Err(AppError::Message(
            "Your profile changed before export. Refresh the workspace and try again.".into(),
        ));
    }
    validate_provenance(&state, &interactions)?;
    tauri::async_runtime::spawn_blocking(move || {
        excel::export(&path, existing, &saved, &interactions)
    })
    .await
    .map_err(|e| AppError::Message(format!("Excel export task failed: {e}")))?
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
            app.manage(AppState::new(config_dir)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_status,
            save_microsoft_config,
            sign_in,
            sign_out,
            save_profile,
            set_ollama_model,
            extract_interactions,
            export_tracker
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
    }
}
