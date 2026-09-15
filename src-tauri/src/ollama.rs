use crate::{
    diagnostics,
    error::{AppError, Context, Result},
};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::OpenOptions,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::Duration,
};
use url::Url;

pub const BUNDLED_MODEL: &str = "qwen2.5:1.5b-instruct-q4_K_M";
const FIRST_OLLAMA_PORT: u16 = 11435;
const LAST_OLLAMA_PORT: u16 = 11445;
const MODEL_BLOB: &str =
    "models/blobs/sha256-183715c435899236895da3869489cc30ac241476b4971a20285b1a462818a5b4";
const MODEL_BLOB_SIZE: u64 = 986_048_512;
const MAX_AI_EVIDENCE_ITEMS: usize = 12;
const MAX_AI_TOTAL_EVIDENCE_ITEMS: usize = 48;
const MAX_AI_EVIDENCE_CHARS: usize = 8_000;
const MAX_AI_TEXT_CHARS: usize = 700;
const MAX_AI_INTERACTIONS: usize = 12;
const MAX_AI_OUTPUT_TOKENS: u32 = 768;

pub struct ManagedRuntime {
    root: PathBuf,
    log_path: PathBuf,
    child: Mutex<Option<Child>>,
    port: Mutex<Option<u16>>,
    startup: tokio::sync::Mutex<()>,
}

