//! Portal-assisted installation. This module never obtains Microsoft credentials,
//! invokes PAC, controls a browser, or calls a tenant API.
//!
//! Each Atlas role ships its own solution template (tracker: calendar, mail
//! and Teams evidence; QA: manager mailbox export and analyst watcher). The
//! template is personalized per installation before the user imports it, so
//! every person gets an independent solution named after them.
use crate::{
    diagnostics,
    error::{AppError, Result},
    evidence_validation, qa_engine,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

const SOLUTION: &[u8] = include_bytes!("../../power-automate/AtlasBridge_1_0_0_0.zip");
const WRITER_SOLUTION: &[u8] =
    include_bytes!("../../power-automate-writer/AtlasTrackerWriter_1_0_0_0.zip");
const QA_SOLUTION: &[u8] = include_bytes!("../../power-automate-qa/AtlasQA_1_0_0_0.zip");
const SCHEDULED_WORKFLOW: &str =
    "Workflows/AtlasExportEvidence-8e5c1f84-dcbb-4a2c-9d2f-62e9c38105d2.json";
const TEAMS_EVENT_WORKFLOW: &str =
    "Workflows/AtlasCaptureTeamsMessages-7f7115e8-b821-4bb9-9b4e-5c7a5448610e.json";
const MAX_FILE: u64 = 25 * 1024 * 1024;
const PORTAL: &str = "https://make.powerautomate.com/";
const SOLUTION_VERSION: &str = "1.14.0.0";
const QA_SOLUTION_VERSION: &str = "1.0.1.0";
const MAX_OWNER_CHARS: usize = 80;
const MAX_OWNER_SLUG: usize = 20;
/// Dataverse rejects solutions whose UniqueName has 50 or more characters.
const MAX_UNIQUE_NAME: usize = 49;
/// Characters of the installation key kept in owner-named unique names:
/// 48 random bits, unique per installation while leaving room for the name.
const UNIQUE_NAME_KEY_CHARS: usize = 12;
/// Dataverse process (flow) names hold at most 100 characters.
const MAX_WORKFLOW_NAME: usize = 100;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SolutionKind {
    Tracker,
    Qa,
    Writer,
}

pub struct TemplateWorkflow {
    file: &'static str,
    id: &'static str,
    name: &'static str,
}

/// One bundled solution template and the identities it declares.
pub struct Template {
    kind: SolutionKind,
    bytes: &'static [u8],
    version: &'static str,
    base_unique_name: &'static str,
    /// Display name written in the template manifest.
    manifest_display_name: &'static str,
    legacy_display_name: &'static str,
    display_prefix: &'static str,
    package_file: &'static str,
    workflows: &'static [TemplateWorkflow],
    connection_references: &'static [&'static str],
    connectors: &'static [&'static str],
    /// Whether every flow carries an AtlasInstallationId compose action.
    installation_action: bool,
}

pub static TRACKER: Template = Template {
    kind: SolutionKind::Tracker,
    bytes: SOLUTION,
    version: SOLUTION_VERSION,
    base_unique_name: "AtlasBridge",
    manifest_display_name: "AtlasBridge",
    legacy_display_name: "Atlas Bridge",
    display_prefix: "Atlas Tracker",
    package_file: "AtlasBridge_1_0_0_0.zip",
    workflows: &[
        TemplateWorkflow {
            file: SCHEDULED_WORKFLOW,
            id: SCHEDULED_WORKFLOW_ID,
            name: "Atlas - Export evidence to OneDrive",
        },
        TemplateWorkflow {
            file: TEAMS_EVENT_WORKFLOW,
            id: TEAMS_EVENT_WORKFLOW_ID,
            name: "Atlas - Capture Teams messages",
        },
    ],
    connection_references: &CONNECTION_REFERENCES,
    connectors: &[
        "shared_office365",
        "shared_teams",
        "shared_onedriveforbusiness",
    ],
    installation_action: true,
};

pub static QA: Template = Template {
    kind: SolutionKind::Qa,
    bytes: QA_SOLUTION,
    version: QA_SOLUTION_VERSION,
    base_unique_name: "AtlasQA",
    manifest_display_name: "AtlasQA",
    legacy_display_name: "Atlas QA",
    display_prefix: "Atlas QA",
    package_file: "AtlasQA_1_0_0_0.zip",
    workflows: &[
        TemplateWorkflow {
            file: "Workflows/AtlasQaExportMail-3b9d2e61-5c47-4f0a-9e2b-7a41c6d8f213.json",
            id: "3b9d2e61-5c47-4f0a-9e2b-7a41c6d8f213",
            name: "Atlas QA - Export mailbox evidence",
        },
        TemplateWorkflow {
            file: "Workflows/AtlasQaWatchMail-c5a07f9e-2d18-4b63-8f4e-91e0b3d6a574.json",
            id: "c5a07f9e-2d18-4b63-8f4e-91e0b3d6a574",
            name: "Atlas QA - Watch analyst mail",
        },
    ],
    connection_references: &["atlas_qa_office365", "atlas_qa_onedriveforbusiness"],
    connectors: &["shared_office365", "shared_onedriveforbusiness"],
    installation_action: true,
};

/// Optional second tracker solution that writes a SharePoint workbook.
pub static WRITER: Template = Template {
    kind: SolutionKind::Writer,
    bytes: WRITER_SOLUTION,
    version: "1.0.0.0",
    base_unique_name: "AtlasTrackerWriter",
    manifest_display_name: "Atlas Tracker Writer",
    legacy_display_name: "Atlas Tracker Writer",
    display_prefix: "Atlas Tracker Writer",
    package_file: "AtlasTrackerWriter_1_0_0_0.zip",
    workflows: &[TemplateWorkflow {
        file: "Workflows/AtlasWriteDailyTracker-4d6c39b7-cac8-4d19-a12e-95af49503b7f.json",
        id: "4d6c39b7-cac8-4d19-a12e-95af49503b7f",
        name: "Atlas - Write daily tracker to SharePoint",
    }],
    connection_references: &[
        "atlas_writer_onedrive",
        "atlas_writer_sharepoint",
        "atlas_writer_excel",
    ],
    connectors: &[
        "shared_onedriveforbusiness",
        "shared_sharepointonline",
        "shared_excelonlinebusiness",
    ],
    installation_action: false,
};

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
    /// Person the personalized solution is named after. Absent for
    /// installations prepared by older releases, which keep their names.
    #[serde(default)]
    pub owner_name: Option<String>,
    /// Name of the solution the user must import (shown in the wizard).
    #[serde(default)]
    pub solution_display_name: Option<String>,
    /// QA only: request the flow must answer to prove the installation.
    #[serde(default)]
    pub ping_request_id: Option<String>,
    /// A newer solution version was prepared for a completed installation;
    /// the user re-imports the ZIP to update the same solution.
    #[serde(default)]
    pub update_available: bool,
}

