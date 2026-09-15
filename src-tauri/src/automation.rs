use crate::{
    diagnostics,
    error::{AppError, Result},
    state::AppState,
};
use chrono::{Local, NaiveTime};
use std::{fs, path::PathBuf, process::Command, time::Duration};
use tauri::{Emitter, Manager};

pub const DEFAULT_DAILY_TIME: &str = "17:30";
const TASK_NAME: &str = "Atlas Daily Tracker";
const STARTUP_TASK_NAME: &str = "Atlas Background Startup";
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Atlas Background Startup";

pub fn normalize_time(value: &str) -> Result<String> {
    let parsed = NaiveTime::parse_from_str(value.trim(), "%H:%M").map_err(|_| {
        AppError::Message("Choose a valid daily time between 00:00 and 23:59.".into())
    })?;
    Ok(parsed.format("%H:%M").to_string())
}

fn daily_run_due(now: NaiveTime, scheduled: NaiveTime, last_run: &str, today: &str) -> bool {
    now >= scheduled && last_run.trim() != today
}

#[cfg(windows)]
fn schtasks_path() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .map(|root| root.join("System32").join("schtasks.exe"))
        .unwrap_or_else(|| PathBuf::from("schtasks.exe"))
}

#[cfg(windows)]
fn remove_legacy_task(name: &str) {
    let _ = Command::new(schtasks_path())
        .args(["/Delete", "/TN", name, "/F"])
        .output();
}

#[cfg(windows)]
fn reg_path() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .map(|root| root.join("System32").join("reg.exe"))
        .unwrap_or_else(|| PathBuf::from("reg.exe"))
}

#[cfg(windows)]
fn configure_startup(enabled: bool) -> Result<()> {
    let mut arguments = vec![
        if enabled { "ADD" } else { "DELETE" }.to_string(),
        RUN_KEY.to_string(),
        "/V".to_string(),
        RUN_VALUE.to_string(),
    ];
    if enabled {
        let executable = std::env::current_exe().map_err(|error| {
            AppError::Message(format!("Atlas could not locate its executable: {error}"))
        })?;
        let action = format!("\"{}\" --atlas-startup", executable.display());
        arguments.extend([
            "/T".into(),
            "REG_SZ".into(),
            "/D".into(),
            action,
            "/F".into(),
        ]);
    } else {
        arguments.push("/F".into());
    }
    let output = Command::new(reg_path())
        .args(arguments)
        .output()
        .map_err(|error| {
            AppError::Message(format!("Windows startup could not be configured: {error}"))
        })?;
    if output.status.success() || !enabled {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(AppError::Message(format!(
        "Windows could not register Atlas for the current user{}{}",
        if detail.is_empty() { "." } else { ": " },
        detail
    )))
}

#[cfg(windows)]
pub fn configure(enabled: bool, time: &str) -> Result<()> {
    normalize_time(time)?;
    // Older releases used Task Scheduler, which many managed PCs block for
    // standard users. Remove those tasks when possible and use the current
    // user's Run key plus Atlas's own daily timer instead.
    remove_legacy_task(TASK_NAME);
    remove_legacy_task(STARTUP_TASK_NAME);
    configure_startup(enabled)
}

#[cfg(not(windows))]
pub fn configure(_enabled: bool, time: &str) -> Result<()> {
    normalize_time(time).map(|_| ())
}

pub fn start_background_scheduler(app: tauri::AppHandle, config_dir: PathBuf) {
    tauri::async_runtime::spawn(async move {
        let marker = config_dir.join("last-automatic-run.txt");
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            let Ok(settings) = state.read_settings() else {
                continue;
            };
            if !settings.auto_sync || settings.destination.is_none() || settings.profile.is_none() {
                continue;
            }
            let Ok(time) = NaiveTime::parse_from_str(&settings.auto_sync_time, "%H:%M") else {
                continue;
            };
            let now = Local::now();
            let today = now.format("%Y-%m-%d").to_string();
            let last_run = fs::read_to_string(&marker).unwrap_or_default();
            if !daily_run_due(now.time(), time, &last_run, &today) {
                continue;
            }
            if let Err(error) = fs::write(&marker, &today) {
                diagnostics::error("automation/marker", &error.to_string());
                continue;
            }
            diagnostics::info(
                "automation/run",
                &format!("Starting background daily tracker for {today}"),
            );
            if let Err(error) = app.emit("atlas-background-daily-run", ()) {
                diagnostics::error("automation/emit", &error.to_string());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_and_normalizes_daily_time() {
        assert_eq!(normalize_time("17:30").unwrap(), "17:30");
        assert_eq!(normalize_time(" 05:07 ").unwrap(), "05:07");
    }

    #[test]
    fn rejects_invalid_daily_time() {
        assert!(normalize_time("25:00").is_err());
        assert!(normalize_time("five thirty").is_err());
    }

    #[test]
    fn background_timer_catches_up_once_per_day() {
        let scheduled = NaiveTime::parse_from_str("17:30", "%H:%M").unwrap();
        let before = NaiveTime::parse_from_str("17:29", "%H:%M").unwrap();
        let after = NaiveTime::parse_from_str("18:05", "%H:%M").unwrap();
        assert!(!daily_run_due(before, scheduled, "", "2026-09-15"));
        assert!(daily_run_due(after, scheduled, "2026-09-14", "2026-09-15"));
        assert!(!daily_run_due(
            after,
            scheduled,
            "2026-09-15\n",
            "2026-09-15"
        ));
    }
}