impl ManagedRuntime {
    pub fn discover(log_path: PathBuf) -> Self {
        #[cfg(debug_assertions)]
        let configured_root = std::env::var_os("ATLAS_AI_ROOT").map(PathBuf::from);
        #[cfg(not(debug_assertions))]
        let configured_root: Option<PathBuf> = None;

        let root = configured_root.unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| PathBuf::from("."))
                .join("AtlasAI")
        });
        Self {
            root,
            log_path,
            child: Mutex::new(None),
            port: Mutex::new(None),
            startup: tokio::sync::Mutex::new(()),
        }
    }

    fn executable(&self) -> PathBuf {
        self.root.join("ollama.exe")
    }

    fn models(&self) -> PathBuf {
        self.root.join("models")
    }

    fn validate_bundle(&self) -> Result<()> {
        let executable = self.executable();
        if !executable.is_file() {
            return Err(AppError::Message(format!(
                "The bundled Local AI runtime is missing at {}. Extract the complete Atlas portable ZIP and try again.",
                executable.display()
            )));
        }
        let model = self.root.join(MODEL_BLOB);
        let metadata =
            std::fs::metadata(&model).context("Unable to inspect the bundled AI model")?;
        if !metadata.is_file() || metadata.len() != MODEL_BLOB_SIZE {
            return Err(AppError::Message(format!(
                "The bundled Local AI model is missing or incomplete at {}. Extract the complete Atlas portable ZIP and try again.",
                model.display()
            )));
        }
        Ok(())
    }

    fn start_if_needed(&self, port: u16) -> Result<()> {
        self.validate_bundle()?;
        let mut child = self
            .child
            .lock()
            .map_err(|_| AppError::Message("Local AI process lock was poisoned".into()))?;
        if let Some(process) = child.as_mut() {
            match process.try_wait() {
                Ok(None) => return Ok(()),
                Ok(Some(status)) => diagnostics::error(
                    "local-ai/process",
                    &format!("Bundled Ollama exited unexpectedly with {status}"),
                ),
                Err(error) => {
                    return Err(AppError::Message(format!(
                        "Unable to inspect the bundled Local AI process: {error}"
                    )))
                }
            }
        }

        let output = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .context("Unable to open the Local AI diagnostic log")?;
        let error_output = output
            .try_clone()
            .context("Unable to prepare the Local AI diagnostic log")?;
        let mut command = Command::new(self.executable());
        command
            .arg("serve")
            .current_dir(&self.root)
            .env("OLLAMA_HOST", format!("127.0.0.1:{port}"))
            .env("OLLAMA_MODELS", self.models())
            .env("OLLAMA_NOHISTORY", "1")
            .env("OLLAMA_NO_CLOUD", "1")
            .env("OLLAMA_KEEP_ALIVE", "5m")
            .stdin(Stdio::null())
            .stdout(Stdio::from(output))
            .stderr(Stdio::from(error_output));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let process = command
            .spawn()
            .context("Unable to start the bundled Local AI runtime")?;
        diagnostics::info(
            "local-ai/process",
            &format!(
                "Started bundled Ollama process {} on 127.0.0.1:{port}",
                process.id()
            ),
        );
        *child = Some(process);
        Ok(())
    }

    fn child_is_running(&self) -> Result<bool> {
        let mut child = self
            .child
            .lock()
            .map_err(|_| AppError::Message("Local AI process lock was poisoned".into()))?;
        let Some(process) = child.as_mut() else {
            return Ok(false);
        };
        match process.try_wait() {
            Ok(None) => Ok(true),
            Ok(Some(status)) => {
                diagnostics::error(
                    "local-ai/process",
                    &format!("Bundled Ollama stopped with {status}"),
                );
                *child = None;
                Ok(false)
            }
            Err(error) => Err(AppError::Message(format!(
                "Unable to inspect the bundled Local AI process: {error}"
            ))),
        }
    }

    pub async fn ensure_ready(&self, client: &Client) -> Result<()> {
        let _startup = self.startup.lock().await;
        let current_port = *self
            .port
            .lock()
            .map_err(|_| AppError::Message("Local AI port lock was poisoned".into()))?;
        if self.child_is_running()? {
            if let Some(port) = current_port {
                if probe(client, port).await == (true, true) {
                    return Ok(());
                }
            }
            self.stop();
        }

        let mut available = None;
        for port in FIRST_OLLAMA_PORT..=LAST_OLLAMA_PORT {
            if !probe(client, port).await.0 {
                available = Some(port);
                break;
            }
        }
        let port = available.ok_or_else(|| {
            AppError::Message(format!(
                "No Local AI port is available between {FIRST_OLLAMA_PORT} and {LAST_OLLAMA_PORT}."
            ))
        })?;
        *self
            .port
            .lock()
            .map_err(|_| AppError::Message("Local AI port lock was poisoned".into()))? = Some(port);
        self.start_if_needed(port)?;
        for _ in 0..80 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let current = probe(client, port).await;
            if current == (true, true) {
                return Ok(());
            }
            if current.0 {
                return Err(AppError::Message(format!(
                    "The bundled Local AI runtime started, but model {BUNDLED_MODEL} was not found in the portable package."
                )));
            }
            if !self.child_is_running()? {
                return Err(AppError::Message(format!(
                    "The bundled Local AI runtime stopped during startup. See {} for details.",
                    self.log_path.display()
                )));
            }
        }
        Err(AppError::Message(format!(
            "The bundled Local AI runtime did not become ready within 20 seconds. See {} for details.",
            self.log_path.display()
        )))
    }

    pub async fn status(&self, client: &Client) -> (bool, bool, Option<String>) {
        match self.ensure_ready(client).await {
            Ok(()) => (true, true, None),
            Err(error) => {
                let message = error.to_string();
                diagnostics::error("local-ai/status", &message);
                (false, false, Some(message))
            }
        }
    }

    pub async fn infer_tasks(
        &self,
        client: &Client,
        model: &str,
        evidence: &[ChatEvidence],
        source_label: &str,
        actor: &str,
    ) -> Result<Vec<SuggestedInteraction>> {
        self.ensure_ready(client).await?;
        let port = self
            .port
            .lock()
            .map_err(|_| AppError::Message("Local AI port lock was poisoned".into()))?
            .ok_or_else(|| AppError::Message("Local AI did not select a port".into()))?;
        let batches = evidence_batches(evidence);
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for batch in batches {
            for suggestion in summarize_at(client, model, &batch, source_label, actor, port).await?
            {
                let key = format!(
                    "{}|{}",
                    suggestion.source_ids.join("|"),
                    suggestion.summary.trim().to_ascii_lowercase()
                );
                if seen.insert(key) {
                    result.push(suggestion);
                }
            }
        }
        Ok(result)
    }

    pub fn stop(&self) {
        let Ok(mut child) = self.child.lock() else {
            return;
        };
        if let Some(mut process) = child.take() {
            let pid = process.id();
            #[cfg(windows)]
            let tree_stopped = terminate_process_tree(pid);
            #[cfg(not(windows))]
            let tree_stopped = false;

            let still_running = matches!(process.try_wait(), Ok(None));
            let stopped = if still_running {
                match process.kill() {
                    Ok(()) => true,
                    Err(_) if tree_stopped => true,
                    Err(error) => {
                        diagnostics::error(
                            "local-ai/process",
                            &format!("Unable to stop bundled Ollama process {pid}: {error}"),
                        );
                        false
                    }
                }
            } else {
                true
            };
            if stopped {
                let _ = process.wait();
                diagnostics::info(
                    "local-ai/process",
                    &format!("Stopped bundled Ollama process tree {pid}"),
                );
            }
        }
        if let Ok(mut port) = self.port.lock() {
            *port = None;
        }
    }
}

