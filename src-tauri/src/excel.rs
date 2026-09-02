use crate::{
    error::{AppError, Context, Result},
    models::{ExportResult, Interaction, SourceKind, UserProfile},
};
use chrono::{DateTime, Local, NaiveDate};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use umya_spreadsheet::{Color, Spreadsheet, Style, Worksheet};
use uuid::Uuid;

const HEADERS: &[&str] = &[
    "Interaction",
    "Login ID (Corp ID)",
    "Reception Date/Time",
    "Interaction Date/Time",
    "Resolution Date/Time",
    "Area",
    "Type of Client",
    "End Client",
    "Status",
    "Resolution Type",
    "Category",
    "Subcategory",
    "Priority",
    "Incident Number",
    "Comments",
    "CSA Name",
    "Capgemini Team Lead",
    "Circana Manager",
    "MTTR",
    "Resolution time",
    "IR Time",
    "_source_id",
];

const ORANGE_FORMULA_COLUMNS: &[&str] = &[
    "csaname",
    "capgeminiteamlead",
    "circanamanager",
    "mttr",
    "resolutiontime",
    "irtime",
];

fn normalized(value: &str) -> String {
    value
        .chars()
        .filter(|v| v.is_ascii_alphanumeric())
        .flat_map(|v| v.to_lowercase())
        .collect()
}

fn sheet_name(profile: &UserProfile) -> String {
    let clean: String = profile
        .full_name
        .chars()
        .map(|v| if "[]:*?/\\".contains(v) { ' ' } else { v })
        .collect();
    clean.trim().chars().take(31).collect::<String>()
}

fn resolve_sheet(book: &Spreadsheet, profile: &UserProfile) -> Result<String> {
    let shortened = sheet_name(profile);
    let wanted = [
        profile.full_name.trim(),
        profile.login_id.trim(),
        shortened.as_str(),
    ]
    .into_iter()
    .map(normalized)
    .collect::<Vec<_>>();
    let mut matches = book
        .get_sheet_collection()
        .iter()
        .filter(|sheet| wanted.contains(&normalized(sheet.get_name())));
    let first = matches.next().map(|v| v.get_name().to_string());
    if matches.next().is_some() {
        return Err(AppError::Message("More than one worksheet matches this profile. Rename the intended tab to the exact full name or Login ID.".into()));
    }
    first.ok_or_else(|| {
        let available = book
            .get_sheet_collection()
            .iter()
            .map(|v| v.get_name())
            .collect::<Vec<_>>()
            .join(", ");
        AppError::Message(format!(
            "No worksheet matches '{}' or '{}'. Available tabs: {available}",
            profile.full_name, profile.login_id
        ))
    })
}

fn find_headers(sheet: &Worksheet) -> Result<(u32, HashMap<String, u32>)> {
    let max_col = sheet.get_highest_column().max(1);
    for row in 1..=sheet.get_highest_row().max(1).min(40) {
        let mut map = HashMap::new();
        for col in 1..=max_col {
            let value = sheet.get_value((col, row));
            if !value.trim().is_empty() {
                map.insert(normalized(&value), col);
            }
        }
        if map.contains_key("interaction")
            && (map.contains_key("loginidcorpid")
                || map.contains_key("loginid")
                || map.contains_key("corpid"))
            && map.contains_key("interactiondatetime")
        {
            if let Some(value) = map.get("loginid").or_else(|| map.get("corpid")).copied() {
                map.insert("loginidcorpid".into(), value);
            }
            return Ok((row, map));
        }
    }
    Err(AppError::Message(
        "The selected worksheet does not contain the Circana tracker headers in its first 40 rows."
            .into(),
    ))
}

fn ensure_required_headers(headers: &HashMap<String, u32>) -> Result<()> {
    let required = [
        "interaction",
        "loginidcorpid",
        "receptiondatetime",
        "interactiondatetime",
        "resolutiondatetime",
        "area",
        "typeofclient",
        "endclient",
        "status",
        "resolutiontype",
        "category",
        "subcategory",
        "priority",
        "incidentnumber",
        "comments",
    ];
    let missing: Vec<_> = required
        .into_iter()
        .filter(|v| !headers.contains_key(*v))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(AppError::Message(format!(
            "The selected tracker tab is missing required columns: {}",
            missing.join(", ")
        )))
    }
}

