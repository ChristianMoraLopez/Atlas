use chrono::Utc;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOG_LOCK: Mutex<()> = Mutex::new(());

pub fn init(config_dir: &Path) -> std::io::Result<PathBuf> {
    let log_dir = config_dir.join("logs");
    fs::create_dir_all(&log_dir)?;
    let path = log_dir.join("atlas.log");
    let _ = LOG_PATH.set(path.clone());
    log("INFO", "startup", "Atlas diagnostics initialized");
    Ok(path)
}

pub fn path() -> String {
    LOG_PATH
        .get()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default()
}

pub fn log(level: &str, context: &str, message: &str) {
    let Some(path) = LOG_PATH.get() else {
        return;
    };
    let Ok(_guard) = LOG_LOCK.lock() else {
        return;
    };
    let clean_context = one_line(context);
    let clean_message = one_line(message);
    let line = format!(
        "{} [{}] {}: {}\n",
        Utc::now().to_rfc3339(),
        level,
        clean_context,
        clean_message
    );
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn info(context: &str, message: &str) {
    log("INFO", context, message);
}

pub fn error(context: &str, message: &str) {
    log("ERROR", context, message);
}

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let payload = panic
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| panic.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("Unknown panic");
        let location = panic
            .location()
            .map(|value| format!("{}:{}", value.file(), value.line()))
            .unwrap_or_else(|| "unknown location".into());
        error("panic", &format!("{payload} ({location})"));
        previous(panic);
    }));
}

fn one_line(value: &str) -> String {
    value
        .replace('\r', " ")
        .replace('\n', " ")
        .chars()
        .take(4_000)
        .collect()
}
