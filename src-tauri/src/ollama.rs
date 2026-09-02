use crate::error::{AppError, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use url::Url;

const OLLAMA_GENERATE_URL: &str = "http://127.0.0.1:11434/api/generate";
const OLLAMA_TAGS_URL: &str = "http://127.0.0.1:11434/api/tags";

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
    if url.scheme() != "http" || !ip.is_some_and(|v| v.is_loopback()) {
        return Err(AppError::Message(
            "Refused a local-AI request because its destination was not a loopback address.".into(),
        ));
    }
    Ok(url)
}

pub async fn status(client: &Client, model: &str) -> (bool, bool) {
    let Ok(url) = localhost_url(OLLAMA_TAGS_URL) else {
        return (false, false);
    };
    let Ok(response) = client
        .get(url)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    else {
        return (false, false);
    };
    if !response.status().is_success() {
        return (false, false);
    }
    let Ok(tags) = response.json::<TagsResponse>().await else {
        return (true, false);
    };
    let base = model.split(':').next().unwrap_or(model);
    (
        true,
        tags.models
            .iter()
            .any(|v| v.name == model || v.name.split(':').next() == Some(base)),
    )
}

#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: String,
    stream: bool,
    format: &'static str,
    options: GenerateOptions,
}
#[derive(Serialize)]
struct GenerateOptions {
    temperature: f32,
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
}

pub async fn summarize(
    client: &Client,
    model: &str,
    evidence: &[ChatEvidence],
) -> Result<Vec<SuggestedInteraction>> {
    if evidence.is_empty() {
        return Ok(Vec::new());
    }
    let url = localhost_url(OLLAMA_GENERATE_URL)?;
    let lines = evidence
        .iter()
        .map(|m| {
            format!(
                "[id={}] [{}] {}: {}",
                m.id,
                m.created.to_rfc3339(),
                m.author,
                m.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        r#"You are grouping REAL Microsoft Teams chat messages into possible work interactions for human review.
Return strict JSON with this shape: {{"interactions":[{{"source_ids":["exact-message-id"],"participants":["names found in messages"],"summary":"one factual sentence"}}]}}.
Rules: use only IDs and facts present below; never invent work, people, clients, incidents, outcomes, or times; omit social chatter and messages that do not evidence a work interaction; an interaction must reference at least one exact message ID. The app derives times from those real messages, not from your output.

MESSAGES:
{lines}"#
    );
    let response = client
        .post(url)
        .timeout(std::time::Duration::from_secs(90))
        .json(&GenerateRequest {
            model,
            prompt,
            stream: false,
            format: "json",
            options: GenerateOptions { temperature: 0.0 },
        })
        .send()
        .await
        .context("The local Ollama request failed")?;
    if !response.status().is_success() {
        return Err(AppError::Message(format!(
            "Local Ollama returned {}. No chat suggestions were created.",
            response.status()
        )));
    }
    let raw: GenerateResponse = response
        .json()
        .await
        .context("Ollama returned an invalid response envelope")?;
    let parsed: ModelOutput =
        serde_json::from_str(raw.response.trim()).context("Ollama did not return valid JSON")?;
    let by_id: HashMap<&str, &ChatEvidence> = evidence.iter().map(|v| (v.id.as_str(), v)).collect();
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for candidate in parsed.interactions {
        let mut ids: Vec<String> = candidate
            .source_ids
            .into_iter()
            .filter(|id| by_id.contains_key(id.as_str()) && seen.insert(id.clone()))
            .collect();
        ids.sort();
        ids.dedup();
        if ids.is_empty() || candidate.summary.trim().is_empty() {
            continue;
        }
        let mut matched: Vec<_> = ids
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .collect();
        matched.sort_by_key(|v| v.created);
        let start = matched.first().unwrap().created;
        let end = matched.last().unwrap().created;
        result.push(SuggestedInteraction {
            source_ids: ids,
            start,
            end,
            summary: candidate.summary.trim().to_string(),
            participants: candidate.participants,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ollama_destinations_are_compile_time_loopback_only() {
        assert!(localhost_url(OLLAMA_GENERATE_URL).is_ok());
        assert!(localhost_url("https://example.com/api").is_err());
        assert!(localhost_url("http://localhost:11434/api/generate").is_err());
    }
}
