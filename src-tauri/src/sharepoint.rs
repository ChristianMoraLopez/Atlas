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
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
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

pub fn workbook_name(value: &str) -> Result<String> {
    let normalized = normalize_url(value)?;
    let url = Url::parse(&normalized).context("Enter a valid SharePoint sharing link")?;
    let query_name = url
        .query_pairs()
        .find(|(key, _)| key.eq_ignore_ascii_case("file"))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let path_name = url
        .path_segments()
        .and_then(|segments| segments.last())
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("doc2.aspx"))
        .map(str::to_string);
    let name = query_name.or(path_name).ok_or_else(|| {
        AppError::Message("The SharePoint link does not identify an Excel workbook.".into())
    })?;
    let extension = Path::new(&name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "xlsx" | "xlsm") {
        return Err(AppError::Message(
            "The SharePoint link must identify an .xlsx or .xlsm workbook.".into(),
        ));
    }
    Ok(name)
}

fn one_drive_roots(bridge_folder: Option<&str>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for variable in ["OneDriveCommercial", "OneDrive"] {
        if let Some(path) = std::env::var_os(variable)
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
        {
            roots.push(path);
        }
    }
    if let Some(root) = bridge_folder
        .map(Path::new)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|path| path.is_dir())
    {
        roots.push(root.to_path_buf());
    }
    let mut seen = HashSet::new();
    roots.retain(|path| seen.insert(path.to_string_lossy().to_ascii_lowercase()));
    roots
}

fn matching_files(root: &Path, wanted: &str) -> Vec<PathBuf> {
    let mut matches = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    let mut inspected = 0usize;
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            inspected += 1;
            if inspected > 100_000 {
                return matches;
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() && !kind.is_symlink() {
                pending.push(path);
            } else if kind.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(wanted)
            {
                matches.push(path);
            }
        }
    }
    matches
}

pub fn resolve_synced_copy(
    sharing_url: &str,
    selected_path: Option<&str>,
    bridge_folder: Option<&str>,
) -> Result<String> {
    let wanted = workbook_name(sharing_url)?;
    if let Some(selected) = selected_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let path = Path::new(selected);
        if !path.is_file() {
            return Err(AppError::Message(
                "Choose the locally synced copy of the SharePoint workbook.".into(),
            ));
        }
        let name_matches = path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case(&wanted));
        if !name_matches {
            return Err(AppError::Message(format!(
                "The selected file must be the synced copy of {wanted}."
            )));
        }
        return Ok(path.to_string_lossy().into_owned());
    }

    let mut matches = one_drive_roots(bridge_folder)
        .iter()
        .flat_map(|root| matching_files(root, &wanted))
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    match matches.as_slice() {
        [path] => Ok(path.to_string_lossy().into_owned()),
        [] => Err(AppError::Message(format!(
            "Atlas no encontró {wanted} en OneDrive. Abre el enlace, agrega un acceso directo a 'Mis archivos' y espera la sincronización, o selecciona la copia local durante el setup."
        ))),
        _ => Err(AppError::Message(format!(
            "Atlas encontró varias copias de {wanted}. Selecciona la copia sincronizada correcta durante el setup."
        ))),
    }
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

    #[test]
    fn extracts_workbook_name_from_office_link() {
        let url = "https://tenant-my.sharepoint.com/:x:/r/personal/user/_layouts/15/doc2.aspx?sourcedoc=%7B1%7D&file=Daily%20Tracker.xlsm&web=1";
        assert_eq!(workbook_name(url).unwrap(), "Daily Tracker.xlsm");
    }
}