impl Session {
    fn new(template: &Template) -> Self {
        Self {
            phase: Phase::CheckingRequirements,
            resume_phase: None,
            installation_id: uuid::Uuid::new_v4().to_string(),
            started_at: Utc::now().to_rfc3339(),
            folder: None,
            calendar_name: "Calendar".into(),
            environment_id: None,
            diagnostic: "portal_required".into(),
            solution_version: Some(template.version.into()),
            last_checked_at: None,
            last_checked_file: None,
            owner_name: None,
            solution_display_name: None,
            ping_request_id: None,
            update_available: false,
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new(&TRACKER)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub session: Session,
    pub kind: SolutionKind,
    pub package_path: String,
    pub one_drive_root: Option<String>,
    pub pac_detected: bool,
}

pub struct Installer {
    directory: PathBuf,
    template: &'static Template,
    session: Mutex<Session>,
}

fn fail(code: &str) -> AppError {
    AppError::Message(format!("Atlas connector: {code}"))
}

impl Installer {
    pub fn new(directory: PathBuf, template: &'static Template) -> Self {
        let saved = load_saved_session(&directory, template);
        let (session, migrated) = match saved {
            Some(mut session) if session.solution_version.as_deref() != Some(template.version) => {
                migrate_session(&mut session, template);
                (session, true)
            }
            Some(session) => (session, false),
            None => (Session::new(template), false),
        };
        let installer = Self {
            directory,
            template,
            session: Mutex::new(session),
        };
        if migrated {
            if let Ok(session) = installer.session.lock().map(|s| s.clone()) {
                let _ = installer.persist(&session);
            }
        }
        installer.refresh_package();
        installer
    }

    /// Rebuilds the personalized ZIP of an already prepared installation so
    /// naming fixes in a new Atlas version reach it without a new setup.
    /// The installation id and owner stay the same, so re-importing still
    /// updates the same solution.
    fn refresh_package(&self) {
        let Ok(mut session) = self.session.lock() else {
            return;
        };
        if session.folder.is_none() {
            return;
        }
        let refreshed = instance_identity(
            self.template,
            &session.installation_id,
            session.owner_name.as_deref(),
        )
        .and_then(|identity| {
            let bytes = personalized_solution(self.template, &session)?;
            Ok((identity.display_name, bytes))
        });
        let (display_name, bytes) = match refreshed {
            Ok(value) => value,
            Err(error) => {
                diagnostics::error("connector/package", &error.to_string());
                return;
            }
        };
        let current = fs::read(self.package_path()).ok();
        if current.as_deref() != Some(bytes.as_slice()) {
            let staging = self.directory.join("solution.pending.zip");
            let written = fs::create_dir_all(&self.directory)
                .and_then(|_| fs::write(&staging, &bytes))
                .and_then(|_| replace_file(&staging, &self.package_path()));
            match written {
                Ok(()) => diagnostics::info(
                    "connector/package",
                    &format!("Refreshed the personalized {:?} package", self.template.kind),
                ),
                Err(error) => {
                    diagnostics::error("connector/package", &error.to_string());
                    return;
                }
            }
        }
        if session.solution_display_name.as_deref() != Some(display_name.as_str()) {
            session.solution_display_name = Some(display_name);
            let snapshot = session.clone();
            drop(session);
            let _ = self.persist(&snapshot);
        }
    }

    fn package_path(&self) -> PathBuf {
        self.directory.join(self.template.package_file)
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
            kind: self.template.kind,
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
                owner_name,
            } => {
                if next.phase != Phase::CheckingRequirements {
                    return Err(fail("invalid_transition"));
                }
                if calendar_name.chars().count() > 128
                    || calendar_name.chars().any(char::is_control)
                {
                    return Err(fail("invalid_calendar_name"));
                }
                next.owner_name = normalize_owner(&owner_name)?;
                let folder = prepare_inbox(Path::new(&one_drive_root), self.template.kind)?;
                next.folder = Some(folder.to_string_lossy().into_owned());
                next.calendar_name = calendar_name.trim().to_string();
                fs::create_dir_all(&self.directory).map_err(|_| fail("settings_not_writable"))?;
                let identity = instance_identity(
                    self.template,
                    &next.installation_id,
                    next.owner_name.as_deref(),
                )?;
                let bytes = personalized_solution(self.template, &next)?;
                let staging = self.directory.join("solution.pending.zip");
                fs::write(&staging, bytes).map_err(|_| fail("package_not_writable"))?;
                replace_file(&staging, &self.package_path())
                    .map_err(|_| fail("package_not_writable"))?;
                if self.template.kind == SolutionKind::Qa {
                    let ping = qa_engine::ping_request(&next.installation_id);
                    qa_engine::write_request(&folder, &ping)
                        .map_err(|_| fail("inbox_not_writable"))?;
                    qa_engine::write_watch(&folder, &next.installation_id, &[])
                        .map_err(|_| fail("inbox_not_writable"))?;
                    next.ping_request_id = Some(ping.request_id);
                }
                next.solution_display_name = Some(identity.display_name);
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
                let verification = match self.template.kind {
                    SolutionKind::Qa => verify_qa(&mut next),
                    SolutionKind::Tracker | SolutionKind::Writer => verify_inbox(&next),
                };
                next.last_checked_at = Some(Utc::now().to_rfc3339());
                next.last_checked_file = verification.file_name.clone();
                // The wizard polls every 15 seconds; log only real changes.
                if next.diagnostic != verification.diagnostic {
                    diagnostics::info(
                        "connector/verify",
                        &format!(
                            "{:?}: {}{}",
                            self.template.kind,
                            verification.diagnostic,
                            verification
                                .file_name
                                .as_deref()
                                .map(|name| format!(" ({name})"))
                                .unwrap_or_default()
                        ),
                    );
                }
                next.diagnostic = verification.diagnostic.into();
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
            Action::AcknowledgeUpdate {} => {
                next.update_available = false;
            }
            Action::Reset {} => {
                next = Session::new(self.template);
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
        #[serde(default)]
        owner_name: String,
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
    /// The user imported the updated ZIP of a completed installation.
    AcknowledgeUpdate {},
}

/// Reads the saved setup. A file that cannot be used is kept aside and the
/// reason is logged, so a lost setup (which leads to a second solution in
/// Power Automate) can always be explained and recovered.
fn load_saved_session(directory: &Path, template: &Template) -> Option<Session> {
    let path = directory.join("session.json");
    let data = match fs::read(&path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if directory.exists() {
                diagnostics::info(
                    "connector/session",
                    &format!("{:?}: no saved setup in {}; starting a new one", template.kind, directory.display()),
                );
            }
            return None;
        }
        Err(error) => {
            diagnostics::error(
                "connector/session",
                &format!("{:?}: the saved setup could not be read: {error}", template.kind),
            );
            return None;
        }
    };
    let problem = if data.len() >= 16_384 {
        Some("the file is too large".to_string())
    } else {
        match serde_json::from_slice::<Session>(&data) {
            Ok(session)
                if uuid::Uuid::parse_str(&session.installation_id).is_ok()
                    && DateTime::parse_from_rfc3339(&session.started_at).is_ok() =>
            {
                return Some(session);
            }
            Ok(_) => Some("its installation id or start time is invalid".to_string()),
            Err(error) => Some(error.to_string()),
        }
    };
    let backup = directory.join(format!("session.unreadable-{}.json", uuid::Uuid::new_v4()));
    let kept = fs::copy(&path, &backup).is_ok();
    diagnostics::error(
        "connector/session",
        &format!(
            "{:?}: the saved setup was not usable ({}){}; starting a new one",
            template.kind,
            problem.unwrap_or_default(),
            if kept {
                format!(", a copy was kept at {}", backup.display())
            } else {
                String::new()
            }
        ),
    );
    None
}

/// A new Atlas release ships a newer solution version. The installation id,
/// owner and folder are kept so re-importing updates the same solution
/// instead of creating a second one next to the old flows.
fn migrate_session(session: &mut Session, template: &Template) {
    diagnostics::info(
        "connector/update",
        &format!(
            "{:?} solution {} -> {}: keeping installation {}",
            template.kind,
            session.solution_version.as_deref().unwrap_or("unknown"),
            template.version,
            session.installation_id
        ),
    );
    session.solution_version = Some(template.version.into());
    match session.phase {
        // Working installations keep working; Atlas asks for a re-import.
        Phase::Completed => session.update_available = true,
        // The old package may already be imported but never proved itself:
        // import the updated one, then verify again.
        Phase::ImportingSolution | Phase::ActivatingFlow | Phase::VerifyingFile => {
            session.phase = Phase::ImportingSolution;
            session.diagnostic = "solution_update_required".into();
        }
        _ => {}
    }
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

/// Drops the Windows verbatim prefix (`\\?\C:\...`) that canonicalize adds,
/// so stored and displayed folders stay readable. UNC paths are kept as-is.
pub fn readable_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path,
    }
}

fn prepare_inbox(root: &Path, kind: SolutionKind) -> Result<PathBuf> {
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
    let leaf = match kind {
        SolutionKind::Qa => "qa",
        SolutionKind::Tracker | SolutionKind::Writer => "inbox",
    };
    // Do not follow a junction or symlink out of the user-selected sync root.
    let mut folder = root.clone();
    for part in ["AtlasBridge", leaf] {
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
    if kind == SolutionKind::Qa {
        qa_engine::prepare_folders(&folder).map_err(|_| fail("inbox_not_writable"))?;
        for path in [
            qa_engine::requests_dir(&folder),
            qa_engine::inbox_dir(&folder),
        ] {
            let canonical = fs::canonicalize(&path).map_err(|_| fail("inbox_unavailable"))?;
            if !canonical.starts_with(&root) || !canonical.is_dir() {
                return Err(fail("unsafe_inbox_link"));
            }
        }
        return Ok(readable_path(folder));
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
    Ok(readable_path(folder))
}

// Identity of one logical installation. The same persistent installation id
// (and the owner name captured when it was prepared) always derives the same
// solution unique name, connection reference logical names and workflow ids,
// so Power Platform imports a new Atlas version as an update of that
// installation instead of colliding with components owned by another user of
// the same environment.
const CONNECTION_REFERENCES: [&str; 3] = [
    "atlas_office365",
    "atlas_teams",
    "atlas_onedriveforbusiness",
];
const SCHEDULED_WORKFLOW_ID: &str = "8e5c1f84-dcbb-4a2c-9d2f-62e9c38105d2";
const TEAMS_EVENT_WORKFLOW_ID: &str = "7f7115e8-b821-4bb9-9b4e-5c7a5448610e";
#[cfg(test)]
const WORKFLOW_IDS: [&str; 2] = [SCHEDULED_WORKFLOW_ID, TEAMS_EVENT_WORKFLOW_ID];

#[derive(Debug, PartialEq)]
struct InstanceIdentity {
    key: String,
    unique_name: String,
    display_name: String,
    owner: Option<String>,
    connection_references: Vec<String>,
    workflows: Vec<(String, String)>,
}

/// Deterministic Power Platform-safe key derived from the persistent
/// installation id: lowercase ASCII letters, digits and underscores only.
fn instance_key(installation_id: &str) -> Result<String> {
    let key: String = installation_id
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
        .take(32)
        .collect();
    if key.is_empty() {
        return Err(fail("invalid_installation_id"));
    }
    Ok(key)
}

fn normalize_owner(value: &str) -> Result<Option<String>> {
    let owner = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if owner.is_empty() {
        return Ok(None);
    }
    if owner.chars().count() > MAX_OWNER_CHARS
        || owner.chars().any(char::is_control)
        || owner_slug(&owner).is_empty()
    {
        return Err(fail("invalid_owner_name"));
    }
    Ok(Some(owner))
}

fn fold_accent(c: char) -> char {
    match c {
        'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => 'a',
        'Á' | 'À' | 'Ä' | 'Â' | 'Ã' | 'Å' => 'A',
        'é' | 'è' | 'ë' | 'ê' => 'e',
        'É' | 'È' | 'Ë' | 'Ê' => 'E',
        'í' | 'ì' | 'ï' | 'î' => 'i',
        'Í' | 'Ì' | 'Ï' | 'Î' => 'I',
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
        'Ó' | 'Ò' | 'Ö' | 'Ô' | 'Õ' => 'O',
        'ú' | 'ù' | 'ü' | 'û' => 'u',
        'Ú' | 'Ù' | 'Ü' | 'Û' => 'U',
        'ñ' => 'n',
        'Ñ' => 'N',
        'ç' => 'c',
        'Ç' => 'C',
        other => other,
    }
}

/// "Christian Mora López" -> "ChristianMoraLopez": ASCII letters and digits
/// only, so it is valid inside a solution unique name.
fn owner_slug(owner: &str) -> String {
    let folded: String = owner.chars().map(fold_accent).collect();
    let slug: String = folded
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            let first = chars.next().map(|c| c.to_ascii_uppercase());
            first
                .into_iter()
                .chain(chars.map(|c| c.to_ascii_lowercase()))
                .collect::<String>()
        })
        .collect();
    slug.chars().take(MAX_OWNER_SLUG).collect()
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Deterministic workflow id: SHA-256 of installation id + original workflow
/// id, encoded as an RFC 4122 version-5 style UUID. Never random per run.
fn instance_workflow_id(installation_id: &str, original_workflow_id: &str) -> String {
    let digest = Sha256::digest(
        format!("atlasbridge/workflow/v1:{installation_id}:{original_workflow_id}").as_bytes(),
    );
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}

fn instance_identity(
    template: &Template,
    installation_id: &str,
    owner: Option<&str>,
) -> Result<InstanceIdentity> {
    let key = instance_key(installation_id)?;
    let short_key = &key[..key.len().min(UNIQUE_NAME_KEY_CHARS)];
    let owner = owner.map(str::to_string);
    let (unique_name, display_name) = match owner.as_deref() {
        Some(name) => {
            let room = MAX_UNIQUE_NAME
                .saturating_sub(template.base_unique_name.len() + short_key.len() + 2);
            let slug: String = owner_slug(name).chars().take(room).collect();
            (
                format!("{}_{slug}_{short_key}", template.base_unique_name),
                format!(
                    "{} - {name} - {}",
                    template.display_prefix,
                    &key[..key.len().min(8)]
                ),
            )
        }
        None => {
            // Installations prepared before owner names keep their exact
            // unique name so a new version still updates them.
            let legacy = format!("{}_{key}", template.base_unique_name);
            (
                if legacy.len() <= MAX_UNIQUE_NAME {
                    legacy
                } else {
                    format!("{}_{short_key}", template.base_unique_name)
                },
                format!("{} [{key}]", template.legacy_display_name),
            )
        }
    };
    Ok(InstanceIdentity {
        unique_name,
        display_name,
        owner,
        connection_references: template
            .connection_references
            .iter()
            .map(|name| format!("{name}_{key}"))
            .collect(),
        workflows: template
            .workflows
            .iter()
            .map(|workflow| {
                (
                    workflow.id.to_string(),
                    instance_workflow_id(installation_id, workflow.id),
                )
            })
            .collect(),
        key,
    })
}

/// "Flow name (Owner)", shortened so it fits the Dataverse process name
/// limit even for very long names.
fn personal_workflow_name(workflow_name: &str, owner: &str) -> String {
    let room = MAX_WORKFLOW_NAME.saturating_sub(workflow_name.chars().count() + 3);
    let owner: String = owner.chars().take(room).collect();
    format!("{workflow_name} ({})", owner.trim_end())
}

/// Dataverse solution unique names: a letter first, then letters, digits
/// or underscores, fewer than 50 characters.
fn valid_unique_name(name: &str) -> bool {
    name.len() <= MAX_UNIQUE_NAME
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn replace_required(content: String, from: &str, to: &str) -> Result<String> {
    if !content.contains(from) {
        return Err(fail("invalid_bundled_solution"));
    }
    Ok(content.replace(from, to))
}

fn personalize_solution_manifest(
    bytes: &[u8],
    template: &Template,
    identity: &InstanceIdentity,
) -> Result<Vec<u8>> {
    let mut xml =
        String::from_utf8(bytes.to_vec()).map_err(|_| fail("invalid_bundled_solution"))?;
    xml = replace_required(
        xml,
        &format!("<UniqueName>{}</UniqueName>", template.base_unique_name),
        &format!("<UniqueName>{}</UniqueName>", identity.unique_name),
    )?;
    xml = replace_required(
        xml,
        &format!(
            "<LocalizedName description=\"{}\" languagecode=\"1033\" />",
            template.manifest_display_name
        ),
        &format!(
            "<LocalizedName description=\"{}\" languagecode=\"1033\" />",
            xml_escape(&identity.display_name)
        ),
    )?;
    for (original, derived) in &identity.workflows {
        xml = replace_required(xml, &format!("{{{original}}}"), &format!("{{{derived}}}"))?;
    }
    Ok(xml.into_bytes())
}

fn personalize_customizations(
    bytes: &[u8],
    template: &Template,
    identity: &InstanceIdentity,
) -> Result<Vec<u8>> {
    let mut xml =
        String::from_utf8(bytes.to_vec()).map_err(|_| fail("invalid_bundled_solution"))?;
    for (original, derived) in &identity.workflows {
        xml = replace_required(xml, &format!("{{{original}}}"), &format!("{{{derived}}}"))?;
        xml = replace_required(xml, original, derived)?;
    }
    for (position, original) in template.connection_references.iter().enumerate() {
        xml = replace_required(
            xml,
            &format!("connectionreferencelogicalname=\"{original}\""),
            &format!(
                "connectionreferencelogicalname=\"{}\"",
                identity.connection_references[position]
            ),
        )?;
    }
    if let Some(owner) = identity.owner.as_deref() {
        // Flows show the person in the Power Automate list as well.
        for workflow in template.workflows {
            let personal = xml_escape(&personal_workflow_name(workflow.name, owner));
            xml = replace_required(
                xml,
                &format!("Name=\"{}\"", workflow.name),
                &format!("Name=\"{personal}\""),
            )?;
            xml = replace_required(
                xml,
                &format!("description=\"{}\"", workflow.name),
                &format!("description=\"{personal}\""),
            )?;
        }
    }
    Ok(xml.into_bytes())
}

fn personalize_flow(
    bytes: &[u8],
    template: &Template,
    identity: &InstanceIdentity,
    installation_id: &str,
    patch: &dyn Fn(&mut Value) -> Result<()>,
) -> Result<Vec<u8>> {
    let mut flow: Value =
        serde_json::from_slice(bytes).map_err(|_| fail("invalid_bundled_solution"))?;
    if template.installation_action {
        let action = flow["properties"]["definition"]["actions"]
            .get_mut("AtlasInstallationId")
            .ok_or_else(|| fail("invalid_bundled_solution"))?;
        action["inputs"] = Value::String(installation_id.to_string());
    }
    patch(&mut flow)?;
    // The keys (shared_office365, shared_teams, shared_onedriveforbusiness)
    // identify the standard connectors and must stay intact. Only the logical
    // name of the connection reference becomes installation-specific.
    let references = flow["properties"]["connectionReferences"]
        .as_object_mut()
        .ok_or_else(|| fail("invalid_bundled_solution"))?;
    for reference in references.values_mut() {
        let logical = reference["connection"]["connectionReferenceLogicalName"]
            .as_str()
            .ok_or_else(|| fail("invalid_bundled_solution"))?
            .to_string();
        let position = template
            .connection_references
            .iter()
            .position(|name| *name == logical)
            .ok_or_else(|| fail("invalid_bundled_solution"))?;
        reference["connection"]["connectionReferenceLogicalName"] =
            Value::String(identity.connection_references[position].clone());
    }
    Ok(serde_json::to_vec_pretty(&flow)?)
}

fn instance_workflow_file_name(template_name: &str, identity: &InstanceIdentity) -> String {
    let mut name = template_name.to_string();
    for (original, derived) in &identity.workflows {
        name = name.replace(original.as_str(), derived.as_str());
    }
    name
}

fn ensure_well_formed_xml(content: &[u8]) -> Result<()> {
    let mut reader = quick_xml::Reader::from_reader(content);
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(quick_xml::events::Event::Eof) => return Ok(()),
            Ok(_) => buffer.clear(),
            Err(_) => return Err(fail("package_generation_failed")),
        }
    }
}

fn read_package_entry(package: &[u8], name: &str) -> Result<Vec<u8>> {
    let mut zip =
        ZipArchive::new(Cursor::new(package)).map_err(|_| fail("package_generation_failed"))?;
    let mut entry = zip
        .by_name(name)
        .map_err(|_| fail("package_generation_failed"))?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|_| fail("package_generation_failed"))?;
    Ok(bytes)
}

/// Verifies the generated package carries exactly this installation's
/// identities and none of the template's global identities.
fn validate_instance_package(
    package: &[u8],
    template: &Template,
    identity: &InstanceIdentity,
    installation_id: &str,
) -> Result<()> {
    let solution = read_package_entry(package, "solution.xml")?;
    let customizations = read_package_entry(package, "customizations.xml")?;
    ensure_well_formed_xml(&solution)?;
    ensure_well_formed_xml(&customizations)?;
    let solution = String::from_utf8(solution).map_err(|_| fail("package_generation_failed"))?;
    let customizations =
        String::from_utf8(customizations).map_err(|_| fail("package_generation_failed"))?;
    // Never hand out a package Power Platform would refuse to import.
    if !valid_unique_name(&identity.unique_name) {
        return Err(fail("package_generation_failed"));
    }
    if !solution.contains(&format!("<UniqueName>{}</UniqueName>", identity.unique_name))
        || solution.contains(&format!(
            "<UniqueName>{}</UniqueName>",
            template.base_unique_name
        ))
    {
        return Err(fail("package_generation_failed"));
    }
    for (workflow, (original, derived)) in template.workflows.iter().zip(&identity.workflows) {
        if solution.contains(original)
            || customizations.contains(original)
            || !customizations.contains(derived)
        {
            return Err(fail("package_generation_failed"));
        }
        let flow_bytes = read_package_entry(
            package,
            &instance_workflow_file_name(workflow.file, identity),
        )?;
        let flow: Value =
            serde_json::from_slice(&flow_bytes).map_err(|_| fail("package_generation_failed"))?;
        if template.installation_action
            && flow["properties"]["definition"]["actions"]["AtlasInstallationId"]["inputs"]
                != Value::String(installation_id.to_string())
        {
            return Err(fail("package_generation_failed"));
        }
        let references = flow["properties"]["connectionReferences"]
            .as_object()
            .ok_or_else(|| fail("package_generation_failed"))?;
        if references.len() != template.connection_references.len() {
            return Err(fail("package_generation_failed"));
        }
        for (connector, reference) in references {
            if !connector.starts_with("shared_") {
                return Err(fail("package_generation_failed"));
            }
            let logical = reference["connection"]["connectionReferenceLogicalName"]
                .as_str()
                .ok_or_else(|| fail("package_generation_failed"))?;
            if !identity.connection_references.contains(&logical.to_string()) {
                return Err(fail("package_generation_failed"));
            }
        }
    }
    for (position, original) in template.connection_references.iter().enumerate() {
        // No unsuffixed template reference may remain as an active identity.
        if customizations.contains(&format!("connectionreferencelogicalname=\"{original}\""))
            || !customizations.contains(&format!(
                "connectionreferencelogicalname=\"{}\"",
                identity.connection_references[position]
            ))
        {
            return Err(fail("package_generation_failed"));
        }
    }
    // The standard Microsoft connector ids stay untouched.
    for connector in template.connectors {
        if !customizations.contains(connector) {
            return Err(fail("package_generation_failed"));
        }
    }
    Ok(())
}

fn personalized_solution(template: &Template, session: &Session) -> Result<Vec<u8>> {
    personalized_package(
        template,
        &session.installation_id,
        session.owner_name.as_deref(),
        &|_| Ok(()),
    )
}

/// Personalizes any bundled template for one installation and owner.
/// `patch` may set extra flow inputs (for example the writer target).
pub fn personalized_package(
    template: &Template,
    installation_id: &str,
    owner: Option<&str>,
    patch: &dyn Fn(&mut Value) -> Result<()>,
) -> Result<Vec<u8>> {
    let identity = instance_identity(template, installation_id, owner)?;
    let mut source = ZipArchive::new(Cursor::new(template.bytes))
        .map_err(|_| fail("invalid_bundled_solution"))?;
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
        let out_name = match name.as_str() {
            "solution.xml" => {
                bytes = personalize_solution_manifest(&bytes, template, &identity)?;
                name
            }
            "customizations.xml" => {
                bytes = personalize_customizations(&bytes, template, &identity)?;
                name
            }
            name if template.workflows.iter().any(|w| w.file == name) => {
                bytes = personalize_flow(&bytes, template, &identity, installation_id, patch)?;
                changed += 1;
                instance_workflow_file_name(name, &identity)
            }
            _ => name,
        };
        output
            .start_file(
                out_name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(|_| fail("package_generation_failed"))?;
        output
            .write_all(&bytes)
            .map_err(|_| fail("package_generation_failed"))?;
    }
    if changed != template.workflows.len() {
        return Err(fail("invalid_bundled_solution"));
    }
    let package = output
        .finish()
        .map_err(|_| fail("package_generation_failed"))?
        .into_inner();
    validate_instance_package(&package, template, &identity, installation_id)?;
    Ok(package)
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

/// QA verification: the personalized export flow must answer the ping
/// request written when the package was prepared.
fn verify_qa(session: &mut Session) -> Verification {
    let Some(folder) = session.folder.clone().map(PathBuf::from) else {
        return verification(VerificationState::Unavailable, "inbox_unavailable", None);
    };
    if !folder.is_dir() {
        return verification(VerificationState::Unavailable, "inbox_unavailable", None);
    }
    let ping = match session.ping_request_id.clone() {
        Some(ping) => ping,
        None => {
            let request = qa_engine::ping_request(&session.installation_id);
            if qa_engine::write_request(&folder, &request).is_err() {
                return verification(VerificationState::Unavailable, "inbox_unavailable", None);
            }
            session.ping_request_id = Some(request.request_id.clone());
            request.request_id
        }
    };
    match qa_engine::ping_state(&folder, &session.installation_id, &ping) {
        qa_engine::PingState::Answered => {
            verification(VerificationState::Valid, "file_verified", Some(ping))
        }
        qa_engine::PingState::Failed => {
            verification(VerificationState::Invalid, "qa_mail_query_failed", Some(ping))
        }
        qa_engine::PingState::NotDownloaded => {
            verification(VerificationState::Waiting, "evidence_not_downloaded", Some(ping))
        }
        qa_engine::PingState::Waiting => {
            verification(VerificationState::Waiting, "waiting_for_sync", None)
        }
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
        let folder = prepare_inbox(dir.path(), SolutionKind::Tracker).unwrap();
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
        assert_eq!(prepare_inbox(dir.path(), SolutionKind::Tracker).unwrap(), folder);
        assert_eq!(
            fs::read_to_string(folder.join("existing.json")).unwrap(),
            "keep me"
        );
        assert!(prepare_inbox(Path::new("relative"), SolutionKind::Tracker).is_err());
        let blocked = tempfile::tempdir().unwrap();
        fs::write(blocked.path().join("AtlasBridge"), "a file").unwrap();
        assert!(prepare_inbox(blocked.path(), SolutionKind::Tracker).is_err());
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

    fn flow_from(bytes: Vec<u8>, workflow_prefix: &str) -> Value {
        let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(zip.by_name("[Content_Types].xml").is_ok());
        assert!(zip.by_name("solution.xml").is_ok());
        assert!(zip.by_name("customizations.xml").is_ok());
        let name = (0..zip.len())
            .map(|index| zip.by_index(index).unwrap().name().to_string())
            .find(|name| name.starts_with(workflow_prefix))
            .unwrap();
        let mut flow = String::new();
        zip.by_name(&name)
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
        let key = instance_key(&session.installation_id).unwrap();
        let bytes = personalized_solution(&TRACKER, &session).unwrap();
        let flow = flow_from(bytes.clone(), "Workflows/AtlasExportEvidence-");
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
            let logical = reference["connection"]["connectionReferenceLogicalName"]
                .as_str()
                .unwrap();
            assert!(logical.starts_with("atlas_"));
            assert!(logical.ends_with(&key));
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
        assert_eq!(actions["RequestedDate"]["type"], "SetVariable");
        assert_eq!(
            actions["RequestedDate"]["runAfter"]["Read_Atlas_requested_date"],
            json!(["Succeeded"])
        );
        assert_eq!(
            actions["RequestedDate"]["inputs"]["value"],
            "@trim(base64ToString(coalesce(json(if(startsWith(string(body('Read_Atlas_requested_date')),'{'),string(body('Read_Atlas_requested_date')),'{}'))?['$content'],base64(string(body('Read_Atlas_requested_date'))))))"
        );
        assert_eq!(
            actions["Initialize_RequestedDate"]["inputs"]["variables"][0]["value"],
            ""
        );
        assert!(actions["TargetDate"]["inputs"]
            .as_str()
            .unwrap()
            .contains("variables('RequestedDate')"));
        let event_flow = flow_from(bytes, "Workflows/AtlasCaptureTeamsMessages-");
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
        let installer = Installer::new(dir.path().to_path_buf(), &TRACKER);
        assert!(installer.action(Action::ConfirmImport {}).is_err());
        let prepared = installer
            .action(Action::Prepare {
                one_drive_root: sync.path().to_string_lossy().into(),
                calendar_name: "Calendar".into(),
                owner_name: "Christian Mora".into(),
            })
            .unwrap();
        let id = prepared.session.installation_id.clone();
        assert_eq!(prepared.session.phase, Phase::WaitingSignIn);
        assert_eq!(prepared.session.owner_name.as_deref(), Some("Christian Mora"));
        assert!(prepared
            .session
            .solution_display_name
            .as_deref()
            .unwrap()
            .starts_with("Atlas Tracker - Christian Mora - "));
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
        let resumed = Installer::new(dir.path().to_path_buf(), &TRACKER);
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

    fn session_with_installation(installation_id: &str) -> Session {
        Session {
            installation_id: installation_id.to_string(),
            ..Session::default()
        }
    }

    fn package_entry_text(package: &[u8], name: &str) -> String {
        String::from_utf8(read_package_entry(package, name).unwrap()).unwrap()
    }

    fn package_workflow_names(package: &[u8]) -> Vec<String> {
        let mut zip = ZipArchive::new(Cursor::new(package)).unwrap();
        (0..zip.len())
            .map(|index| zip.by_index(index).unwrap().name().to_string())
            .filter(|name| name.starts_with("Workflows/"))
            .collect()
    }

    #[test]
    fn instance_key_sanitizes_deterministically_for_power_platform_names() {
        assert_eq!(instance_key("A84C-12F9-9282").unwrap(), "a84c12f99282");
        assert_eq!(instance_key("user_test_a").unwrap(), "user_test_a");
        assert_eq!(
            instance_key("8E5C1F84-DCBB-4A2C-9D2F-62E9C38105D2").unwrap(),
            "8e5c1f84dcbb4a2c9d2f62e9c38105d2"
        );
        assert!(instance_key("---").is_err());
        assert!(instance_key("").is_err());
    }

    #[test]
    fn separate_installations_get_independent_component_identities() {
        let first = personalized_solution(&TRACKER, &session_with_installation("user_test_a")).unwrap();
        let second = personalized_solution(&TRACKER, &session_with_installation("user_test_b")).unwrap();

        let solution_a = package_entry_text(&first, "solution.xml");
        let solution_b = package_entry_text(&second, "solution.xml");
        assert!(solution_a.contains("<UniqueName>AtlasBridge_user_test_a</UniqueName>"));
        assert!(solution_b.contains("<UniqueName>AtlasBridge_user_test_b</UniqueName>"));
        assert!(solution_a.contains("Atlas Bridge [user_test_a]"));

        let customizations_a = package_entry_text(&first, "customizations.xml");
        let customizations_b = package_entry_text(&second, "customizations.xml");
        for (name_a, name_b) in [
            ("atlas_office365_user_test_a", "atlas_office365_user_test_b"),
            ("atlas_teams_user_test_a", "atlas_teams_user_test_b"),
            (
                "atlas_onedriveforbusiness_user_test_a",
                "atlas_onedriveforbusiness_user_test_b",
            ),
        ] {
            assert!(customizations_a.contains(&format!(
                "connectionreferencelogicalname=\"{name_a}\""
            )));
            assert!(!customizations_a.contains(name_b));
            assert!(customizations_b.contains(&format!(
                "connectionreferencelogicalname=\"{name_b}\""
            )));
            assert!(!customizations_b.contains(name_a));
        }
        // The standard Microsoft connector ids are never personalized.
        for connector in [
            "shared_office365",
            "shared_teams",
            "shared_onedriveforbusiness",
        ] {
            assert!(customizations_a.contains(connector));
            assert!(customizations_b.contains(connector));
        }

        let workflows_a = package_workflow_names(&first);
        let workflows_b = package_workflow_names(&second);
        assert_eq!(workflows_a.len(), 2);
        assert_eq!(workflows_b.len(), 2);
        assert!(workflows_a.iter().all(|name| !workflows_b.contains(name)));
        for original in WORKFLOW_IDS {
            assert!(workflows_a.iter().all(|name| !name.contains(original)));
            assert!(workflows_b.iter().all(|name| !name.contains(original)));
        }

        let flow_a = flow_from(first, "Workflows/AtlasExportEvidence-");
        let flow_b = flow_from(second, "Workflows/AtlasExportEvidence-");
        assert_eq!(
            flow_a["properties"]["definition"]["actions"]["AtlasInstallationId"]["inputs"],
            "user_test_a"
        );
        assert_eq!(
            flow_b["properties"]["definition"]["actions"]["AtlasInstallationId"]["inputs"],
            "user_test_b"
        );
    }

    #[test]
    fn same_installation_keeps_identity_across_runs_and_versions() {
        let first = personalized_solution(&TRACKER, &session_with_installation("user_test_a")).unwrap();
        let second = personalized_solution(&TRACKER, &session_with_installation("user_test_a")).unwrap();
        // A new Atlas version changes <Version> only; the unique name, the
        // connection references and the workflow ids stay identical so Power
        // Platform updates the same solution instead of creating another one.
        assert_eq!(
            package_entry_text(&first, "solution.xml"),
            package_entry_text(&second, "solution.xml")
        );
        assert_eq!(
            package_entry_text(&first, "customizations.xml"),
            package_entry_text(&second, "customizations.xml")
        );
        assert_eq!(package_workflow_names(&first), package_workflow_names(&second));
        let solution = package_entry_text(&first, "solution.xml");
        assert!(solution.contains("<UniqueName>AtlasBridge_user_test_a</UniqueName>"));
        assert!(!solution.contains("AtlasBridge_user_test_a_1"));
        assert!(solution.contains(&format!("<Version>{SOLUTION_VERSION}</Version>")));
    }

    #[test]
    fn writes_demo_packages_for_manual_review() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("tmp")
            .join("atlasbridge-identity-demo");
        fs::create_dir_all(&directory).unwrap();
        for id in ["user_test_a", "user_test_b"] {
            let package = personalized_solution(&TRACKER, &session_with_installation(id)).unwrap();
            fs::write(directory.join(format!("AtlasBridge_{id}.zip")), package).unwrap();
            let mut owned = session_with_installation(id);
            owned.owner_name = Some("Christian Mora López".into());
            let package = personalized_solution(&QA, &owned).unwrap();
            fs::write(directory.join(format!("AtlasQA_{id}.zip")), package).unwrap();
        }
    }

    fn owned_session(installation_id: &str, owner: &str) -> Session {
        Session {
            owner_name: Some(owner.into()),
            ..session_with_installation(installation_id)
        }
    }

    #[test]
    fn owner_names_become_safe_unique_name_segments() {
        assert_eq!(owner_slug("Christian Mora"), "ChristianMora");
        assert_eq!(owner_slug("  maría-josé  NÚÑEZ "), "MariaJoseNunez");
        assert_eq!(owner_slug("Christian Rey Mora López"), "ChristianReyMoraLope");
        assert_eq!(normalize_owner("  Ana   Test ").unwrap().as_deref(), Some("Ana Test"));
        assert_eq!(normalize_owner("   ").unwrap(), None);
        assert!(normalize_owner("!!!").is_err());
        assert!(normalize_owner(&"x".repeat(81)).is_err());
    }

    #[test]
    fn every_role_package_is_named_after_its_owner_and_installation() {
        let installation = "8e5c1f84-0000-4000-8000-000000000001";
        let key = instance_key(installation).unwrap();
        for template in [&TRACKER, &QA, &WRITER] {
            let session = owned_session(installation, "Christian Mora & Co <QA>");
            let package = personalized_solution(template, &session).unwrap();
            let solution = package_entry_text(&package, "solution.xml");
            let unique = format!(
                "<UniqueName>{}_ChristianMoraCoQa_{}</UniqueName>",
                template.base_unique_name,
                &key[..UNIQUE_NAME_KEY_CHARS]
            );
            assert!(solution.contains(&unique), "{solution}");
            assert!(solution.contains(&format!(
                "{} - Christian Mora &amp; Co &lt;QA&gt; - {}",
                template.display_prefix,
                &key[..8]
            )));
            let customizations = package_entry_text(&package, "customizations.xml");
            for workflow in template.workflows {
                assert!(customizations.contains(&format!(
                    "Name=\"{} (Christian Mora &amp; Co &lt;QA&gt;)\"",
                    workflow.name
                )));
            }
            ensure_well_formed_xml(solution.as_bytes()).unwrap();
            ensure_well_formed_xml(customizations.as_bytes()).unwrap();
            assert!(unique.len() - "<UniqueName></UniqueName>".len() <= MAX_UNIQUE_NAME);
        }
    }

    #[test]
    fn unique_names_import_for_every_person_and_template() {
        // Dataverse: "The solution UniqueName must contain less than 50 characters".
        let long_names = [
            "Christian Mora",
            "Christian Rey Mora López",
            "María José de los Ángeles Fernández-Villavicencio Rodríguez",
            "Ab",
            "Jean-Luc O'Brien van der Berg III",
            "Alexandra Maximiliana Konstantinopoulou-Papadopoulou de la Santísima Trinidad",
        ];
        for template in [&TRACKER, &QA, &WRITER] {
            for _ in 0..25 {
                let installation = uuid::Uuid::new_v4().to_string();
                for name in long_names {
                    let owner = normalize_owner(name).unwrap();
                    let identity =
                        instance_identity(template, &installation, owner.as_deref()).unwrap();
                    assert!(
                        valid_unique_name(&identity.unique_name),
                        "{} is not importable",
                        identity.unique_name
                    );
                    assert!(identity.unique_name.len() < 50);
                    for workflow in template.workflows {
                        let flow_name =
                            personal_workflow_name(workflow.name, owner.as_deref().unwrap());
                        assert!(flow_name.chars().count() <= MAX_WORKFLOW_NAME, "{flow_name}");
                        assert!(flow_name.starts_with(workflow.name));
                    }
                    // The package generator enforces the same rule.
                    let session = Session {
                        installation_id: installation.clone(),
                        owner_name: owner.clone(),
                        ..Session::new(template)
                    };
                    personalized_solution(template, &session).unwrap();
                }
                // Installations prepared before owner names also import.
                let legacy = instance_identity(template, &installation, None).unwrap();
                assert!(valid_unique_name(&legacy.unique_name), "{}", legacy.unique_name);
            }
        }
        // Existing tracker installations keep their exact legacy name.
        let legacy = instance_identity(&TRACKER, "8e5c1f84-0000-4000-8000-000000000001", None).unwrap();
        assert_eq!(legacy.unique_name, "AtlasBridge_8e5c1f84000040008000000000000001");
        assert!(!valid_unique_name(&format!("AtlasQA_{}", "x".repeat(43))));
        assert!(!valid_unique_name("1Atlas"));
        assert!(!valid_unique_name("Atlas-QA"));
    }

    #[test]
    fn an_unusable_saved_setup_is_kept_aside_not_silently_lost() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("session.json"), b"{ not json").unwrap();
        let installer = Installer::new(dir.path().to_path_buf(), &QA);
        assert_eq!(installer.snapshot().unwrap().session.phase, Phase::CheckingRequirements);
        let kept = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("session.unreadable-"))
            .count();
        assert_eq!(kept, 1);
    }

