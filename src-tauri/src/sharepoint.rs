use crate::{
    diagnostics,
    error::{AppError, Context, Result},
    excel,
    models::{ExportResult, Interaction, UserProfile},
    state::AppState,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use reqwest::header::{CONTENT_TYPE, IF_MATCH};
use serde::Deserialize;
use std::{fs, path::PathBuf};
use url::Url;

const GRAPH_ROOT: &str = "https://graph.microsoft.com/v1.0";
const SIMPLE_UPLOAD_LIMIT: usize = 250 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveItem {
    id: String,
    name: String,
    e_tag: Option<String>,
    parent_reference: Option<ParentReference>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParentReference {
    drive_id: Option<String>,
}

struct TemporaryWorkbook(PathBuf);

impl Drop for TemporaryWorkbook {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn normalize_url(value: &str) -> Result<String> {
    let value = value.trim();
    let url = Url::parse(value).context("Enter a valid SharePoint sharing link")?;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let allowed = host == "1drv.ms"
        || host == "onedrive.live.com"
        || host == "sharepoint.com"
        || host.ends_with(".sharepoint.com");
    if url.scheme() != "https" || !allowed {
        return Err(AppError::Message(
            "Use an HTTPS SharePoint or OneDrive sharing link.".into(),
        ));
    }
    Ok(url.to_string())
}

fn sharing_token(value: &str) -> String {
    format!("u!{}", URL_SAFE_NO_PAD.encode(value.as_bytes()))
}

fn graph_segment(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

async fn resolve(state: &AppState, token: &str, sharing_url: &str) -> Result<DriveItem> {
    let endpoint = format!(
        "{GRAPH_ROOT}/shares/{}/driveItem?$select=id,name,eTag,parentReference",
        sharing_token(sharing_url)
    );
    let response = state
        .http
        .get(endpoint)
        .bearer_auth(token)
        .header("Prefer", "redeemSharingLinkIfNecessary")
        .send()
        .await
        .context("Unable to resolve the SharePoint tracker link")?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Message(format!(
            "SharePoint could not open this tracker ({status}): {}",
            body.chars().take(500).collect::<String>()
        )));
    }
    let item: DriveItem = response
        .json()
        .await
        .context("SharePoint returned invalid tracker metadata")?;
    let extension = PathBuf::from(&item.name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "xlsx" | "xlsm") {
        return Err(AppError::Message(
            "The SharePoint link must point directly to an .xlsx or .xlsm workbook.".into(),
        ));
    }
    if item
        .parent_reference
        .as_ref()
        .and_then(|value| value.drive_id.as_ref())
        .is_none()
    {
        return Err(AppError::Message(
            "SharePoint did not return a drive identifier for this workbook.".into(),
        ));
    }
    Ok(item)
}

pub async fn validate(state: &AppState, token: &str, sharing_url: &str) -> Result<String> {
    let normalized = normalize_url(sharing_url)?;
    let item = resolve(state, token, &normalized).await?;
    diagnostics::info("sharepoint", &format!("Validated workbook {}", item.name));
    Ok(item.name)
}

pub async fn export(
    state: &AppState,
    token: &str,
    sharing_url: &str,
    profile: &UserProfile,
    interactions: &[Interaction],
) -> Result<ExportResult> {
    let normalized = normalize_url(sharing_url)?;
    let item = resolve(state, token, &normalized).await?;
    let drive_id = item
        .parent_reference
        .as_ref()
        .and_then(|value| value.drive_id.as_deref())
        .ok_or_else(|| AppError::Message("SharePoint drive information is missing.".into()))?;
    let content_url = format!(
        "{GRAPH_ROOT}/drives/{}/items/{}/content",
        graph_segment(drive_id),
        graph_segment(&item.id)
    );
    diagnostics::info("sharepoint", &format!("Downloading workbook {}", item.name));
    let response = state
        .http
        .get(&content_url)
        .bearer_auth(token)
        .send()
        .await
        .context("Unable to download the SharePoint tracker")?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::Message(format!(
            "SharePoint could not download the tracker ({status})."
        )));
    }
    let bytes = response
        .bytes()
        .await
        .context("Unable to read the downloaded SharePoint tracker")?;
    if bytes.len() > SIMPLE_UPLOAD_LIMIT {
        return Err(AppError::Message(
            "This SharePoint workbook is larger than Atlas's 250 MB safe upload limit.".into(),
        ));
    }
    let extension = PathBuf::from(&item.name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("xlsx")
        .to_ascii_lowercase();
    let temp = TemporaryWorkbook(std::env::temp_dir().join(format!(
        "atlas-sharepoint-{}.{}",
        uuid::Uuid::new_v4(),
        extension
    )));
    fs::write(&temp.0, &bytes).context("Unable to stage the SharePoint tracker locally")?;
    let mut result = excel::export(
        temp.0.to_string_lossy().as_ref(),
        true,
        profile,
        interactions,
    )?;
    let updated = fs::read(&temp.0).context("Unable to read the updated tracker for upload")?;
    diagnostics::info("sharepoint", &format!("Uploading workbook {}", item.name));
    let mut request = state
        .http
        .put(&content_url)
        .bearer_auth(token)
        .header(CONTENT_TYPE, "application/octet-stream")
        .body(updated);
    if let Some(etag) = item.e_tag.as_deref() {
        request = request.header(IF_MATCH, etag);
    }
    let response = request
        .send()
        .await
        .context("Unable to upload the updated SharePoint tracker")?;
    let status = response.status();
    if status == reqwest::StatusCode::PRECONDITION_FAILED {
        return Err(AppError::Message(
            "The SharePoint tracker changed while Atlas was updating it. Nothing was overwritten; sync again to use the newest version.".into(),
        ));
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Message(format!(
            "SharePoint rejected the updated tracker ({status}): {}",
            body.chars().take(500).collect::<String>()
        )));
    }
    result.path = normalized;
    diagnostics::info(
        "sharepoint",
        &format!("Workbook {} updated successfully", item.name),
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_sharepoint_excel_links() {
        let url =
            "https://tenant-my.sharepoint.com/:x:/r/personal/user/Documents/Tracker.xlsm?web=1";
        assert_eq!(normalize_url(url).unwrap(), url);
    }

    #[test]
    fn rejects_non_sharepoint_destinations() {
        assert!(normalize_url("https://example.com/Tracker.xlsm").is_err());
        assert!(normalize_url("file:///C:/Tracker.xlsm").is_err());
    }

    #[test]
    fn sharing_tokens_use_unpadded_base64url() {
        let token = sharing_token("https://tenant.sharepoint.com/file.xlsx?a=1");
        assert!(token.starts_with("u!"));
        assert!(!token.contains('='));
        assert!(!token.contains('+'));
        assert!(!token.contains('/'));
    }
}
