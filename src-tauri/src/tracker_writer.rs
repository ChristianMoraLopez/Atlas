use crate::{
    error::{AppError, Result},
    models::{ExportResult, Interaction, UserProfile},
};
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

const OFFICE_SCRIPT: &[u8] =
    include_bytes!("../../power-automate-writer/office-script/Atlas Write Tracker.osts");

pub struct PreparedWriter {
    pub package_path: String,
}

fn bridge_root(bridge_folder: &str) -> Result<PathBuf> {
    let inbox = fs::canonicalize(bridge_folder).map_err(|_| {
        AppError::Message("The configured AtlasBridge inbox is unavailable.".into())
    })?;
    let bridge = inbox
        .parent()
        .ok_or_else(|| AppError::Message("The AtlasBridge inbox has no parent folder.".into()))?;
    let root = bridge.parent().ok_or_else(|| {
        AppError::Message("The AtlasBridge folder is not inside a OneDrive root.".into())
    })?;
    Ok(root.to_path_buf())
}

fn flow_target(sharing_url: &str) -> Result<(String, String)> {
    let url = url::Url::parse(sharing_url)
        .map_err(|_| AppError::Message("Paste a valid SharePoint workbook link.".into()))?;
    let host = url
        .host_str()
        .ok_or_else(|| AppError::Message("The SharePoint workbook link has no host.".into()))?;
    if url.scheme() != "https" || !host.ends_with("sharepoint.com") {
        return Err(AppError::Message(
            "The cloud writer requires an HTTPS SharePoint workbook link.".into(),
        ));
    }
    let segments = url
        .path_segments()
        .map(|segments| segments.collect::<Vec<_>>())
        .unwrap_or_default();
    let layouts = segments
        .iter()
        .position(|segment| *segment == "_layouts")
        .ok_or_else(|| {
            AppError::Message(
                "Use the workbook link copied from Excel Online or SharePoint.".into(),
            )
        })?;
    if layouts == 0 {
        return Err(AppError::Message(
            "The SharePoint site path could not be read from this link.".into(),
        ));
    }
    let site_segments = if segments
        .first()
        .is_some_and(|segment| segment.starts_with(':'))
        && segments.get(1) == Some(&"r")
    {
        &segments[2..layouts]
    } else {
        &segments[..layouts]
    };
    if site_segments.is_empty() {
        return Err(AppError::Message(
            "The SharePoint site path could not be read from this link.".into(),
        ));
    }
    let site_path = site_segments.join("/");
    let site = format!("https://{host}/{site_path}");
    let sourcedoc = url
        .query_pairs()
        .find(|(name, _)| name.eq_ignore_ascii_case("sourcedoc"))
        .map(|(_, value)| value.into_owned())
        .ok_or_else(|| {
            AppError::Message(
                "The SharePoint link does not contain the workbook sourcedoc identifier.".into(),
            )
        })?;
    let id = sourcedoc.trim().trim_matches(['{', '}']);
    let parsed = uuid::Uuid::parse_str(id)
        .map_err(|_| AppError::Message("The SharePoint workbook identifier is invalid.".into()))?;
    Ok((site, parsed.to_string()))
}

/// The writer solution is personalized like every Atlas solution: unique
/// name, connection references and workflow id derive from the tracker
/// installation id and carry the owner name, so colleagues in the same
/// environment never overwrite each other.
fn personalized_solution(
    site: &str,
    file_id: &str,
    installation_id: &str,
    owner: Option<&str>,
) -> Result<Vec<u8>> {
    let site = site.to_string();
    let file_id = file_id.to_string();
    crate::connector_installer::personalized_package(
        &crate::connector_installer::WRITER,
        installation_id,
        owner,
        &move |flow: &mut Value| {
            let actions = &mut flow["properties"]["definition"]["actions"];
            for (action, value) in [("AtlasTargetSite", &site), ("AtlasTargetFileId", &file_id)] {
                actions
                    .get_mut(action)
                    .ok_or_else(|| {
                        AppError::Message("The bundled tracker writer workflow is invalid.".into())
                    })?["inputs"] = Value::String(value.clone());
            }
            Ok(())
        },
    )
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Message("The output file has no parent folder.".into()))?;
    fs::create_dir_all(parent).map_err(|error| {
        AppError::Message(format!("Unable to create {}: {error}", parent.display()))
    })?;
    let pending = parent.join(format!(".atlas-{}.pending", uuid::Uuid::new_v4()));
    fs::write(&pending, bytes).map_err(|error| {
        AppError::Message(format!("Unable to write {}: {error}", path.display()))
    })?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| {
            AppError::Message(format!("Unable to replace {}: {error}", path.display()))
        })?;
    }
    fs::rename(&pending, path).map_err(|error| {
        AppError::Message(format!("Unable to publish {}: {error}", path.display()))
    })
}