    #[test]
    fn a_new_solution_version_updates_the_same_installation() {
        let sync = tempfile::tempdir().unwrap();
        for (template, phase, expected_phase, expected_update) in [
            (&QA, Phase::VerifyingFile, Phase::ImportingSolution, false),
            (&TRACKER, Phase::Completed, Phase::Completed, true),
            (&TRACKER, Phase::FindingConnections, Phase::FindingConnections, false),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let folder = prepare_inbox(sync.path(), template.kind).unwrap();
            let old = Session {
                phase,
                folder: Some(folder.to_string_lossy().into()),
                owner_name: Some("Christian Mora".into()),
                solution_version: Some("0.9.0.0".into()),
                ..Session::new(template)
            };
            fs::write(dir.path().join("session.json"), serde_json::to_vec(&old).unwrap()).unwrap();
            let installer = Installer::new(dir.path().to_path_buf(), template);
            let session = installer.snapshot().unwrap().session;
            // Same installation, same owner: re-importing updates the same solution.
            assert_eq!(session.installation_id, old.installation_id);
            assert_eq!(session.owner_name, old.owner_name);
            assert_eq!(session.solution_version.as_deref(), Some(template.version));
            assert_eq!(session.phase, expected_phase);
            assert_eq!(session.update_available, expected_update);
            // The migration is saved and the package is rebuilt for this identity.
            let saved: Session =
                serde_json::from_slice(&fs::read(dir.path().join("session.json")).unwrap()).unwrap();
            assert_eq!(saved.solution_version.as_deref(), Some(template.version));
            let package = fs::read(dir.path().join(template.package_file)).unwrap();
            let solution = package_entry_text(&package, "solution.xml");
            assert!(solution.contains(&format!("<Version>{}</Version>", template.version)));
            let key = instance_key(&old.installation_id).unwrap();
            assert!(solution.contains(&key[..UNIQUE_NAME_KEY_CHARS]));
            if expected_update {
                let acknowledged = installer.action(Action::AcknowledgeUpdate {}).unwrap();
                assert!(!acknowledged.session.update_available);
                assert_eq!(acknowledged.session.phase, Phase::Completed);
            }
        }
    }

