//! Portal-assisted installation. This module never obtains Microsoft credentials,
//! invokes PAC, controls a browser, or calls a tenant API.
use crate::{
    diagnostics,
    error::{AppError, Result},
    evidence_validation,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

const SOLUTION: &[u8] = include_bytes!("../../power-automate/AtlasBridge_1_0_0_0.zip");
const SCHEDULED_WORKFLOW: &str =
    "Workflows/AtlasExportEvidence-8e5c1f84-dcbb-4a2c-9d2f-62e9c38105d2.json";
const TEAMS_EVENT_WORKFLOW: &str =
    "Workflows/AtlasCaptureTeamsMessages-7f7115e8-b821-4bb9-9b4e-5c7a5448610e.json";
const WORKFLOWS: [&str; 2] = [SCHEDULED_WORKFLOW, TEAMS_EVENT_WORKFLOW];
const MAX_FILE: u64 = 25 * 1024 * 1024;
const PORTAL: &str = "https://make.powerautomate.com/";
const SOLUTION_VERSION: &str = "1.11.0.0";

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    CheckingRequirements,
    WaitingSignIn,
    FindingEnvironment,
    FindingConnections,
    ImportingSolution,
    ActivatingFlow,
    VerifyingFile,
    Completed,
    BlockedByPolicy,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Session {
    pub phase: Phase,
    pub resume_phase: Option<Phase>,
    pub installation_id: String,
    pub started_at: String,
    pub folder: Option<String>,
    pub calendar_name: String,
    pub environment_id: Option<String>,
    pub diagnostic: String,
    pub solution_version: Option<String>,
    #[serde(default)]
    pub last_checked_at: Option<String>,
    #[serde(default)]
    pub last_checked_file: Option<String>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            phase: Phase::CheckingRequirements,
            resume_phase: None,
            installation_id: uuid::Uuid::new_v4().to_string(),
            started_at: Utc::now().to_rfc3339(),
            folder: None,
            calendar_name: "Calendar".into(),
            environment_id: None,
            diagnostic: "portal_required".into(),
            solution_version: Some(SOLUTION_VERSION.into()),
            last_checked_at: None,
            last_checked_file: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub session: Session,
    pub package_path: String,
    pub one_drive_root: Option<String>,
    pub pac_detected: bool,
}

pub struct Installer {
    directory: PathBuf,
    session: Mutex<Session>,
}

fn fail(code: &str) -> AppError {
    AppError::Message(format!("Atlas connector: {code}"))
}

impl Installer {
    pub fn new(directory: PathBuf) -> Self {
        let session = fs::read(directory.join("session.json"))
            .ok()
            .filter(|data| data.len() < 16_384)
            .and_then(|data| serde_json::from_slice::<Session>(&data).ok())
            .filter(|s| {
                uuid::Uuid::parse_str(&s.installation_id).is_ok()
                    && DateTime::parse_from_rfc3339(&s.started_at).is_ok()
                    && s.solution_version.as_deref() == Some(SOLUTION_VERSION)
            })
            .unwrap_or_default();
        Self {
            directory,
            session: Mutex::new(session),
        }
    }

    fn package_path(&self) -> PathBuf {
        self.directory.join("AtlasBridge_1_0_0_0.zip")
    }

    fn persist(&self, next: &Session) -> Result<()> {
        fs::create_dir_all(&self.directory).map_err(|_| fail("settings_not_writable"))?;
        let staging = self.directory.join("session.pending.json");
        fs::write(&staging, serde_json::to_vec_pretty(next)?)
            .map_err(|_| fail("settings_not_writable"))?;
        replace_file(&staging, &self.directory.join("session.json"))
            .map_err(|_| fail("settings_not_writable"))?;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        Ok(Snapshot {
            session: self
                .session
                .lock()
                .map_err(|_| fail("installer_busy"))?
                .clone(),
            package_path: self.package_path().to_string_lossy().into_owned(),
            one_drive_root: std::env::var("OneDriveCommercial")
                .ok()
                .filter(|p| Path::new(p).is_dir()),
            // Presence only; never execute arbitrary PATH binaries or read auth profiles.
            pac_detected: detect_pac(std::env::var_os("PATH").as_deref()),
        })
    }

    pub fn action(&self, action: Action) -> Result<Snapshot> {
        let mut guard = self.session.lock().map_err(|_| fail("installer_busy"))?;
        let mut next = guard.clone();
        match action {
            Action::Prepare {
                one_drive_root,
                calendar_name,
            } => {
                if next.phase != Phase::CheckingRequirements {
                    return Err(fail("invalid_transition"));
                }
                if calendar_name.chars().count() > 128
                    || calendar_name.chars().any(char::is_control)
                {
                    return Err(fail("invalid_calendar_name"));
                }
                next.folder = Some(
                    prepare_inbox(Path::new(&one_drive_root))?
                        .to_string_lossy()
                        .into_owned(),
                );
                next.calendar_name = calendar_name.trim().to_string();
                fs::create_dir_all(&self.directory).map_err(|_| fail("settings_not_writable"))?;
                let bytes = personalized_solution(&next)?;
                let staging = self.directory.join("solution.pending.zip");
                fs::write(&staging, bytes).map_err(|_| fail("package_not_writable"))?;
                replace_file(&staging, &self.package_path())
                    .map_err(|_| fail("package_not_writable"))?;
                next.phase = Phase::WaitingSignIn;
                next.diagnostic = "portal_required".into();
            }
            Action::ConfirmSignIn {} => {
                advance(&mut next, Phase::WaitingSignIn, Phase::FindingConnections)?
            }
            Action::ConfirmEnvironment { environment_id } => {
                let id = if environment_id.trim().is_empty() {
                    None
                } else {
                    Some(
                        parse_environment(&environment_id)
                            .ok_or_else(|| fail("invalid_environment_id"))?,
                    )
                };
                advance(
                    &mut next,
                    Phase::FindingEnvironment,
                    Phase::FindingConnections,
                )?;
                next.environment_id = id;
            }
            Action::ConfirmConnections {} => advance(
                &mut next,
                Phase::FindingConnections,
                Phase::ImportingSolution,
            )?,
            Action::ConfirmImport {} => {
                advance(&mut next, Phase::ImportingSolution, Phase::VerifyingFile)?
            }
            Action::ConfirmActive {} => {
                advance(&mut next, Phase::ActivatingFlow, Phase::VerifyingFile)?
            }
            Action::Verify {} => {
                if next.phase != Phase::VerifyingFile {
                    return Err(fail("invalid_transition"));
                }
                let verification = verify_inbox(&next);
                next.last_checked_at = Some(Utc::now().to_rfc3339());
                next.last_checked_file = verification.file_name.clone();
                next.diagnostic = verification.diagnostic.into();
                diagnostics::info(
                    "connector/verify",
                    &format!(
                        "{}{}",
                        verification.diagnostic,
                        verification
                            .file_name
                            .as_deref()
                            .map(|name| format!(" ({name})"))
                            .unwrap_or_default()
                    ),
                );
                match verification.state {
                    VerificationState::Valid => {
                        next.phase = Phase::Completed;
                    }
                    VerificationState::Waiting
                    | VerificationState::Invalid
                    | VerificationState::Unavailable => {}
                }
            }
            Action::Report { message } => {
                if message.len() > 8_192 {
                    return Err(fail("diagnostic_too_long"));
                }
                if matches!(next.phase, Phase::Completed | Phase::BlockedByPolicy) {
                    return Err(fail("invalid_transition"));
                }
                let code = classify_error(&message);
                next.diagnostic = code.into(); // Never persist or log the source text.
                if is_policy_error(code) {
                    next.resume_phase = Some(next.phase);
                    next.phase = Phase::BlockedByPolicy;
                }
            }
            Action::Retry {} => {
                if next.phase == Phase::BlockedByPolicy {
                    next.phase = next
                        .resume_phase
                        .take()
                        .ok_or_else(|| fail("invalid_transition"))?;
                }
                next.diagnostic = "portal_required".into();
            }
            Action::Reset {} => {
                next = Session::default();
            }
        }
        self.persist(&next)?;
        *guard = next;
        drop(guard);
        self.snapshot()
    }

    pub fn open_portal(&self) -> Result<()> {
        // Only the documented portal home. Microsoft owns navigation, MFA and consent.
        open::that(PORTAL).map_err(|_| fail("browser_unavailable"))
    }

    pub fn show_package(&self) -> Result<()> {
        if !self.package_path().is_file() {
            return Err(fail("package_missing"));
        }
        open::that(&self.directory).map_err(|_| fail("folder_unavailable"))
    }

    pub fn open_inbox(&self) -> Result<()> {
        let folder = self
            .session
            .lock()
            .map_err(|_| fail("installer_busy"))?
            .folder
            .clone()
            .ok_or_else(|| fail("inbox_unavailable"))?;
        if !Path::new(&folder).is_dir() {
            return Err(fail("inbox_unavailable"));
        }
        open::that(folder).map_err(|_| fail("folder_unavailable"))
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Prepare {
        one_drive_root: String,
        calendar_name: String,
    },
    ConfirmSignIn {},
    ConfirmEnvironment {
        environment_id: String,
    },
    ConfirmConnections {},
    ConfirmImport {},
    ConfirmActive {},
    Verify {},
    Report {
        message: String,
    },
    Retry {},
    Reset {},
}

fn advance(session: &mut Session, from: Phase, to: Phase) -> Result<()> {
    if session.phase != from {
        return Err(fail("invalid_transition"));
    }
    session.phase = to;
    session.diagnostic = "user_reported_not_remotely_verified".into();
    Ok(())
}

fn detect_pac(path: Option<&std::ffi::OsStr>) -> bool {
    path.is_some_and(|path| {
        std::env::split_paths(path)
            .filter(|p| p.is_absolute())
            .any(|p| p.join("pac.exe").is_file())
    })
}

fn replace_file(staging: &Path, target: &Path) -> std::io::Result<()> {
    if !target.exists() {
        return fs::rename(staging, target);
    }
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("installer-file");
    let backup = target.with_file_name(format!("{file_name}.previous-{}", uuid::Uuid::new_v4()));
    fs::rename(target, &backup)?;
    match fs::rename(staging, target) {
        Ok(()) => {
            let _ = fs::remove_file(backup);
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(backup, target);
            Err(error)
        }
    }
}

fn parse_environment(input: &str) -> Option<String> {
    let input = input.trim();
    let owned;
    let id = if input.starts_with("https://") {
        let url = url::Url::parse(input).ok()?;
        if url.host_str()? != "make.powerautomate.com"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return None;
        }
        let segments: Vec<_> = url.path_segments()?.collect();
        let position = segments.iter().position(|s| *s == "environments")?;
        owned = segments.get(position + 1)?.to_string();
        owned.as_str()
    } else {
        input
    };
    let guid = id.strip_prefix("Default-").unwrap_or(id);
    let parsed = uuid::Uuid::parse_str(guid).ok()?;
    if parsed.is_nil() {
        return None;
    }
    Some(if id.starts_with("Default-") {
        format!("Default-{parsed}")
    } else {
        parsed.to_string()
    })
}

fn prepare_inbox(root: &Path) -> Result<PathBuf> {
    if !root.is_absolute() || !root.is_dir() {
        return Err(fail("choose_onedrive_root"));
    }
    let root = fs::canonicalize(root).map_err(|_| fail("choose_onedrive_root"))?;
    if root.parent().is_none() {
        return Err(fail("choose_onedrive_root"));
    }
    for variable in [
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
    ] {
        if let Some(protected) = std::env::var_os(variable).and_then(|p| fs::canonicalize(p).ok()) {
            if root.starts_with(protected) {
                return Err(fail("protected_location"));
            }
        }
    }
    // Do not follow a junction or symlink out of the user-selected sync root.
    let mut folder = root.clone();
    for part in ["AtlasBridge", "inbox"] {
        folder.push(part);
        if folder.exists() {
            let canonical = fs::canonicalize(&folder).map_err(|_| fail("inbox_unavailable"))?;
            if !canonical.starts_with(&root) || !canonical.is_dir() {
                return Err(fail("unsafe_inbox_link"));
            }
        } else {
            fs::create_dir(&folder).map_err(|_| fail("inbox_not_writable"))?;
        }
    }
    for path in [
        folder.join("scheduled"),
        folder.join("teams"),
        folder.join("requested"),
        root.join("AtlasBridge").join("requests"),
    ] {
        if path.exists() {
            let canonical = fs::canonicalize(&path).map_err(|_| fail("inbox_unavailable"))?;
            if !canonical.starts_with(&root) || !canonical.is_dir() {
                return Err(fail("unsafe_inbox_link"));
            }
        } else {
            fs::create_dir(&path).map_err(|_| fail("inbox_not_writable"))?;
        }
    }
    let request = root
        .join("AtlasBridge")
        .join("requests")
        .join("selected-date.txt");
    if !request.exists() {
        fs::write(&request, "").map_err(|_| fail("inbox_not_writable"))?;
    }
    Ok(folder)
}

fn personalized_solution(session: &Session) -> Result<Vec<u8>> {
    let mut source =
        ZipArchive::new(Cursor::new(SOLUTION)).map_err(|_| fail("invalid_bundled_solution"))?;
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    let mut changed = 0usize;
    for index in 0..source.len() {
        let mut entry = source
            .by_index(index)
            .map_err(|_| fail("invalid_bundled_solution"))?;
        let name = entry.name().to_string();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|_| fail("invalid_bundled_solution"))?;
        if WORKFLOWS.contains(&name.as_str()) {
            let mut flow: Value =
                serde_json::from_slice(&bytes).map_err(|_| fail("invalid_bundled_solution"))?;
            let actions = &mut flow["properties"]["definition"]["actions"];
            actions["AtlasInstallationId"]["inputs"] =
                Value::String(session.installation_id.clone());
            bytes = serde_json::to_vec_pretty(&flow)?;
            changed += 1;
        }
        output
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(|_| fail("package_generation_failed"))?;
        output
            .write_all(&bytes)
            .map_err(|_| fail("package_generation_failed"))?;
    }
    if changed != WORKFLOWS.len() {
        return Err(fail("invalid_bundled_solution"));
    }
    Ok(output
        .finish()
        .map_err(|_| fail("package_generation_failed"))?
        .into_inner())
}

