use crate::{
    diagnostics,
    error::{AppError, Result},
    models::{AutomationMode, AutomationRequest},
    state::AppState,
};
use chrono::{Datelike, Days, Local, NaiveDate, NaiveTime, Weekday};
use std::{fs, path::PathBuf, process::Command, sync::atomic::Ordering, time::Duration};
use tauri::{Emitter, Manager};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub const DEFAULT_DAILY_TIME: &str = "17:30";
const TASK_NAME: &str = "Atlas Daily Tracker";
const STARTUP_TASK_NAME: &str = "Atlas Background Startup";
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Atlas Background Startup";
const STARTUP_LINK: &str = "Atlas Background Startup.lnk";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn normalize_time(value: &str) -> Result<String> {
    let parsed = NaiveTime::parse_from_str(value.trim(), "%H:%M").map_err(|_| {
        AppError::Message("Choose a valid daily time between 00:00 and 23:59.".into())
    })?;
    Ok(parsed.format("%H:%M").to_string())
}

fn daily_run_due(now: NaiveTime, scheduled: NaiveTime, last_run: &str, today: &str) -> bool {
    now >= scheduled && last_run.trim() != today
}

fn previous_workday(mut date: NaiveDate) -> NaiveDate {
    date = date.checked_sub_days(Days::new(1)).unwrap_or(date);
    while matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
        date = date.checked_sub_days(Days::new(1)).unwrap_or(date);
    }
    date
}

fn marker_path(config_dir: &std::path::Path) -> PathBuf {
    config_dir.join("last-automatic-run.txt")
}

pub fn complete(config_dir: &std::path::Path, run_key: &str) -> Result<()> {
    let run_key = run_key.trim();
    if run_key.is_empty() {
        return Ok(());
    }
    fs::write(marker_path(config_dir), run_key).map_err(|error| {
        AppError::Message(format!(
            "Atlas completed the tracker but could not save its automatic-run marker: {error}"
        ))
    })?;
    diagnostics::info(
        "automation/complete",
        &format!("Automatic tracker completed successfully for {run_key}"),
    );
    Ok(())
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
fn configure_run_key(enabled: bool) -> Result<()> {
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
        diagnostics::info(
            "automation/startup-registry",
            if enabled {
                "Registered portable Atlas in the current user's Windows Run key"
            } else {
                "Removed Atlas from the current user's Windows Run key"
            },
        );
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
fn startup_link_path() -> Result<PathBuf> {
    let app_data = std::env::var_os("APPDATA").ok_or_else(|| {
        AppError::Message("Windows did not provide the current user's AppData folder.".into())
    })?;
    Ok(PathBuf::from(app_data)
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("Startup")
        .join(STARTUP_LINK))
}

#[cfg(windows)]
fn powershell_path() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .map(|root| {
            root.join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe")
        })
        .unwrap_or_else(|| PathBuf::from("powershell.exe"))
}

#[cfg(windows)]
fn configure_startup_shortcut(enabled: bool) -> Result<()> {
    let link = startup_link_path()?;
    if !enabled {
        match fs::remove_file(&link) {
            Ok(()) => diagnostics::info(
                "automation/startup-shortcut",
                &format!("Removed Atlas startup shortcut at {}", link.display()),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(AppError::Message(format!(
                    "Windows could not remove the Atlas startup shortcut: {error}"
                )))
            }
        }
        return Ok(());
    }

    let executable = std::env::current_exe().map_err(|error| {
        AppError::Message(format!("Atlas could not locate its executable: {error}"))
    })?;
    let working_directory = executable.parent().ok_or_else(|| {
        AppError::Message("Atlas could not determine its portable folder.".into())
    })?;
    let parent = link.parent().ok_or_else(|| {
        AppError::Message("Atlas could not determine the Windows Startup folder.".into())
    })?;
    fs::create_dir_all(parent)?;

    // Values travel through environment variables so paths containing quotes,
    // spaces, or PowerShell metacharacters are never interpreted as script text.
    let script = "$shell = New-Object -ComObject WScript.Shell; $shortcut = $shell.CreateShortcut($env:ATLAS_STARTUP_LINK); $shortcut.TargetPath = $env:ATLAS_STARTUP_EXE; $shortcut.Arguments = '--atlas-startup'; $shortcut.WorkingDirectory = $env:ATLAS_STARTUP_WORKDIR; $shortcut.WindowStyle = 7; $shortcut.Description = 'Atlas background tracker'; $shortcut.Save()";
    let output = Command::new(powershell_path())
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .env("ATLAS_STARTUP_LINK", &link)
        .env("ATLAS_STARTUP_EXE", &executable)
        .env("ATLAS_STARTUP_WORKDIR", working_directory)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| {
            AppError::Message(format!(
                "Windows could not create the Atlas startup shortcut: {error}"
            ))
        })?;
    if !output.status.success() || !link.is_file() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::Message(format!(
            "Windows did not create the Atlas startup shortcut{}{}",
            if detail.is_empty() { "." } else { ": " },
            detail
        )));
    }
    diagnostics::info(
        "automation/startup-shortcut",
        &format!(
            "Created Atlas startup shortcut at {} for {} --atlas-startup",
            link.display(),
            executable.display()
        ),
    );
    Ok(())
}