fn ensure_source_column(
    sheet: &mut Worksheet,
    header_row: u32,
    headers: &mut HashMap<String, u32>,
) -> u32 {
    if let Some(col) = headers.get("sourceid").copied() {
        return col;
    }
    let col = sheet.get_highest_column() + 1;
    sheet
        .get_cell_mut((col, header_row))
        .set_value("_source_id");
    sheet
        .get_column_dimension_by_number_mut(&col)
        .set_hidden(true);
    headers.insert("sourceid".into(), col);
    col
}

fn validate_exportable(item: &Interaction) -> Result<()> {
    if !item.selected {
        return Ok(());
    }
    match item.source_kind {
        SourceKind::Calendar if !item.source_id.starts_with("graph:calendar:") => Err(AppError::Message("Rejected a calendar row without a real Graph calendar source ID.".into())),
        SourceKind::Email if !item.source_id.starts_with("graph:mail:") || !item.reviewed => Err(AppError::Message("Rejected an email row that was not explicitly reviewed or lacks a real Graph source ID.".into())),
        SourceKind::TeamsChat => Err(AppError::Message("AI-suggested Teams rows cannot be exported directly. Type the real interaction into the blank manual form first.".into())),
        SourceKind::Manual => {
            let id = item.source_id.strip_prefix("manual:").ok_or_else(|| AppError::Message("Rejected a manual row without a manual source ID.".into()))?;
            Uuid::parse_str(id).context("Rejected a manual row with an invalid source ID")?;
            if !item.manual_authored || !item.reviewed || item.comments.trim().is_empty() { return Err(AppError::Message("Rejected a manual row because it was not authored and completed by the user.".into())); }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn excel_serial(value: &str) -> Result<f64> {
    let utc = DateTime::parse_from_rfc3339(value)
        .context("An interaction contains an invalid date/time")?;
    let local = utc.with_timezone(&Local).naive_local();
    let base = NaiveDate::from_ymd_opt(1899, 12, 30)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let duration = local - base;
    Ok(duration.num_seconds() as f64 / 86_400.0
        + duration.subsec_nanos() as f64 / 86_400_000_000_000.0)
}

fn set_text(
    sheet: &mut Worksheet,
    headers: &HashMap<String, u32>,
    key: &str,
    row: u32,
    value: &str,
) {
    if let Some(col) = headers.get(key) {
        sheet.get_cell_mut((*col, row)).set_value_string(value);
    }
}

fn set_datetime(
    sheet: &mut Worksheet,
    headers: &HashMap<String, u32>,
    key: &str,
    row: u32,
    value: Option<&str>,
) -> Result<()> {
    if let Some(col) = headers.get(key) {
        let cell = sheet.get_cell_mut((*col, row));
        match value.filter(|v| !v.is_empty()) {
            Some(value) => {
                cell.set_value_number(excel_serial(value)?);
                cell.get_style_mut()
                    .get_number_format_mut()
                    .set_format_code("yyyy-mm-dd hh:mm");
            }
            None => {
                cell.set_blank();
            }
        }
    }
    Ok(())
}

fn write_row(
    sheet: &mut Worksheet,
    headers: &HashMap<String, u32>,
    row: u32,
    profile: &UserProfile,
    item: &Interaction,
) -> Result<()> {
    // These are the only columns this application is permitted to write. Orange formula columns are intentionally absent.
    set_text(sheet, headers, "interaction", row, &item.interaction_type);
    set_text(sheet, headers, "loginidcorpid", row, &profile.login_id);
    set_datetime(
        sheet,
        headers,
        "receptiondatetime",
        row,
        Some(&item.reception_date_time),
    )?;
    set_datetime(
        sheet,
        headers,
        "interactiondatetime",
        row,
        Some(&item.interaction_date_time),
    )?;
    set_datetime(
        sheet,
        headers,
        "resolutiondatetime",
        row,
        item.resolution_date_time.as_deref(),
    )?;
    set_text(sheet, headers, "area", row, &profile.area);
    set_text(sheet, headers, "typeofclient", row, &item.client_type);
    set_text(sheet, headers, "endclient", row, &item.end_client);
    set_text(sheet, headers, "status", row, &item.status);
    set_text(sheet, headers, "resolutiontype", row, &item.resolution_type);
    set_text(sheet, headers, "category", row, &item.category);
    set_text(sheet, headers, "subcategory", row, &item.subcategory);
    set_text(sheet, headers, "priority", row, &item.priority);
    set_text(sheet, headers, "incidentnumber", row, &item.incident_number);
    set_text(sheet, headers, "comments", row, &item.comments);
    set_text(sheet, headers, "sourceid", row, &item.source_id);
    Ok(())
}

fn row_has_user_data(sheet: &Worksheet, headers: &HashMap<String, u32>, row: u32) -> bool {
    [
        "interaction",
        "loginidcorpid",
        "receptiondatetime",
        "interactiondatetime",
        "comments",
    ]
    .iter()
    .filter_map(|v| headers.get(*v))
    .any(|col| !sheet.get_value((*col, row)).trim().is_empty())
}

fn next_safe_row(sheet: &Worksheet, headers: &HashMap<String, u32>, header_row: u32) -> u32 {
    for row in (header_row + 1)..=sheet.get_highest_row().max(header_row + 1) {
        let source_empty = headers
            .get("sourceid")
            .is_none_or(|col| sheet.get_value((*col, row)).trim().is_empty());
        if source_empty && !row_has_user_data(sheet, headers, row) {
            return row;
        }
    }
    sheet.get_highest_row().max(header_row) + 1
}

fn initialize_new(book: &mut Spreadsheet, profile: &UserProfile) -> Result<String> {
    let name = sheet_name(profile);
    let sheet = book
        .get_sheet_by_name_mut("Sheet1")
        .ok_or_else(|| AppError::Message("Unable to initialize a new workbook.".into()))?;
    sheet.set_name(&name);
    for (i, header) in HEADERS.iter().enumerate() {
        sheet
            .get_cell_mut(((i + 1) as u32, 1))
            .set_value_string(*header);
    }
    let mut header_style = Style::default();
    header_style.set_background_color("0F4F45");
    header_style
        .get_font_mut()
        .set_bold(true)
        .get_color_mut()
        .set_argb(Color::COLOR_WHITE);
    let mut orange_style = Style::default();
    orange_style.set_background_color("F4B183");
    orange_style.get_font_mut().set_bold(true);
    for (i, header) in HEADERS.iter().enumerate() {
        let col = (i + 1) as u32;
        sheet.get_cell_mut((col, 1)).set_style(
            if ORANGE_FORMULA_COLUMNS.contains(&normalized(header).as_str()) {
                orange_style.clone()
            } else {
                header_style.clone()
            },
        );
        sheet
            .get_column_dimension_by_number_mut(&col)
            .set_width(if *header == "Comments" {
                44.0
            } else if header.contains("Date/Time") {
                21.0
            } else {
                18.0
            });
    }
    let helper = HEADERS.len() as u32;
    sheet
        .get_column_dimension_by_number_mut(&helper)
        .set_hidden(true);
    Ok(name)
}

fn save_safely(book: &Spreadsheet, path: &Path, existing: bool) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Message("Choose a valid output location.".into()))?;
    fs::create_dir_all(parent).context("Unable to create the output directory")?;
    let temp = parent.join(format!(".atlas-{}.tmp", Uuid::new_v4()));
    umya_spreadsheet::writer::xlsx::write(book, &temp)
        .context("Unable to write the Excel workbook")?;
    let backup = parent.join(format!(".atlas-{}.backup", Uuid::new_v4()));
    if existing {
        fs::copy(path, &backup)
            .context("Unable to create a safety copy before updating the workbook")?;
    }
    let copy_result = fs::copy(&temp, path)
        .context("Unable to replace the workbook. Close it in Excel and try again");
    let _ = fs::remove_file(&temp);
    if let Err(error) = copy_result {
        if existing && backup.exists() {
            let _ = fs::copy(&backup, path);
        }
        return Err(error);
    }
    if backup.exists() {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

pub fn export(
    path: &str,
    existing: bool,
    profile: &UserProfile,
    interactions: &[Interaction],
) -> Result<ExportResult> {
    if profile.login_id.trim().is_empty() || profile.full_name.trim().is_empty() {
        return Err(AppError::Message(
            "Complete your profile before exporting.".into(),
        ));
    }
    let path = PathBuf::from(path);
    let extension = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if existing && !matches!(extension.as_str(), "xlsx" | "xlsm") {
        return Err(AppError::Message(
            "Existing tracker files must be .xlsx or .xlsm.".into(),
        ));
    }
    if !existing && extension != "xlsx" {
        return Err(AppError::Message(
            "New tracker files must use the .xlsx extension.".into(),
        ));
    }
    if existing && !path.is_file() {
        return Err(AppError::Message(
            "The selected existing workbook no longer exists.".into(),
        ));
    }
    for item in interactions {
        validate_exportable(item)?;
    }
    let selected: Vec<_> = interactions.iter().filter(|v| v.selected).collect();
    if selected.is_empty() {
        return Err(AppError::Message(
            "Select at least one verified interaction to export.".into(),
        ));
    }

    let mut book = if existing {
        umya_spreadsheet::reader::xlsx::read(&path)
            .context("Unable to open the existing workbook. Close it in Excel and try again")?
    } else {
        umya_spreadsheet::new_file()
    };
    let target_name = if existing {
        resolve_sheet(&book, profile)?
    } else {
        initialize_new(&mut book, profile)?
    };
    let sheet = book.get_sheet_by_name_mut(&target_name).ok_or_else(|| {
        AppError::Message("The target worksheet disappeared while exporting.".into())
    })?;
    let (header_row, mut headers) = find_headers(sheet)?;
    ensure_required_headers(&headers)?;
    let source_col = ensure_source_column(sheet, header_row, &mut headers);
    let mut existing_sources = HashMap::new();
    for row in (header_row + 1)..=sheet.get_highest_row() {
        let id = sheet.get_value((source_col, row));
        if !id.trim().is_empty() {
            existing_sources.insert(id.to_string(), row);
        }
    }
    let mut inserted = 0;
    let mut updated = 0;
    let mut skipped = 0;
    for item in selected {
        if let Some(row) = existing_sources.get(&item.source_id).copied() {
            if item.source_kind == SourceKind::Manual {
                skipped += 1;
                continue;
            }
            write_row(sheet, &headers, row, profile, item)?;
            updated += 1;
        } else {
            let row = next_safe_row(sheet, &headers, header_row);
            if row > header_row + 1 && row > sheet.get_highest_row() {
                let last_col = sheet.get_highest_column();
                sheet.copy_row_styling(&(row - 1), &row, Some(&1), Some(&last_col));
            }
            write_row(sheet, &headers, row, profile, item)?;
            existing_sources.insert(item.source_id.clone(), row);
            inserted += 1;
        }
    }
    save_safely(&book, &path, existing)?;
    Ok(ExportResult {
        path: path.to_string_lossy().to_string(),
        inserted,
        updated,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> UserProfile {
        UserProfile {
            login_id: "ABC123".into(),
            full_name: "Test User".into(),
            area: "Manufacturing".into(),
            team_lead: "Never written".into(),
            circana_manager: "Never written".into(),
        }
    }
    fn calendar() -> Interaction {
        Interaction {
            source_kind: SourceKind::Calendar,
            source_id: "graph:calendar:event-1".into(),
            interaction_type: "Meeting".into(),
            reception_date_time: "2026-09-01T14:00:00Z".into(),
            interaction_date_time: "2026-09-01T14:00:00Z".into(),
            resolution_date_time: Some("2026-09-01T14:30:00Z".into()),
            client_type: "Circana".into(),
            end_client: "".into(),
            status: "Resolved".into(),
            resolution_type: "Processed & Resolved".into(),
            category: "".into(),
            subcategory: "".into(),
            priority: "Intermediate".into(),
            incident_number: "".into(),
            comments: "Real event".into(),
            selected: true,
            reviewed: true,
            manual_authored: false,
            ai_suggested: false,
            evidence_label: "Real event".into(),
        }
    }

    #[test]
    fn rejects_unreviewed_email() {
        let mut v = calendar();
        v.source_kind = SourceKind::Email;
        v.source_id = "graph:mail:1".into();
        v.reviewed = false;
        assert!(validate_exportable(&v).is_err());
    }
    #[test]
    fn rejects_ai_rows_as_direct_export_sources() {
        let mut v = calendar();
        v.source_kind = SourceKind::TeamsChat;
        v.source_id = "graph:teams:evidence".into();
        v.ai_suggested = true;
        assert!(validate_exportable(&v).is_err());
    }
    #[test]
    fn duplicate_source_is_updated_not_inserted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Tracker.xlsx");
        let first = export(path.to_str().unwrap(), false, &profile(), &[calendar()]).unwrap();
        assert_eq!(first.inserted, 1);
        let second = export(path.to_str().unwrap(), true, &profile(), &[calendar()]).unwrap();
        assert_eq!(second.inserted, 0);
        assert_eq!(second.updated, 1);
        let book = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
        let sheet = book.get_sheet_by_name("Test User").unwrap();
        assert_eq!(sheet.get_value((22, 2)), "graph:calendar:event-1");
    }
    #[test]
    fn orange_columns_are_never_populated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Tracker.xlsx");
        export(path.to_str().unwrap(), false, &profile(), &[calendar()]).unwrap();
        let book = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
        let sheet = book.get_sheet_by_name("Test User").unwrap();
        for col in 16..=21 {
            assert!(sheet.get_value((col, 2)).is_empty());
        }
    }
}