    #[test]
    fn a_prepared_package_is_rebuilt_with_the_current_naming_rules() {
        let dir = tempfile::tempdir().unwrap();
        let sync = tempfile::tempdir().unwrap();
        let installer = Installer::new(dir.path().to_path_buf(), &QA);
        let prepared = installer
            .action(Action::Prepare {
                one_drive_root: sync.path().to_string_lossy().into(),
                calendar_name: String::new(),
                owner_name: "Christian Mora".into(),
            })
            .unwrap();
        let package = PathBuf::from(&prepared.package_path);
        // Simulate a ZIP produced by an older release with an invalid name.
        fs::write(&package, b"stale package").unwrap();
        let reopened = Installer::new(dir.path().to_path_buf(), &QA);
        let snapshot = reopened.snapshot().unwrap();
        assert_eq!(snapshot.session.installation_id, prepared.session.installation_id);
        let bytes = fs::read(&package).unwrap();
        let solution = package_entry_text(&bytes, "solution.xml");
        let key = instance_key(&snapshot.session.installation_id).unwrap();
        assert!(solution.contains(&format!(
            "<UniqueName>AtlasQA_ChristianMora_{}</UniqueName>",
            &key[..UNIQUE_NAME_KEY_CHARS]
        )));
    }

    #[test]
    fn qa_packages_are_independent_per_person() {
        let first = personalized_solution(&QA, &owned_session("user_test_a", "Ana Test")).unwrap();
        let second = personalized_solution(&QA, &owned_session("user_test_b", "Luis Test")).unwrap();
        let customizations_a = package_entry_text(&first, "customizations.xml");
        let customizations_b = package_entry_text(&second, "customizations.xml");
        for reference in QA.connection_references {
            assert!(customizations_a
                .contains(&format!("connectionreferencelogicalname=\"{reference}_user_test_a\"")));
            assert!(!customizations_a.contains(&format!("{reference}_user_test_b")));
            assert!(customizations_b
                .contains(&format!("connectionreferencelogicalname=\"{reference}_user_test_b\"")));
        }
        let workflows_a = package_workflow_names(&first);
        let workflows_b = package_workflow_names(&second);
        assert_eq!(workflows_a.len(), 2);
        assert!(workflows_a.iter().all(|name| !workflows_b.contains(name)));
        for workflow in QA.workflows {
            assert!(workflows_a.iter().all(|name| !name.contains(workflow.id)));
        }
        let flow = flow_from(first, "Workflows/AtlasQaExportMail-");
        assert_eq!(
            flow["properties"]["definition"]["actions"]["AtlasInstallationId"]["inputs"],
            "user_test_a"
        );
        assert_eq!(flow["properties"]["connectionReferences"].as_object().unwrap().len(), 2);
    }