#[cfg(windows)]
pub fn configure(enabled: bool, time: &str) -> Result<()> {
    normalize_time(time)?;
    // Older releases used Task Scheduler, which many managed PCs block for
    // standard users. Remove those tasks when possible and use the current
    // user's Run key plus Atlas's own daily timer instead.
    remove_legacy_task(TASK_NAME);
    remove_legacy_task(STARTUP_TASK_NAME);
    let registry = configure_run_key(enabled);
    let shortcut = configure_startup_shortcut(enabled);
    match (registry, shortcut) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(error)) | (Err(error), Ok(())) => {
            diagnostics::error(
                "automation/startup-fallback",
                &format!("One Windows startup method failed; the fallback is active: {error}"),
            );
            Ok(())
        }
        (Err(registry_error), Err(shortcut_error)) => Err(AppError::Message(format!(
            "Windows could not register Atlas at startup. Run key: {registry_error} Startup folder: {shortcut_error}"
        ))),
    }
}

#[cfg(not(windows))]
pub fn configure(_enabled: bool, time: &str) -> Result<()> {
    normalize_time(time).map(|_| ())
}

pub fn start_background_scheduler(app: tauri::AppHandle, config_dir: PathBuf) {
    tauri::async_runtime::spawn(async move {
        // Give OneDrive time to initialize after Windows sign-in. The event is
        // emitted only after the WebView confirms its listener is attached.
        tokio::time::sleep(Duration::from_secs(20)).await;
        let wait_started = std::time::Instant::now();
        loop {
            let Some(state) = app.try_state::<AppState>() else {
                tokio::time::sleep(Duration::from_secs(20)).await;
                continue;
            };
            let Ok(settings) = state.read_settings() else {
                tokio::time::sleep(Duration::from_secs(20)).await;
                continue;
            };
            if !settings.auto_sync || settings.destination.is_none() || settings.profile.is_none() {
                tokio::time::sleep(Duration::from_secs(20)).await;
                continue;
            }
            let now = Local::now();
            let today = now.format("%Y-%m-%d").to_string();
            let last_run = fs::read_to_string(marker_path(&config_dir)).unwrap_or_default();
            let request = match settings.automation_mode {
                AutomationMode::StartupPreviousWorkday => {
                    if !state.background_launch {
                        return;
                    }
                    let date = previous_workday(now.date_naive())
                        .format("%Y-%m-%d")
                        .to_string();
                    if last_run.trim() == date {
                        diagnostics::info(
                            "automation/run",
                            &format!("Previous workday {date} was already completed; exiting"),
                        );
                        app.exit(0);
                        return;
                    }
                    AutomationRequest {
                        date: date.clone(),
                        run_key: date,
                        reason: "windows_startup".into(),
                    }
                }
                AutomationMode::DailyTime => {
                    let Ok(time) = NaiveTime::parse_from_str(&settings.auto_sync_time, "%H:%M")
                    else {
                        tokio::time::sleep(Duration::from_secs(20)).await;
                        continue;
                    };
                    if !daily_run_due(now.time(), time, &last_run, &today) {
                        tokio::time::sleep(Duration::from_secs(20)).await;
                        continue;
                    }
                    AutomationRequest {
                        date: today.clone(),
                        run_key: today,
                        reason: "daily_time".into(),
                    }
                }
            };
            if !state.frontend_ready.load(Ordering::SeqCst) {
                if state.background_launch && wait_started.elapsed() >= Duration::from_secs(120) {
                    diagnostics::error(
                        "automation/frontend",
                        "The hidden interface did not become ready within 120 seconds; opening Atlas for attention",
                    );
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                    return;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            if state.automation_pending.swap(true, Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_secs(20)).await;
                continue;
            }
            diagnostics::info(
                "automation/run",
                &format!(
                    "Starting hidden {} tracker for {}",
                    request.reason, request.date
                ),
            );
            if let Err(error) = app.emit("atlas-background-daily-run", request) {
                state.automation_pending.store(false, Ordering::SeqCst);
                diagnostics::error("automation/emit", &error.to_string());
            }
            if settings.automation_mode == AutomationMode::StartupPreviousWorkday {
                return;
            }
            tokio::time::sleep(Duration::from_secs(20)).await;
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

    #[test]
    fn startup_uses_the_previous_business_day() {
        assert_eq!(
            previous_workday(NaiveDate::from_ymd_opt(2026, 9, 16).unwrap()),
            NaiveDate::from_ymd_opt(2026, 9, 15).unwrap()
        );
        assert_eq!(
            previous_workday(NaiveDate::from_ymd_opt(2026, 9, 14).unwrap()),
            NaiveDate::from_ymd_opt(2026, 9, 11).unwrap()
        );
    }
}
