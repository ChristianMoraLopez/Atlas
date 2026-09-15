use crate::error::{AppError, Result};
use chrono::NaiveTime;
use std::{path::PathBuf, process::Command};

pub const DEFAULT_DAILY_TIME: &str = "17:30";
const TASK_NAME: &str = "Atlas Daily Tracker";
const STARTUP_TASK_NAME: &str = "Atlas Background Startup";

pub fn normalize_time(value: &str) -> Result<String> {
    let parsed = NaiveTime::parse_from_str(value.trim(), "%H:%M").map_err(|_| {
        AppError::Message("Choose a valid daily time between 00:00 and 23:59.".into())
    })?;
    Ok(parsed.format("%H:%M").to_string())
}

#[cfg(windows)]
fn schtasks_path() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .map(|root| root.join("System32").join("schtasks.exe"))
        .unwrap_or_else(|| PathBuf::from("schtasks.exe"))
}

#[cfg(windows)]
fn run_schtasks(arguments: &[String], deleting: bool) -> Result<()> {
    let output = Command::new(schtasks_path())
        .args(arguments)
        .output()
        .map_err(|error| {
            AppError::Message(format!(
                "Windows Task Scheduler could not be started: {error}"
            ))
        })?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if deleting
        && (detail.contains("cannot find")
            || detail.contains("not exist")
            || detail.contains("no existe")
            || detail.contains("no puede encontrar"))
    {
        return Ok(());
    }
    Err(AppError::Message(format!(
        "Windows could not {} the daily Atlas task{}{}",
        if deleting { "remove" } else { "schedule" },
        if detail.is_empty() { "." } else { ": " },
        detail
    )))
}

#[cfg(windows)]
pub fn configure(enabled: bool, time: &str) -> Result<()> {
    if !enabled {
        run_schtasks(
            &["/Delete", "/TN", TASK_NAME, "/F"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            true,
        )?;
        return run_schtasks(
            &["/Delete", "/TN", STARTUP_TASK_NAME, "/F"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            true,
        );
    }
    let time = normalize_time(time)?;
    let executable = std::env::current_exe().map_err(|error| {
        AppError::Message(format!("Atlas could not locate its executable: {error}"))
    })?;
    let action = format!("\"{}\" --atlas-daily-run", executable.display());
    run_schtasks(
        &[
            "/Create".into(),
            "/SC".into(),
            "DAILY".into(),
            "/ST".into(),
            time,
            "/TN".into(),
            TASK_NAME.into(),
            "/TR".into(),
            action,
            "/RL".into(),
            "LIMITED".into(),
            "/IT".into(),
            "/F".into(),
        ],
        false,
    )?;
    let startup_action = format!("\"{}\" --atlas-startup", executable.display());
    run_schtasks(
        &[
            "/Create".into(),
            "/SC".into(),
            "ONLOGON".into(),
            "/TN".into(),
            STARTUP_TASK_NAME.into(),
            "/TR".into(),
            startup_action,
            "/RL".into(),
            "LIMITED".into(),
            "/IT".into(),
            "/F".into(),
        ],
        false,
    )
}

#[cfg(not(windows))]
pub fn configure(_enabled: bool, time: &str) -> Result<()> {
    normalize_time(time).map(|_| ())
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
}
