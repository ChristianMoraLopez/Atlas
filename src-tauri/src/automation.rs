use crate::{
    diagnostics,
    error::{AppError, Result},
    models::{AppRole, AutomationMode, AutomationRequest},
    qa_engine,
    state::AppState,
};
use chrono::{Datelike, Days, Local, NaiveDate, NaiveTime, Weekday};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub const DEFAULT_DAILY_TIME: &str = "17:30";
/// When Atlas stays running overnight (no Windows sign-in, so no startup
/// launch), the previous workday is processed at this local time.
pub const PREVIOUS_DAY_FALLBACK: &str = "09:30";
const TASK_NAME: &str = "Atlas Daily Tracker";
const STARTUP_TASK_NAME: &str = "Atlas Background Startup";
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Atlas Background Startup";
const STARTUP_LINK: &str = "Atlas Background Startup.lnk";
const RETRY_COOLDOWN: Duration = Duration::from_secs(45 * 60);
const MAX_ATTEMPTS_PER_RUN: u32 = 4;
const QA_TICK_INTERVAL: Duration = Duration::from_secs(60);
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

/// The previous workday runs right after a Windows startup launch, and
/// otherwise once the fallback time is reached while Atlas keeps running.
fn previous_day_due(now: NaiveTime, startup_launch_pending: bool, last_run: &str, date: &str) -> bool {
    let fallback = NaiveTime::parse_from_str(PREVIOUS_DAY_FALLBACK, "%H:%M").expect("valid time");
    last_run.trim() != date && (startup_launch_pending || now >= fallback)
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
fn system_tool(name: &str) -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .map(|root| root.join("System32").join(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

#[cfg(windows)]
fn remove_legacy_task(name: &str) {
    let _ = Command::new(system_tool("schtasks.exe"))
        .args(["/Query", "/TN", name])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|_| {
            Command::new(system_tool("schtasks.exe"))
                .args(["/Delete", "/TN", name, "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        });
}

#[cfg(windows)]
fn startup_action() -> Result<String> {
    let executable = std::env::current_exe().map_err(|error| {
        AppError::Message(format!("Atlas could not locate its executable: {error}"))
    })?;
    Ok(format!("\"{}\" --atlas-startup", executable.display()))
}

#[cfg(windows)]
fn run_key_value() -> Option<String> {
    let output = Command::new(system_tool("reg.exe"))
        .args(["QUERY", RUN_KEY, "/V", RUN_VALUE])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .find(|line| line.contains("REG_SZ"))
        .and_then(|line| line.split_once("REG_SZ"))
        .map(|(_, value)| value.trim().to_string())
}

#[cfg(windows)]
fn configure_run_key(enabled: bool) -> Result<bool> {
    if enabled {
        let action = startup_action()?;
        if run_key_value().as_deref() == Some(action.as_str()) {
            return Ok(false);
        }
        let output = Command::new(system_tool("reg.exe"))
            .args(["ADD", RUN_KEY, "/V", RUN_VALUE, "/T", "REG_SZ", "/D", &action, "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| {
                AppError::Message(format!("Windows startup could not be configured: {error}"))
            })?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AppError::Message(format!(
                "Windows could not register Atlas for the current user{}{}",
                if detail.is_empty() { "." } else { ": " },
                detail
            )));
        }
        diagnostics::info(
            "automation/startup-registry",
            "Registered portable Atlas in the current user's Windows Run key",
        );
        return Ok(true);
    }
    if run_key_value().is_some() {
        let _ = Command::new(system_tool("reg.exe"))
            .args(["DELETE", RUN_KEY, "/V", RUN_VALUE, "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        diagnostics::info(
            "automation/startup-registry",
            "Removed Atlas from the current user's Windows Run key",
        );
    }
    Ok(false)
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
fn remove_startup_shortcut() -> Result<()> {
    let link = startup_link_path()?;
    match fs::remove_file(&link) {
        Ok(()) => {
            diagnostics::info(
                "automation/startup-shortcut",
                &format!("Removed Atlas startup shortcut at {}", link.display()),
            );
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::Message(format!(
            "Windows could not remove the Atlas startup shortcut: {error}"
        ))),
    }
}

#[cfg(windows)]
fn create_startup_shortcut() -> Result<()> {
    let link = startup_link_path()?;
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
    let output = Command::new(system_tool(r"WindowsPowerShell\v1.0\powershell.exe"))
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

/// Registers Atlas to start hidden at Windows sign-in. Idempotent: nothing
/// is rewritten when the registration already points at this executable.
/// Only one method is kept active so Windows never launches Atlas twice.
#[cfg(windows)]
pub fn configure(enabled: bool, time: &str) -> Result<()> {
    normalize_time(time)?;
    // Older releases used Task Scheduler, which many managed PCs block for
    // standard users. Remove those tasks when present.
    remove_legacy_task(TASK_NAME);
    remove_legacy_task(STARTUP_TASK_NAME);
    if !enabled {
        configure_run_key(false)?;
        return remove_startup_shortcut();
    }
    match configure_run_key(true) {
        Ok(_) => remove_startup_shortcut().or_else(|error| {
            diagnostics::error("automation/startup-shortcut", &error.to_string());
            Ok(())
        }),
        Err(registry_error) => {
            diagnostics::error(
                "automation/startup-fallback",
                &format!("The Run key failed; using the Startup folder instead: {registry_error}"),
            );
            let link_exists = startup_link_path().map(|link| link.is_file()).unwrap_or(false);
            if link_exists {
                return Ok(());
            }
            create_startup_shortcut().map_err(|shortcut_error| {
                AppError::Message(format!(
                    "Windows could not register Atlas at startup. Run key: {registry_error} Startup folder: {shortcut_error}"
                ))
            })
        }
    }
}

#[cfg(not(windows))]
pub fn configure(_enabled: bool, time: &str) -> Result<()> {
    normalize_time(time).map(|_| ())
}

struct SchedulerContext {
    startup_launch_pending: bool,
    attempts: HashMap<String, (u32, Instant)>,
    last_qa_tick: Option<Instant>,
    wait_started: Instant,
    attention_shown: bool,
    logged_done: Option<String>,
}

/// Atlas stays resident (hidden, with a tray icon) so it can run the
/// tracker at 09:30 when the PC was never restarted, and keep the QA engine
/// working for managers.
pub fn start_background_scheduler(app: tauri::AppHandle, config_dir: PathBuf) {
    tauri::async_runtime::spawn(async move {
        let background = app
            .try_state::<AppState>()
            .map(|state| state.background_launch)
            .unwrap_or(false);
        // Give OneDrive time to initialize after Windows sign-in. The event is
        // emitted only after the WebView confirms its listener is attached.
        tokio::time::sleep(Duration::from_secs(if background { 20 } else { 5 })).await;
        let mut context = SchedulerContext {
            startup_launch_pending: background,
            attempts: HashMap::new(),
            last_qa_tick: None,
            wait_started: Instant::now(),
            attention_shown: false,
            logged_done: None,
        };
        loop {
            scheduler_step(&app, &config_dir, &mut context);
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    });
}

fn scheduler_step(app: &tauri::AppHandle, config_dir: &std::path::Path, context: &mut SchedulerContext) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(settings) = state.read_settings() else {
        return;
    };
    match settings.app_role {
        Some(AppRole::Manager) => {
            context.startup_launch_pending = false;
            if context
                .last_qa_tick
                .is_none_or(|last| last.elapsed() >= QA_TICK_INTERVAL)
            {
                context.last_qa_tick = Some(Instant::now());
                tauri::async_runtime::spawn(qa_engine::tick(app.clone()));
            }
            return;
        }
        Some(AppRole::Cs) => {}
        None => return,
    }
    if !settings.auto_sync || settings.destination.is_none() || settings.profile.is_none() {
        return;
    }
    let now = Local::now();
    let today = now.format("%Y-%m-%d").to_string();
    let last_run = fs::read_to_string(marker_path(config_dir)).unwrap_or_default();
    let request = match settings.automation_mode {
        AutomationMode::StartupPreviousWorkday => {
            let date = previous_workday(now.date_naive())
                .format("%Y-%m-%d")
                .to_string();
            if last_run.trim() == date {
                context.startup_launch_pending = false;
                if context.logged_done.as_deref() != Some(date.as_str()) {
                    diagnostics::info(
                        "automation/run",
                        &format!("Previous workday {date} is already in the tracker"),
                    );
                    context.logged_done = Some(date);
                }
                return;
            }
            if !previous_day_due(now.time(), context.startup_launch_pending, &last_run, &date) {
                return;
            }
            AutomationRequest {
                date: date.clone(),
                run_key: date,
                reason: if context.startup_launch_pending {
                    "windows_startup".into()
                } else {
                    "previous_day_0930".into()
                },
            }
        }
        AutomationMode::DailyTime => {
            let Ok(time) = NaiveTime::parse_from_str(&settings.auto_sync_time, "%H:%M") else {
                return;
            };
            if !daily_run_due(now.time(), time, &last_run, &today) {
                return;
            }
            AutomationRequest {
                date: today.clone(),
                run_key: today,
                reason: "daily_time".into(),
            }
        }
    };
    if let Some((count, last)) = context.attempts.get(&request.run_key) {
        if *count >= MAX_ATTEMPTS_PER_RUN || last.elapsed() < RETRY_COOLDOWN {
            return;
        }
    }
    if !state.frontend_ready.load(Ordering::SeqCst) {
        if state.background_launch
            && !context.attention_shown
            && context.wait_started.elapsed() >= Duration::from_secs(120)
        {
            context.attention_shown = true;
            diagnostics::error(
                "automation/frontend",
                "The hidden interface did not become ready within 120 seconds; opening Atlas for attention",
            );
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        return;
    }
    if state.automation_pending.swap(true, Ordering::SeqCst) {
        return;
    }
    let entry = context
        .attempts
        .entry(request.run_key.clone())
        .or_insert((0, Instant::now()));
    entry.0 += 1;
    entry.1 = Instant::now();
    let hidden = app
        .get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .is_some_and(|visible| !visible);
    state.hidden_run.store(hidden, Ordering::SeqCst);
    context.startup_launch_pending = false;
    diagnostics::info(
        "automation/run",
        &format!(
            "Starting {} {} tracker for {} (attempt {})",
            if hidden { "hidden" } else { "visible" },
            request.reason,
            request.date,
            entry.0
        ),
    );
    if let Err(error) = app.emit("atlas-background-daily-run", request) {
        state.automation_pending.store(false, Ordering::SeqCst);
        diagnostics::error("automation/emit", &error.to_string());
    }
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
    fn previous_day_runs_at_startup_or_from_0930_when_atlas_stayed_open() {
        let early = NaiveTime::from_hms_opt(8, 10, 0).unwrap();
        let fallback = NaiveTime::from_hms_opt(9, 30, 0).unwrap();
        let late = NaiveTime::from_hms_opt(14, 0, 0).unwrap();
        // Windows sign-in launch: run immediately, even before 09:30.
        assert!(previous_day_due(early, true, "2026-09-21", "2026-09-22"));
        // PC left on overnight: wait for 09:30, then catch up.
        assert!(!previous_day_due(early, false, "2026-09-21", "2026-09-22"));
        assert!(previous_day_due(fallback, false, "2026-09-21", "2026-09-22"));
        assert!(previous_day_due(late, false, "", "2026-09-22"));
        // Never twice for the same workday.
        assert!(!previous_day_due(late, true, "2026-09-22\n", "2026-09-22"));
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