    #[test]
    fn qa_bundled_solution_version_matches_installer_contract() {
        let mut zip = ZipArchive::new(Cursor::new(QA_SOLUTION)).unwrap();
        let mut solution = String::new();
        zip.by_name("solution.xml")
            .unwrap()
            .read_to_string(&mut solution)
            .unwrap();
        assert!(solution.contains(&format!("<Version>{QA_SOLUTION_VERSION}</Version>")));
    }

    #[test]
    fn qa_setup_writes_a_ping_and_completes_when_the_flow_answers() {
        let dir = tempfile::tempdir().unwrap();
        let sync = tempfile::tempdir().unwrap();
        let installer = Installer::new(dir.path().to_path_buf(), &QA);
        let prepared = installer
            .action(Action::Prepare {
                one_drive_root: sync.path().to_string_lossy().into(),
                calendar_name: String::new(),
                owner_name: "Ana Test".into(),
            })
            .unwrap();
        assert_eq!(prepared.kind, SolutionKind::Qa);
        assert!(prepared.package_path.ends_with("AtlasQA_1_0_0_0.zip"));
        let session = prepared.session;
        let folder = PathBuf::from(session.folder.clone().unwrap());
        assert!(folder.ends_with(Path::new("AtlasBridge").join("qa")));
        assert!(!folder.to_string_lossy().starts_with(r"\\?\"));
        let request: qa_engine::BridgeRequest =
            serde_json::from_slice(&fs::read(qa_engine::request_file(&folder)).unwrap()).unwrap();
        assert_eq!(request.installation_id, session.installation_id);
        assert_eq!(Some(request.request_id.clone()), session.ping_request_id);
        installer.action(Action::ConfirmSignIn {}).unwrap();
        installer.action(Action::ConfirmConnections {}).unwrap();
        installer.action(Action::ConfirmImport {}).unwrap();
        let waiting = installer.action(Action::Verify {}).unwrap();
        assert_eq!(waiting.session.phase, Phase::VerifyingFile);
        let inbox = qa_engine::inbox_dir(&folder);
        let prefix = format!("atlas-qa-{}-{}", session.installation_id, request.request_id);
        fs::write(
            inbox.join(format!("{prefix}-ping.json")),
            r#"{"contract":"atlas-qa-v1","status":"ok","count":1,"messages":[]}"#,
        )
        .unwrap();
        fs::write(
            inbox.join(format!("{prefix}-done.json")),
            r#"{"contract":"atlas-qa-v1","status":"done"}"#,
        )
        .unwrap();
        assert_eq!(
            installer.action(Action::Verify {}).unwrap().session.phase,
            Phase::Completed
        );
    }
}