#[derive(Debug, PartialEq)]
enum VerificationState {
    Valid,
    Waiting,
    Invalid,
    Unavailable,
}

#[derive(Debug, PartialEq)]
struct Verification {
    state: VerificationState,
    diagnostic: &'static str,
    file_name: Option<String>,
}

fn verification(
    state: VerificationState,
    diagnostic: &'static str,
    file_name: Option<String>,
) -> Verification {
    Verification {
        state,
        diagnostic,
        file_name,
    }
}

fn verification_files(folder: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![(folder.to_path_buf(), 0usize)];
    while let Some((directory, depth)) = pending.pop() {
        for entry in fs::read_dir(directory)?.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() && depth < 2 {
                pending.push((entry.path(), depth + 1));
            } else if kind.is_file() {
                files.push(entry.path());
                if files.len() >= 10_000 {
                    return Ok(files);
                }
            }
        }
    }
    Ok(files)
}

fn verify_inbox(session: &Session) -> Verification {
    let Some(folder) = &session.folder else {
        return verification(VerificationState::Unavailable, "inbox_unavailable", None);
    };
    let Ok(entries) = verification_files(Path::new(folder)) else {
        return verification(VerificationState::Unavailable, "inbox_unavailable", None);
    };
    let current_prefix = format!("atlas-evidence-{}-", session.installation_id);
    let mut saw_invalid = false;
    let mut saw_placeholder = false;
    let mut saw_stale = false;
    let mut saw_future = false;
    let mut saw_previous_installation = false;
    let mut saw_event_only = false;
    let mut saw_incomplete_sources = false;
    let mut checked_file = None;
    let now = Utc::now();
    // Bound directory traversal and each read. Verification never sends evidence to logs/UI.
    for path in entries {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if !name.ends_with(".json") {
            continue;
        }
        if !is_atlas_evidence_name(&name) {
            continue;
        }
        let current_installation = name.starts_with(&current_prefix);
        if !current_installation {
            // A package from an older personalized flow cannot prove that the
            // ZIP prepared by this setup was imported. Accepting it allowed an
            // outdated collector to pass verification after an Atlas upgrade.
            saw_previous_installation = true;
            continue;
        }
        checked_file = Some(name);
        let Ok(metadata) = fs::metadata(&path) else {
            saw_invalid = true;
            continue;
        };
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            // Do not block on hydration of OneDrive placeholders.
            if metadata.file_attributes() & (0x1000 | 0x40000 | 0x400000) != 0 {
                saw_placeholder = true;
                continue;
            }
        }
        if metadata.len() > MAX_FILE {
            saw_invalid = true;
            continue;
        }
        let Ok(file) = fs::File::open(&path) else {
            saw_invalid = true;
            continue;
        };
        let Ok(metadata) = file.metadata() else {
            saw_invalid = true;
            continue;
        };
        if metadata.len() > MAX_FILE {
            saw_invalid = true;
            continue;
        }
        let mut bytes = Vec::new();
        if file.take(MAX_FILE + 1).read_to_end(&mut bytes).is_err() {
            saw_invalid = true;
            continue;
        }
        if bytes.len() as u64 > MAX_FILE {
            saw_invalid = true;
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            saw_invalid = true;
            continue;
        };
        if !evidence_validation::valid(&value) {
            saw_invalid = true;
            continue;
        }
        let Ok(exported) = DateTime::parse_from_rfc3339(value["exportedAt"].as_str().unwrap_or(""))
        else {
            saw_invalid = true;
            continue;
        };
        if exported > now + chrono::Duration::minutes(5) {
            saw_future = true;
            continue;
        }
        // A restart must not invalidate a package produced by this same personalized flow.
        // Keep the readiness proof recent while allowing OneDrive and setup delays.
        if exported < now - chrono::Duration::hours(48) {
            saw_stale = true;
            continue;
        }
        let output_folder = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if output_folder.eq_ignore_ascii_case("teams") {
            // The event flow proves only the Teams connection. Setup is ready
            // after the scheduled collector has exercised all three sources.
            saw_event_only = true;
            continue;
        }
        let sources = &value["sources"];
        if !sources["calendar"].as_bool().unwrap_or(false)
            || !sources["mail"].as_bool().unwrap_or(false)
            || !sources["teams"].as_bool().unwrap_or(false)
        {
            saw_incomplete_sources = true;
            continue;
        }
        return verification(VerificationState::Valid, "file_verified", checked_file);
    }
    if saw_placeholder {
        verification(
            VerificationState::Waiting,
            "evidence_not_downloaded",
            checked_file,
        )
    } else if saw_future {
        verification(
            VerificationState::Invalid,
            "evidence_from_future",
            checked_file,
        )
    } else if saw_stale {
        verification(VerificationState::Waiting, "evidence_too_old", checked_file)
    } else if saw_invalid {
        verification(VerificationState::Invalid, "invalid_evidence", checked_file)
    } else if saw_incomplete_sources {
        verification(
            VerificationState::Invalid,
            "collector_sources_incomplete",
            checked_file,
        )
    } else if saw_event_only {
        verification(
            VerificationState::Waiting,
            "collector_file_pending",
            checked_file,
        )
    } else if saw_previous_installation {
        verification(
            VerificationState::Waiting,
            "previous_installation_detected",
            None,
        )
    } else {
        verification(VerificationState::Waiting, "waiting_for_sync", None)
    }
}