#[cfg(windows)]
fn terminate_process_tree(pid: u32) -> bool {
    use std::os::windows::process::CommandExt;

    let executable = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .map(|root| root.join("System32").join("taskkill.exe"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("taskkill.exe"));
    match Command::new(executable)
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) => status.success(),
        Err(error) => {
            diagnostics::error(
                "local-ai/process",
                &format!("Unable to terminate bundled Ollama process tree {pid}: {error}"),
            );
            false
        }
    }
}

impl Drop for ManagedRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone, Debug)]
pub struct ChatEvidence {
    pub id: String,
    pub author: String,
    pub created: DateTime<Utc>,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct SuggestedInteraction {
    pub source_ids: Vec<String>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub summary: String,
    pub participants: Vec<String>,
    pub resolved: bool,
}

#[derive(Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<ModelInfo>,
}

#[derive(Deserialize)]
struct ModelInfo {
    name: String,
}

fn localhost_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).context("Invalid built-in Ollama URL")?;
    let ip = url
        .host_str()
        .and_then(|host| host.parse::<std::net::IpAddr>().ok());
    if url.scheme() != "http" || !ip.is_some_and(|value| value.is_loopback()) {
        return Err(AppError::Message(
            "Refused a local-AI request because its destination was not a loopback address.".into(),
        ));
    }
    Ok(url)
}

fn runtime_url(port: u16, path: &str) -> Result<Url> {
    localhost_url(&format!("http://127.0.0.1:{port}/{path}"))
}

async fn probe(client: &Client, port: u16) -> (bool, bool) {
    let Ok(url) = runtime_url(port, "api/tags") else {
        return (false, false);
    };
    let Ok(response) = client.get(url).timeout(Duration::from_secs(2)).send().await else {
        return (false, false);
    };
    if !response.status().is_success() {
        return (false, false);
    }
    let Ok(tags) = response.json::<TagsResponse>().await else {
        return (true, false);
    };
    (
        true,
        tags.models.iter().any(|model| model.name == BUNDLED_MODEL),
    )
}

#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: String,
    stream: bool,
    format: serde_json::Value,
    options: GenerateOptions,
}

#[derive(Serialize)]
struct GenerateOptions {
    temperature: f32,
    num_ctx: u32,
    num_predict: u32,
}

#[derive(Deserialize)]
struct GenerateResponse {
    response: String,
}

#[derive(Deserialize)]
struct ModelOutput {
    #[serde(default)]
    interactions: Vec<ModelCandidate>,
}

#[derive(Deserialize)]
struct ModelCandidate {
    #[serde(default)]
    source_ids: Vec<String>,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    participants: Vec<String>,
    #[serde(default)]
    is_work_task: bool,
    #[serde(default)]
    reminder_or_notice: bool,
    #[serde(default)]
    work_status: String,
}