pub fn prepare(
    config_dir: &Path,
    bridge_folder: &str,
    sharing_url: &str,
    installation_id: &str,
    owner: Option<&str>,
) -> Result<PreparedWriter> {
    let (site, file_id) = flow_target(sharing_url)?;
    let root = bridge_root(bridge_folder)?;
    let outbox = root.join("AtlasBridge").join("tracker-outbox");
    fs::create_dir_all(&outbox).map_err(|error| {
        AppError::Message(format!("Unable to create the tracker outbox: {error}"))
    })?;
    let script_path = root
        .join("Documents")
        .join("Office Scripts")
        .join("Atlas Write Tracker.osts");
    write_atomic(&script_path, OFFICE_SCRIPT)?;
    let writer_dir = config_dir.join("tracker-writer");
    let package_path = writer_dir.join("AtlasTrackerWriter_1_0_0_0.zip");
    write_atomic(
        &package_path,
        &personalized_solution(&site, &file_id, installation_id, owner)?,
    )?;
    Ok(PreparedWriter {
        package_path: package_path.to_string_lossy().into_owned(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackerPayload<'a> {
    schema_version: u8,
    export_id: String,
    generated_at: String,
    timezone_offset_minutes: i32,
    date: &'a str,
    profile: &'a UserProfile,
    interactions: Vec<&'a Interaction>,
}

pub fn queue(
    bridge_folder: &str,
    sharing_url: &str,
    date: &str,
    profile: &UserProfile,
    interactions: &[Interaction],
) -> Result<ExportResult> {
    let selected = interactions
        .iter()
        .filter(|interaction| interaction.selected)
        .collect::<Vec<_>>();
    let id = uuid::Uuid::new_v4().to_string();
    let payload = TrackerPayload {
        schema_version: 1,
        export_id: id.clone(),
        generated_at: Utc::now().to_rfc3339(),
        timezone_offset_minutes: chrono::Local::now().offset().local_minus_utc() / 60,
        date,
        profile,
        interactions: selected.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&payload)?;
    let root = bridge_root(bridge_folder)?;
    let outbox = root.join("AtlasBridge").join("tracker-outbox");
    let history = outbox
        .join(date)
        .join(format!("atlas-tracker-{date}-{id}.json"));
    write_atomic(&history, &bytes)?;
    write_atomic(&outbox.join("current.json"), &bytes)?;
    Ok(ExportResult {
        path: sharing_url.into(),
        inserted: selected.len(),
        updated: 0,
        skipped: interactions.len().saturating_sub(selected.len()),
        queued: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_excel_online_link() {
        let (site, id) = flow_target("https://example-my.sharepoint.com/:x:/r/personal/user_example_com/_layouts/15/doc2.aspx?sourcedoc=%7B665EB967-BE4F-4BB8-BF7C-135380354E21%7D&file=Tracker.xlsm").unwrap();
        assert_eq!(
            site,
            "https://example-my.sharepoint.com/personal/user_example_com"
        );
        assert_eq!(id, "665eb967-be4f-4bb8-bf7c-135380354e21");
    }

    #[test]
    fn writer_package_is_personalized_per_installation_and_owner() {
        let first = personalized_solution(
            "https://example.sharepoint.com/sites/team",
            "665eb967-be4f-4bb8-bf7c-135380354e21",
            "user_test_a",
            Some("Christian Mora"),
        )
        .unwrap();
        let second = personalized_solution(
            "https://example.sharepoint.com/sites/team",
            "665eb967-be4f-4bb8-bf7c-135380354e21",
            "user_test_b",
            Some("Ana Test"),
        )
        .unwrap();
        let read = |bytes: &[u8], name: &str| {
            use std::io::Read;
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut text = String::new();
            zip.by_name(name).unwrap().read_to_string(&mut text).unwrap();
            text
        };
        let solution = read(&first[..], "solution.xml");
        assert!(solution.contains("<UniqueName>AtlasTrackerWriter_ChristianMora_user_test_a</UniqueName>"));
        assert!(solution.contains("Atlas Tracker Writer - Christian Mora - user_tes"));
        assert!(read(&second[..], "solution.xml").contains("AtlasTrackerWriter_AnaTest_user_test_b"));
        let customizations = read(&first[..], "customizations.xml");
        assert!(customizations.contains("atlas_writer_excel_user_test_a"));
        assert!(customizations.contains("Atlas - Write daily tracker to SharePoint (Christian Mora)"));
        assert!(!customizations.contains("4d6c39b7-cac8-4d19-a12e-95af49503b7f"));
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(first)).unwrap();
        let workflow = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .find(|name| name.starts_with("Workflows/"))
            .unwrap();
        let mut text = String::new();
        {
            use std::io::Read;
            zip.by_name(&workflow).unwrap().read_to_string(&mut text).unwrap();
        }
        let flow: Value = serde_json::from_str(&text).unwrap();
        let actions = &flow["properties"]["definition"]["actions"];
        assert_eq!(actions["AtlasTargetSite"]["inputs"], "https://example.sharepoint.com/sites/team");
        assert_eq!(actions["AtlasTargetFileId"]["inputs"], "665eb967-be4f-4bb8-bf7c-135380354e21");
        assert!(actions.get("AtlasInstallationId").is_none());
    }

    #[test]
    fn rejects_link_without_file_id() {
        assert!(flow_target("https://example.sharepoint.com/sites/team/Tracker.xlsx").is_err());
    }
}
