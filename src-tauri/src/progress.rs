//! Live progress for the window: long imports report what they are reading or
//! interpreting so the user can tell Atlas is working, not stuck. The message is
//! an i18n key plus values; the frontend translates it.

use serde::Serialize;
use std::sync::OnceLock;
use tauri::Emitter;

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

#[derive(Clone, Serialize)]
struct Progress<'a> {
    key: &'a str,
    values: &'a [(&'a str, String)],
}

pub fn init(app: tauri::AppHandle) {
    let _ = APP.set(app);
}

pub fn report(key: &str, values: &[(&str, String)]) {
    if let Some(app) = APP.get() {
        let _ = app.emit("atlas-progress", Progress { key, values });
    }
}