fn is_atlas_evidence_name(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix("atlas-evidence-")
        .and_then(|value| value.strip_suffix(".json"))
    else {
        return false;
    };
    let Some(id) = rest.get(..36) else {
        return false;
    };
    uuid::Uuid::parse_str(id).is_ok() && rest.as_bytes().get(36) == Some(&b'-')
}

fn is_policy_error(code: &str) -> bool {
    code.starts_with("missing_prv")
        || matches!(
            code,
            "environment_maker_required"
                | "dataverse_required"
                | "connector_policy"
                | "outlook_access"
                | "teams_access"
                | "onedrive_access"
                | "admin_consent_required"
                | "conditional_access"
                | "license_required"
                | "unmanaged_blocked"
        )
}

fn classify_error(message: &str) -> &'static str {
    let m = message.to_lowercase();
    // Specific documented identifiers before broad text. No guessing privileges from a 403.
    if m.contains("aadsts53003") {
        "conditional_access"
    } else if m.contains("aadsts65001")
        || m.contains("aadsts90094")
        || m.contains("aprobación del administrador")
        || m.contains("need admin approval")
    {
        "admin_consent_required"
    } else if m.contains("environment maker") || m.contains("environmentmaker") {
        "environment_maker_required"
    } else if m.contains("prvimportcustomizations") {
        "missing_prvImportCustomizations"
    } else if m.contains("prvcreateworkflow") {
        "missing_prvCreateWorkflow"
    } else if m.contains("prvcreateconnectionreference") {
        "missing_prvCreateConnectionReference"
    } else if m.contains("prvreadsolution") {
        "missing_prvReadSolution"
    } else if m.contains("prvcreatesolution") {
        "missing_prvCreateSolution"
    } else if m.contains("dataverse_required")
        || m.contains("no dataverse database")
        || m.contains("dataverse database is not")
        || m.contains("sin base de datos de dataverse")
    {
        "dataverse_required"
    } else if m.contains("unmanaged") && (m.contains("block") || m.contains("not allowed")) {
        "unmanaged_blocked"
    } else if m.contains("dlp")
        || m.contains("datapolicyviolation")
        || m.contains("data loss prevention")
    {
        "connector_policy"
    } else if m.contains("license_required")
        || m.contains("license is required")
        || m.contains("licencia requerida")
    {
        "license_required"
    } else if m.contains("outlook_access") {
        "outlook_access"
    } else if m.contains("teams_access") {
        "teams_access"
    } else if m.contains("onedrive_access") {
        "onedrive_access"
    } else if m.contains("atlascalendarnotunique") {
        "calendar_not_unique"
    } else if m.contains("429") || m.contains("throttl") {
        "throttled"
    } else if m.contains("timeout") || m.contains("timed out") {
        "timeout"
    } else {
        "unclassified_portal_error"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tools_are_optional_and_never_executed() {
        assert!(!detect_pac(None));
        let dir = tempfile::tempdir().unwrap();
        let path = std::env::join_paths([dir.path()]).unwrap();
        assert!(!detect_pac(Some(&path)));
        fs::write(dir.path().join("pac.exe"), "not an executable").unwrap();
        assert!(detect_pac(Some(&path)));
        assert!(!detect_pac(Some(std::ffi::OsStr::new("."))));
    }

    #[test]
    fn parses_only_environment_identifiers_and_official_portal_urls() {
        let guid = "11111111-1111-4111-8111-111111111111";
        assert_eq!(parse_environment(guid).as_deref(), Some(guid));
        assert_eq!(parse_environment(&format!("https://make.powerautomate.com/environments/Default-{guid}/solutions?untrusted=ignored")), Some(format!("Default-{guid}")));
        for invalid in [
            "",
            "not-an-id",
            "00000000-0000-0000-0000-000000000000",
            "https://make.powerautomate.com.evil.test/environments/test",
            "https://user@make.powerautomate.com/environments/test",
            "https://make.powerautomate.com/",
            "http://make.powerautomate.com/",
        ] {
            assert!(parse_environment(invalid).is_none());
        }
    }

    #[test]
    fn parses_commands_strictly_and_rejects_unknown_auth_fields() {
        assert!(serde_json::from_value::<Action>(json!({"kind":"confirm_sign_in"})).is_ok());
        assert!(serde_json::from_value::<Action>(
            json!({"kind":"confirm_sign_in", "accessToken":"secret"})
        )
        .is_err());
        assert!(serde_json::from_value::<Action>(json!({"kind":"silently_import"})).is_err());
    }

    #[test]
    fn policy_errors_are_specific_and_unknown_403_is_not_a_role_diagnosis() {
        for (text, code) in [
            (
                "User missing prvImportCustomizations",
                "missing_prvImportCustomizations",
            ),
            ("prvCreateWorkflow", "missing_prvCreateWorkflow"),
            ("Environment Maker", "environment_maker_required"),
            ("no Dataverse database", "dataverse_required"),
            ("DlpViolation", "connector_policy"),
            ("AADSTS53003", "conditional_access"),
            (
                "Se necesita aprobación del administrador",
                "admin_consent_required",
            ),
            ("unmanaged customizations blocked", "unmanaged_blocked"),
            ("teams_access", "teams_access"),
            ("license_required", "license_required"),
        ] {
            assert_eq!(classify_error(text), code);
            assert!(is_policy_error(code));
        }
        assert_eq!(
            classify_error("403 Forbidden Authorization: Bearer private-data"),
            "unclassified_portal_error"
        );
        assert!(!is_policy_error(classify_error("403 Forbidden")));
        assert_eq!(classify_error("429 Too Many Requests"), "throttled");
        assert_eq!(classify_error("request timed out"), "timeout");
    }

    #[test]
    fn folder_preparation_is_idempotent_and_keeps_existing_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let folder = prepare_inbox(dir.path()).unwrap();
        for child in ["scheduled", "teams", "requested"] {
            assert!(folder.join(child).is_dir());
        }
        assert!(dir
            .path()
            .join("AtlasBridge")
            .join("requests")
            .join("selected-date.txt")
            .is_file());
        fs::write(folder.join("existing.json"), "keep me").unwrap();
        assert_eq!(prepare_inbox(dir.path()).unwrap(), folder);
        assert_eq!(
            fs::read_to_string(folder.join("existing.json")).unwrap(),
            "keep me"
        );
        assert!(prepare_inbox(Path::new("relative")).is_err());
        let blocked = tempfile::tempdir().unwrap();
        fs::write(blocked.path().join("AtlasBridge"), "a file").unwrap();
        assert!(prepare_inbox(blocked.path()).is_err());
    }

    #[test]
    fn local_file_replacement_preserves_new_content_and_cleans_backup() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("session.json");
        let staging = dir.path().join("session.pending.json");
        fs::write(&target, "old").unwrap();
        fs::write(&staging, "new").unwrap();
        replace_file(&staging, &target).unwrap();
        assert_eq!(fs::read_to_string(target).unwrap(), "new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    fn flow_from(bytes: Vec<u8>, workflow: &str) -> Value {
        let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(zip.by_name("[Content_Types].xml").is_ok());
        assert!(zip.by_name("solution.xml").is_ok());
        assert!(zip.by_name("customizations.xml").is_ok());
        let mut flow = String::new();
        zip.by_name(workflow)
            .unwrap()
            .read_to_string(&mut flow)
            .unwrap();
        serde_json::from_str(&flow).unwrap()
    }

    #[test]
    fn bundled_solution_version_matches_installer_contract() {
        let mut zip = ZipArchive::new(Cursor::new(SOLUTION)).unwrap();
        let mut solution = String::new();
        zip.by_name("solution.xml")
            .unwrap()
            .read_to_string(&mut solution)
            .unwrap();
        assert!(solution.contains(&format!("<Version>{SOLUTION_VERSION}</Version>")));
    }

    #[test]
    fn package_preserves_connection_references_and_personalizes_installation() {
        let session = Session::default();
        let bytes = personalized_solution(&session).unwrap();
        let flow = flow_from(bytes.clone(), SCHEDULED_WORKFLOW);
        let actions = &flow["properties"]["definition"]["actions"];
        assert!(actions.get("AtlasCalendarName").is_none());
        assert_eq!(
            actions["AtlasInstallationId"]["inputs"],
            session.installation_id
        );
        let refs = flow["properties"]["connectionReferences"]
            .as_object()
            .unwrap();
        assert_eq!(refs.len(), 3);
        for reference in refs.values() {
            assert!(reference["connection"]["connectionReferenceLogicalName"]
                .as_str()
                .unwrap()
                .starts_with("atlas_"));
            assert!(reference.get("connectionName").is_none());
            assert!(reference["connection"].get("id").is_none());
        }
        assert!(flow["properties"]["definition"].get("metadata").is_none());
        assert_eq!(
            actions["Compose_Atlas_bundle"]["runAfter"]["Teams_evidence"],
            json!(["Succeeded", "Failed", "Skipped", "TimedOut"])
        );
        assert_eq!(
            actions["Read_Atlas_requested_date"]["inputs"]["host"]["operationId"],
            "GetFileContentByPath"
        );
        assert_eq!(
            actions["RequestedDate"]["runAfter"]["Read_Atlas_requested_date"],
            json!(["Succeeded", "Failed", "Skipped", "TimedOut"])
        );
        assert!(actions["RequestedDate"]["inputs"]
            .as_str()
            .unwrap()
            .contains("coalesce"));
        assert!(actions["TargetDate"]["inputs"]
            .as_str()
            .unwrap()
            .contains("RequestedDate"));
        let event_flow = flow_from(bytes, TEAMS_EVENT_WORKFLOW);
        let event_actions = &event_flow["properties"]["definition"]["actions"];
        assert_eq!(
            event_actions["AtlasInstallationId"]["inputs"],
            session.installation_id
        );
        assert_eq!(
            event_flow["properties"]["definition"]["triggers"]["When_a_new_chat_message_is_added"]
                ["inputs"]["host"]["operationId"],
            "WebhookChatMessageTrigger"
        );
    }

    #[test]
    fn all_stages_require_confirmation_and_retries_preserve_identity() {
        let dir = tempfile::tempdir().unwrap();
        let sync = tempfile::tempdir().unwrap();
        let installer = Installer::new(dir.path().to_path_buf());
        assert!(installer.action(Action::ConfirmImport {}).is_err());
        let prepared = installer
            .action(Action::Prepare {
                one_drive_root: sync.path().to_string_lossy().into(),
                calendar_name: "Calendar".into(),
            })
            .unwrap();
        let id = prepared.session.installation_id.clone();
        assert_eq!(prepared.session.phase, Phase::WaitingSignIn);
        assert!(installer.action(Action::Verify {}).is_err());
        installer.action(Action::ConfirmSignIn {}).unwrap();
        let blocked = installer
            .action(Action::Report {
                message: "DLP Authorization: Bearer do-not-store alice@example.com".into(),
            })
            .unwrap();
        assert_eq!(blocked.session.phase, Phase::BlockedByPolicy);
        let persisted = fs::read_to_string(dir.path().join("session.json")).unwrap();
        assert!(!persisted.contains("do-not-store"));
        assert!(!persisted.contains("alice@example.com"));
        let resumed = Installer::new(dir.path().to_path_buf());
        let retry = resumed.action(Action::Retry {}).unwrap();
        assert_eq!(retry.session.phase, Phase::FindingConnections);
        assert_eq!(retry.session.installation_id, id);
        resumed.action(Action::ConfirmConnections {}).unwrap();
        resumed.action(Action::ConfirmImport {}).unwrap();
        let waiting = resumed.action(Action::Verify {}).unwrap();
        assert_eq!(waiting.session.phase, Phase::VerifyingFile);
        assert_eq!(waiting.session.diagnostic, "waiting_for_sync");
        let mut bundle: Value = serde_json::from_str(include_str!(
            "../../power-automate/atlas-evidence.example.json"
        ))
        .unwrap();
        bundle["exportedAt"] = json!(Utc::now().to_rfc3339());
        let scheduled = Path::new(waiting.session.folder.as_ref().unwrap()).join("scheduled");
        fs::write(
            scheduled.join(format!("atlas-evidence-{id}-test.json")),
            serde_json::to_vec(&bundle).unwrap(),
        )
        .unwrap();
        assert_eq!(
            resumed.action(Action::Verify {}).unwrap().session.phase,
            Phase::Completed
        );
        assert!(resumed.action(Action::ConfirmImport {}).is_err());
        assert_ne!(
            resumed
                .action(Action::Reset {})
                .unwrap()
                .session
                .installation_id,
            id
        );
    }

    #[test]
    fn verification_explains_wrong_installation_stale_future_and_invalid_bundles() {
        let dir = tempfile::tempdir().unwrap();
        let session = Session {
            folder: Some(dir.path().to_string_lossy().into()),
            ..Session::default()
        };
        let mut bundle: Value = serde_json::from_str(include_str!(
            "../../power-automate/atlas-evidence.example.json"
        ))
        .unwrap();
        fs::write(
            dir.path().join("atlas-evidence.example.json"),
            serde_json::to_vec(&bundle).unwrap(),
        )
        .unwrap();
        let unrelated = verify_inbox(&session);
        assert_eq!(unrelated.state, VerificationState::Waiting);
        assert_eq!(unrelated.diagnostic, "waiting_for_sync");
        let scheduled = dir.path().join("scheduled");
        fs::create_dir(&scheduled).unwrap();
        let target = scheduled.join(format!(
            "atlas-evidence-{}-test.json",
            session.installation_id
        ));
        fs::write(&target, "{broken").unwrap();
        let invalid = verify_inbox(&session);
        assert_eq!(invalid.state, VerificationState::Invalid);
        assert_eq!(invalid.diagnostic, "invalid_evidence");

        bundle["exportedAt"] = json!((Utc::now() - chrono::Duration::days(3)).to_rfc3339());
        fs::write(&target, serde_json::to_vec(&bundle).unwrap()).unwrap();
        let stale = verify_inbox(&session);
        assert_eq!(stale.state, VerificationState::Waiting);
        assert_eq!(stale.diagnostic, "evidence_too_old");

        bundle["exportedAt"] = json!((Utc::now() + chrono::Duration::days(1)).to_rfc3339());
        fs::write(&target, serde_json::to_vec(&bundle).unwrap()).unwrap();
        let future = verify_inbox(&session);
        assert_eq!(future.state, VerificationState::Invalid);
        assert_eq!(future.diagnostic, "evidence_from_future");

        // Restarting the assistant does not invalidate a recent package from its flow.
        bundle["exportedAt"] = json!((Utc::now() - chrono::Duration::days(1)).to_rfc3339());
        fs::write(&target, serde_json::to_vec(&bundle).unwrap()).unwrap();
        assert_eq!(verify_inbox(&session).state, VerificationState::Valid);

        bundle["exportedAt"] = json!(Utc::now().to_rfc3339());
        fs::write(&target, serde_json::to_vec(&bundle).unwrap()).unwrap();
        assert_eq!(verify_inbox(&session).state, VerificationState::Valid);

        let other_id = uuid::Uuid::new_v4();
        let other = scheduled.join(format!("atlas-evidence-{other_id}-recent.json"));
        fs::rename(&target, &other).unwrap();
        let existing_flow = verify_inbox(&session);
        assert_eq!(existing_flow.state, VerificationState::Waiting);
        assert_eq!(existing_flow.diagnostic, "previous_installation_detected");

        let teams = dir.path().join("teams");
        fs::create_dir(&teams).unwrap();
        let event = teams.join(format!(
            "atlas-evidence-{}-teams-test.json",
            session.installation_id
        ));
        bundle["sources"] = json!({ "calendar": false, "mail": false, "teams": true });
        fs::write(&event, serde_json::to_vec(&bundle).unwrap()).unwrap();
        let event_only = verify_inbox(&session);
        assert_eq!(event_only.state, VerificationState::Waiting);
        assert_eq!(event_only.diagnostic, "collector_file_pending");

        let current = scheduled.join(format!(
            "atlas-evidence-{}-partial.json",
            session.installation_id
        ));
        fs::write(&current, serde_json::to_vec(&bundle).unwrap()).unwrap();
        let incomplete = verify_inbox(&session);
        assert_eq!(incomplete.state, VerificationState::Invalid);
        assert_eq!(incomplete.diagnostic, "collector_sources_incomplete");
    }
}