fn normalized_evidence(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|character| match character {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            value if value.is_alphanumeric() => value,
            _ => ' ',
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn looks_like_reminder_or_notice(value: &str) -> bool {
    let clean = normalized_evidence(value);
    let padded = format!(" {clean} ");
    [
        "remember ",
        "reminder ",
        "dont forget ",
        "recuerda ",
        "recordatorio ",
        "recordar ",
        "no olvidar ",
        "was invited ",
        "received an invitation ",
        "received an email invitation ",
        "fue invitado ",
        "recibio una invitacion ",
        "accepted invitation ",
        "declined invitation ",
        "automatic reply ",
        "out of office ",
        "respuesta automatica ",
        "fuera de la oficina ",
    ]
    .iter()
    .any(|prefix| clean.starts_with(prefix))
        || [
            " remember to ",
            " reminder ",
            " was invited ",
            " received an email invitation ",
            " recibio una invitacion ",
            " accepted invitation ",
            " declined invitation ",
            " automatic reply ",
            " out of office ",
            " respuesta automatica ",
            " fuera de la oficina ",
        ]
        .iter()
        .any(|phrase| padded.contains(phrase))
}

fn candidate_is_work_task(candidate: &ModelCandidate, evidence: &[&ChatEvidence]) -> bool {
    if !candidate.is_work_task || candidate.reminder_or_notice {
        return false;
    }
    if candidate.summary.trim().chars().count() < 4
        || looks_like_reminder_or_notice(&candidate.summary)
    {
        return false;
    }
    evidence
        .iter()
        .any(|item| !looks_like_reminder_or_notice(&item.text))
}

async fn summarize_at(
    client: &Client,
    model: &str,
    evidence: &[ChatEvidence],
    source_label: &str,
    actor: &str,
    port: u16,
) -> Result<Vec<SuggestedInteraction>> {
    if evidence.is_empty() {
        return Ok(Vec::new());
    }
    if model != BUNDLED_MODEL {
        return Err(AppError::Message(
            "Refused to use an unbundled Local AI model.".into(),
        ));
    }
    let url = runtime_url(port, "api/generate")?;
    let original_count = evidence.len();
    let evidence = bounded_evidence(evidence);
    if evidence.is_empty() {
        return Ok(Vec::new());
    }
    diagnostics::info(
        "local-ai/analysis",
        &format!(
            "Analyzing {} of {} {} evidence items",
            evidence.len(),
            original_count,
            source_label
        ),
    );
    // Short aliases keep the prompt and JSON response small. The aliases are
    // resolved back to the original evidence IDs before leaving this module.
    let aliased = evidence
        .iter()
        .enumerate()
        .map(|(index, message)| (format!("m{index}"), message))
        .collect::<Vec<_>>();
    let lines = aliased
        .iter()
        .map(|(alias, message)| {
            format!(
                "[id={}] [{}] {}: {}",
                alias,
                message.created.to_rfc3339(),
                message.author,
                message.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        r#"You identify concrete daily work tasks involving {actor} from REAL {source_label} evidence.
Return strict JSON with this shape: {{"interactions":[{{"source_ids":["exact-message-id"],"participants":["names found in messages"],"summary":"one factual English sentence","is_work_task":true,"reminder_or_notice":false,"work_status":"In Progress"}}]}}.
Rules: return up to {MAX_AI_INTERACTIONS} distinct work tasks. A task may be requested, assigned, planned, in progress, or completed; it does not need proof of completion. Split separate tasks into separate interactions even when they use the same source ID. Include a task only when the evidence describes a concrete work action for, by, or involving {actor}, such as analyzing, preparing, changing, resolving, delivering, coordinating, reviewing, testing, documenting, investigating, or producing something. A request such as "Please send the QA audits" is a task and may be In Progress. Work discussed as currently underway is In Progress. Use Resolved only when the evidence says the work was finished; otherwise use In Progress.
Set reminder_or_notice=true and is_work_task=false for reminder text, personal to-do alerts, calendar reminder blocks, meeting invitations, automatic replies, system notifications, greetings, acknowledgements, and messages that merely say an email or chat was sent or received without describing work. "Remember to send QA audits" is a reminder and produces no interaction. A meeting invitation email produces no task because calendar supplies the meeting separately. Never turn a reminder into a task merely because it mentions an action.
Keep each summary under 160 characters. Write every summary in English, even when the evidence is in another language. Use only IDs and facts present below; never invent work, people, clients, incidents, outcomes, or times. Every interaction must reference at least one exact evidence ID. The app derives times from the real evidence. Return the JSON object immediately with no explanation.

MESSAGES:
{lines}"#
    );
    let response = client
        .post(url)
        .timeout(Duration::from_secs(90))
        .json(&GenerateRequest {
            model,
            prompt,
            stream: false,
            format: serde_json::json!({
                "type": "object",
                "properties": {
                    "interactions": {
                        "type": "array",
                        "maxItems": MAX_AI_INTERACTIONS,
                        "items": {
                            "type": "object",
                            "properties": {
                                "source_ids": {
                                    "type": "array",
                                    "minItems": 1,
                                    "maxItems": 3,
                                    "items": { "type": "string" }
                                },
                                "participants": {
                                    "type": "array",
                                    "maxItems": 5,
                                    "items": { "type": "string" }
                                },
                                "summary": { "type": "string", "maxLength": 160 },
                                "is_work_task": { "type": "boolean" },
                                "reminder_or_notice": { "type": "boolean" },
                                "work_status": {
                                    "type": "string",
                                    "enum": ["Resolved", "In Progress"]
                                }
                            },
                            "required": ["source_ids", "participants", "summary", "is_work_task", "reminder_or_notice", "work_status"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["interactions"],
                "additionalProperties": false
            }),
            options: GenerateOptions {
                temperature: 0.0,
                num_ctx: 6144,
                num_predict: MAX_AI_OUTPUT_TOKENS,
            },
        })
        .send()
        .await
        .context("The bundled Local AI request failed")?;
    if !response.status().is_success() {
        return Err(AppError::Message(format!(
            "Bundled Local AI returned {}. No chat suggestions were created.",
            response.status()
        )));
    }
    let raw: GenerateResponse = response
        .json()
        .await
        .context("Bundled Local AI returned an invalid response envelope")?;
    let parsed: ModelOutput = serde_json::from_str(raw.response.trim()).map_err(|error| {
        diagnostics::error(
            "local-ai/analysis",
            &format!(
                "Bundled Local AI returned invalid structured JSON ({} bytes): {error}",
                raw.response.len()
            ),
        );
        AppError::Message("Bundled Local AI did not return valid structured JSON".into())
    })?;
    let by_id: HashMap<&str, &ChatEvidence> = aliased
        .iter()
        .map(|(alias, value)| (alias.as_str(), *value))
        .collect();
    let mut seen_tasks = HashSet::new();
    let mut result = Vec::new();
    for candidate in parsed.interactions.into_iter().take(MAX_AI_INTERACTIONS) {
        let mut aliases: Vec<String> = candidate
            .source_ids
            .iter()
            .filter(|id| by_id.contains_key(id.as_str()))
            .cloned()
            .collect();
        aliases.sort();
        aliases.dedup();
        if aliases.is_empty() || candidate.summary.trim().is_empty() {
            continue;
        }
        let task_key = format!(
            "{}|{}",
            aliases.join("|"),
            candidate.summary.trim().to_ascii_lowercase()
        );
        if !seen_tasks.insert(task_key) {
            continue;
        }
        let mut matched: Vec<_> = aliases
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .collect();
        if !candidate_is_work_task(&candidate, &matched) {
            diagnostics::info(
                "local-ai/validation",
                "Discarded a suggestion classified as a reminder, notice, or non-task",
            );
            continue;
        }
        matched.sort_by_key(|value| value.created);
        let start = matched.first().unwrap().created;
        let end = matched.last().unwrap().created;
        let mut source_ids = matched
            .iter()
            .map(|value| value.id.clone())
            .collect::<Vec<_>>();
        source_ids.sort();
        source_ids.dedup();
        result.push(SuggestedInteraction {
            source_ids,
            start,
            end,
            summary: candidate.summary.trim().to_string(),
            participants: candidate.participants,
            resolved: candidate.work_status.eq_ignore_ascii_case("Resolved"),
        });
    }
    diagnostics::info(
        "local-ai/analysis",
        &format!(
            "Created {} bounded {} task suggestions",
            result.len(),
            source_label
        ),
    );
    Ok(result)
}

fn bounded_evidence(evidence: &[ChatEvidence]) -> Vec<ChatEvidence> {
    let mut candidates = evidence.to_vec();
    candidates.sort_by_key(|item| std::cmp::Reverse(item.created));
    candidates.truncate(MAX_AI_EVIDENCE_ITEMS);

    let mut remaining = MAX_AI_EVIDENCE_CHARS;
    let mut bounded = Vec::new();
    for mut item in candidates {
        if remaining == 0 {
            break;
        }
        let limit = remaining.min(MAX_AI_TEXT_CHARS);
        item.text = item.text.chars().take(limit).collect();
        if item.text.trim().is_empty() {
            continue;
        }
        remaining = remaining.saturating_sub(item.text.chars().count());
        bounded.push(item);
    }
    bounded.sort_by_key(|item| item.created);
    bounded
}

fn evidence_batches(evidence: &[ChatEvidence]) -> Vec<Vec<ChatEvidence>> {
    let mut candidates = evidence.to_vec();
    candidates.sort_by_key(|item| item.created);
    if candidates.len() > MAX_AI_TOTAL_EVIDENCE_ITEMS {
        let last = candidates.len() - 1;
        candidates = (0..MAX_AI_TOTAL_EVIDENCE_ITEMS)
            .map(|index| {
                let source_index = index * last / (MAX_AI_TOTAL_EVIDENCE_ITEMS - 1);
                candidates[source_index].clone()
            })
            .collect();
    }
    candidates
        .chunks(MAX_AI_EVIDENCE_ITEMS)
        .map(|batch| batch.to_vec())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_ai_destinations_are_compile_time_loopback_only() {
        assert!(runtime_url(FIRST_OLLAMA_PORT, "api/generate").is_ok());
        assert!(localhost_url("https://example.com/api").is_err());
        assert!(localhost_url("http://localhost:11435/api/generate").is_err());
    }

    #[test]
    fn runtime_is_discovered_beside_atlas() {
        let runtime = ManagedRuntime {
            root: PathBuf::from(r"C:\Atlas\AtlasAI"),
            log_path: PathBuf::from(r"C:\logs\local-ai.log"),
            child: Mutex::new(None),
            port: Mutex::new(None),
            startup: tokio::sync::Mutex::new(()),
        };
        assert_eq!(
            runtime.executable(),
            PathBuf::from(r"C:\Atlas\AtlasAI\ollama.exe")
        );
        assert_eq!(runtime.models(), PathBuf::from(r"C:\Atlas\AtlasAI\models"));
    }

    #[test]
    fn ai_evidence_is_sampled_across_the_day_and_batched() {
        let evidence = (0..100)
            .map(|index| ChatEvidence {
                id: index.to_string(),
                author: "Person".into(),
                created: DateTime::from_timestamp(index, 0).unwrap(),
                text: "x".repeat(1_000),
            })
            .collect::<Vec<_>>();
        let batches = evidence_batches(&evidence);
        assert_eq!(batches.len(), 4);
        assert!(batches.iter().all(|batch| batch.len() <= 12));
        assert_eq!(batches.first().unwrap().first().unwrap().id, "0");
        assert_eq!(batches.last().unwrap().last().unwrap().id, "99");
        for batch in batches {
            let bounded = bounded_evidence(&batch);
            assert!(bounded.iter().map(|item| item.text.len()).sum::<usize>() <= 8_000);
        }
    }

    fn evidence(text: &str) -> ChatEvidence {
        ChatEvidence {
            id: "message-1".into(),
            author: "Christian Mora".into(),
            created: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            text: text.into(),
        }
    }

    fn candidate(summary: &str) -> ModelCandidate {
        ModelCandidate {
            source_ids: vec!["m0".into()],
            summary: summary.into(),
            participants: Vec::new(),
            is_work_task: true,
            reminder_or_notice: false,
            work_status: "In Progress".into(),
        }
    }

    #[test]
    fn real_tasks_do_not_require_proof_of_completion() {
        for value in [
            "Please send the QA audits tomorrow",
            "I am reviewing the QA audit findings",
            "I sent the QA audits and documented the findings",
            "Christian was assigned the dashboard validation",
        ] {
            let message = evidence(value);
            assert!(candidate_is_work_task(&candidate(value), &[&message]));
        }
    }

    #[test]
    fn reminders_invitations_and_automatic_notices_are_not_tasks() {
        for value in [
            "Remember to send QA audits",
            "Christian received an email invitation to the review",
            "Recuerda enviar el informe",
            "Automatic reply: out of office",
        ] {
            let message = evidence(value);
            assert!(!candidate_is_work_task(&candidate(value), &[&message]));
        }
        let message = evidence("Daily notification");
        let mut notice = candidate("Review the daily notification");
        notice.reminder_or_notice = true;
        assert!(!candidate_is_work_task(&notice, &[&message]));
    }
}
