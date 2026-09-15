use crate::{
    error::{AppError, Context, Result},
    models::{ExportResult, Interaction, SourceKind, UserProfile},
};
use chrono::{DateTime, Local, NaiveDate};
use quick_xml::{
    events::{BytesEnd, BytesStart, BytesText, Event},
    Reader, Writer,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};
use umya_spreadsheet::{Color, Spreadsheet, Style, Worksheet};
use uuid::Uuid;
use zip::{ZipArchive, ZipWriter};

const SOURCE_HEADER: &str = "_source_id";
const TRACKER_HEADERS: &[&str] = &[
    "Interaction",
    "Login ID (Corp ID)",
    "Reception Date/Time (mm/dd/yyyy hh:mm)",
    "Interaction Date/Time \n(mm/dd/yyyy hh:mm)",
    "Resolution Date/Time (mm/dd/yyyy hh:mm)",
    "Area",
    "Type of Client",
    "End Client",
    "Status",
    "Resolution Type",
    "Category",
    "Subcategory",
    "Priority",
    "Incident Number \n(If applies)",
    "Comments",
    "CSA Name",
    "Capgemini Team Lead",
    "Circana Manager",
    "MTTR",
    "Resolution time",
    "IR Time",
];

const ORANGE_FORMULA_COLUMNS: &[&str] = &[
    "csaname",
    "capgeminiteamlead",
    "circanamanager",
    "mttr",
    "resolutiontime",
    "irtime",
];

#[derive(Clone, Debug, PartialEq)]
enum PackageCellValue {
    Text(String),
    Date(f64),
    Blank,
}

type RowUpdates = BTreeMap<u32, BTreeMap<u32, PackageCellValue>>;

fn normalized(value: &str) -> String {
    value
        .chars()
        .filter(|v| v.is_ascii_alphanumeric())
        .flat_map(|v| v.to_lowercase())
        .collect()
}

fn canonical_header(value: &str) -> String {
    let value = normalized(value);
    for (prefix, canonical) in [
        ("receptiondatetime", "receptiondatetime"),
        ("interactiondatetime", "interactiondatetime"),
        ("resolutiondatetime", "resolutiondatetime"),
        ("incidentnumber", "incidentnumber"),
        ("loginidcorpid", "loginidcorpid"),
        ("sourceid", "sourceid"),
    ] {
        if value.starts_with(prefix) {
            return canonical.into();
        }
    }
    match value.as_str() {
        "loginid" | "corpid" => "loginidcorpid".into(),
        _ => value,
    }
}

fn sheet_name(profile: &UserProfile) -> String {
    let clean: String = profile
        .full_name
        .chars()
        .map(|v| if "[]:*?/\\".contains(v) { ' ' } else { v })
        .collect();
    clean.trim().chars().take(31).collect::<String>()
}

fn resolve_sheet(book: &Spreadsheet, profile: &UserProfile) -> Result<(usize, String)> {
    let shortened = sheet_name(profile);
    let wanted = [
        profile.full_name.trim(),
        profile.login_id.trim(),
        shortened.as_str(),
    ]
    .into_iter()
    .map(normalized)
    .collect::<Vec<_>>();
    let matches = book
        .get_sheet_collection()
        .iter()
        .enumerate()
        .filter(|(_, sheet)| wanted.contains(&normalized(sheet.get_name())))
        .map(|(index, sheet)| (index, sheet.get_name().to_string()))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [matched] => Ok(matched.clone()),
        [] => {
            let available = book
                .get_sheet_collection()
                .iter()
                .map(|v| v.get_name())
                .collect::<Vec<_>>()
                .join(", ");
            Err(AppError::Message(format!(
                "No worksheet matches '{}' or '{}'. Available tabs: {available}",
                profile.full_name, profile.login_id
            )))
        }
        _ => Err(AppError::Message("More than one worksheet matches this profile. Rename the intended tab to the exact full name or Login ID.".into())),
    }
}

fn find_headers(sheet: &Worksheet) -> Result<(u32, HashMap<String, u32>)> {
    let max_col = sheet.get_highest_column().max(1);
    for row in 1..=sheet.get_highest_row().max(1).min(40) {
        let mut map = HashMap::new();
        for col in 1..=max_col {
            let value = sheet.get_value((col, row));
            if !value.trim().is_empty() {
                map.insert(canonical_header(&value), col);
            }
        }
        if map.contains_key("interaction")
            && map.contains_key("loginidcorpid")
            && map.contains_key("interactiondatetime")
        {
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
        sheet
            .get_column_dimension_by_number_mut(&col)
            .set_hidden(true);
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
        SourceKind::Calendar
            if !item.source_id.starts_with("graph:calendar:")
                && !item.source_id.starts_with("bridge:calendar:") =>
        {
            Err(AppError::Message(
                "Rejected a calendar row without a verified calendar source ID.".into(),
            ))
        }
        SourceKind::Email
            if (!item.source_id.starts_with("graph:mail:")
                && !item.source_id.starts_with("bridge:mail:"))
                || !item.reviewed =>
        {
            Err(AppError::Message(
                "Rejected an email row that was not reviewed or lacks a verified mail source ID."
                    .into(),
            ))
        }
        SourceKind::TeamsChat
            if (!item.source_id.starts_with("graph:teams:")
                && !item.source_id.starts_with("bridge:teams:"))
                || !item.reviewed
                || item.comments.trim().is_empty() =>
        {
            Err(AppError::Message(
                "Rejected a Teams row that was not reviewed or lacks verified Teams evidence."
                    .into(),
            ))
        }
        SourceKind::Manual => {
            let id = item.source_id.strip_prefix("manual:").ok_or_else(|| {
                AppError::Message("Rejected a manual row without a manual source ID.".into())
            })?;
            Uuid::parse_str(id).context("Rejected a manual row with an invalid source ID")?;
            if !item.manual_authored || !item.reviewed || item.comments.trim().is_empty() {
                return Err(AppError::Message(
                    "Rejected a manual row because it was not authored and completed by the user."
                        .into(),
                ));
            }
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
                    .set_format_code("mm/dd/yyyy\\ hh:mm");
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

fn tracker_table_end(
    sheet: &Worksheet,
    headers: &HashMap<String, u32>,
    header_row: u32,
) -> Result<Option<u32>> {
    let first_col = headers["interaction"];
    let last_col = headers["comments"];
    let matches = sheet
        .get_tables()
        .iter()
        .filter_map(|table| {
            let (start, end) = table.get_area();
            ((*start.get_row_num() == header_row)
                && (*start.get_col_num() <= first_col)
                && (*end.get_col_num() >= last_col))
                .then_some((table.get_name(), *end.get_row_num()))
        })
        .collect::<Vec<_>>();
    if let Some((_, end)) = matches
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("Data"))
    {
        return Ok(Some(*end));
    }
    match matches.as_slice() {
        [] => Ok(None),
        [(_, end)] => Ok(Some(*end)),
        _ => Err(AppError::Message(
            "More than one Excel table contains the Circana tracker headers on this tab.".into(),
        )),
    }
}

fn next_safe_row(
    sheet: &Worksheet,
    headers: &HashMap<String, u32>,
    header_row: u32,
    table_end: Option<u32>,
    reserved: &HashSet<u32>,
) -> Result<u32> {
    let end = table_end.unwrap_or_else(|| sheet.get_highest_row().max(header_row + 1));
    for row in (header_row + 1)..=end {
        if reserved.contains(&row) {
            continue;
        }
        let source_empty = headers
            .get("sourceid")
            .is_none_or(|col| sheet.get_value((*col, row)).trim().is_empty());
        if source_empty && !row_has_user_data(sheet, headers, row) {
            return Ok(row);
        }
    }
    if table_end.is_some() {
        Err(AppError::Message(
            "The Circana Data table has no unused preformatted rows. Add blank rows to the Data table in Excel, then export again."
                .into(),
        ))
    } else {
        let mut row = end + 1;
        while reserved.contains(&row) {
            row += 1;
        }
        Ok(row)
    }
}

fn add_package_text(
    updates: &mut RowUpdates,
    headers: &HashMap<String, u32>,
    key: &str,
    row: u32,
    value: &str,
) {
    if let Some(col) = headers.get(key) {
        let value = if value.is_empty() {
            PackageCellValue::Blank
        } else {
            PackageCellValue::Text(value.to_string())
        };
        updates.entry(row).or_default().insert(*col, value);
    }
}

fn add_package_datetime(
    updates: &mut RowUpdates,
    headers: &HashMap<String, u32>,
    key: &str,
    row: u32,
    value: Option<&str>,
) -> Result<()> {
    if let Some(col) = headers.get(key) {
        let value = match value.filter(|v| !v.is_empty()) {
            Some(value) => PackageCellValue::Date(excel_serial(value)?),
            None => PackageCellValue::Blank,
        };
        updates.entry(row).or_default().insert(*col, value);
    }
    Ok(())
}

fn add_package_row(
    updates: &mut RowUpdates,
    headers: &HashMap<String, u32>,
    row: u32,
    profile: &UserProfile,
    item: &Interaction,
) -> Result<()> {
    // Only blue/user-owned columns and the hidden provenance helper are ever changed.
    add_package_text(updates, headers, "interaction", row, &item.interaction_type);
    add_package_text(updates, headers, "loginidcorpid", row, &profile.login_id);
    add_package_datetime(
        updates,
        headers,
        "receptiondatetime",
        row,
        Some(&item.reception_date_time),
    )?;
    add_package_datetime(
        updates,
        headers,
        "interactiondatetime",
        row,
        Some(&item.interaction_date_time),
    )?;
    add_package_datetime(
        updates,
        headers,
        "resolutiondatetime",
        row,
        item.resolution_date_time.as_deref(),
    )?;
    add_package_text(updates, headers, "area", row, &profile.area);
    add_package_text(updates, headers, "typeofclient", row, &item.client_type);
    add_package_text(updates, headers, "endclient", row, &item.end_client);
    add_package_text(updates, headers, "status", row, &item.status);
    add_package_text(
        updates,
        headers,
        "resolutiontype",
        row,
        &item.resolution_type,
    );
    add_package_text(updates, headers, "category", row, &item.category);
    add_package_text(updates, headers, "subcategory", row, &item.subcategory);
    add_package_text(updates, headers, "priority", row, &item.priority);
    add_package_text(
        updates,
        headers,
        "incidentnumber",
        row,
        &item.incident_number,
    );
    add_package_text(updates, headers, "comments", row, &item.comments);
    add_package_text(updates, headers, "sourceid", row, &item.source_id);
    Ok(())
}

fn local_name_is(start: &BytesStart<'_>, wanted: &[u8]) -> bool {
    start.local_name().as_ref() == wanted
}

fn attribute(start: &BytesStart<'_>, wanted: &[u8]) -> Result<Option<String>> {
    for value in start.attributes().with_checks(false) {
        let value = value.context("Unable to read an Excel XML attribute")?;
        if value.key.as_ref() == wanted {
            return Ok(Some(
                value
                    .unescape_value()
                    .context("Unable to decode an Excel XML attribute")?
                    .into_owned(),
            ));
        }
    }
    Ok(None)
}

fn rebuilt_start(
    start: &BytesStart<'_>,
    overrides: &[(&str, String)],
    removed: &[&str],
) -> Result<BytesStart<'static>> {
    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    let mut values = Vec::new();
    for value in start.attributes().with_checks(false) {
        let value = value.context("Unable to read an Excel XML attribute")?;
        let key = String::from_utf8_lossy(value.key.as_ref()).into_owned();
        if removed.contains(&key.as_str()) || overrides.iter().any(|(name, _)| *name == key) {
            continue;
        }
        let decoded = value
            .unescape_value()
            .context("Unable to decode an Excel XML attribute")?
            .into_owned();
        values.push((key, decoded));
    }
    values.extend(
        overrides
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone())),
    );
    let mut rebuilt = BytesStart::new(name);
    for (key, value) in &values {
        rebuilt.push_attribute((key.as_str(), value.as_str()));
    }
    Ok(rebuilt.into_owned())
}

fn column_name(mut column: u32) -> String {
    let mut result = String::new();
    while column > 0 {
        let value = ((column - 1) % 26) as u8;
        result.insert(0, (b'A' + value) as char);
        column = (column - 1) / 26;
    }
    result
}

fn coordinate_column(coordinate: &str) -> Option<u32> {
    let mut result = 0u32;
    let mut found = false;
    for value in coordinate.bytes() {
        if !value.is_ascii_alphabetic() {
            break;
        }
        found = true;
        result = result
            .checked_mul(26)?
            .checked_add((value.to_ascii_uppercase() - b'A' + 1) as u32)?;
    }
    found.then_some(result)
}

fn coordinate_row(coordinate: &str) -> Option<u32> {
    coordinate
        .trim_start_matches(|value: char| value.is_ascii_alphabetic() || value == '$')
        .parse()
        .ok()
}

fn worksheet_date_style(
    xml: &[u8],
    date_columns: &HashSet<u32>,
    header_row: u32,
) -> Result<Option<String>> {
    let mut reader = Reader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buffer)
            .context("Unable to inspect Excel date styles")?
        {
            Event::Start(start) | Event::Empty(start) if local_name_is(&start, b"c") => {
                let coordinate = attribute(&start, b"r")?.unwrap_or_default();
                if coordinate_row(&coordinate).is_some_and(|row| row > header_row)
                    && coordinate_column(&coordinate)
                        .is_some_and(|column| date_columns.contains(&column))
                {
                    if let Some(style) = attribute(&start, b"s")? {
                        return Ok(Some(style));
                    }
                }
            }
            Event::Eof => return Ok(None),
            _ => {}
        }
        buffer.clear();
    }
}

fn write_cell(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    row: u32,
    column: u32,
    value: &PackageCellValue,
    original: Option<&BytesStart<'_>>,
    date_style: Option<&str>,
) -> Result<()> {
    let coordinate = format!("{}{}", column_name(column), row);
    let base = original
        .map(|value| value.to_owned())
        .unwrap_or_else(|| BytesStart::new("c"));
    let cell = match value {
        PackageCellValue::Text(_) => rebuilt_start(
            &base,
            &[("r", coordinate), ("t", "inlineStr".into())],
            &["r", "t"],
        )?,
        PackageCellValue::Date(_) => {
            let needs_style = attribute(&base, b"s")?.is_none();
            let mut overrides = vec![("r", coordinate)];
            if needs_style {
                let style = date_style.ok_or_else(|| {
                    AppError::Message(
                        "Atlas could not find the tracker date format for a new row. Add one preformatted blank row to the Excel table and try again."
                            .into(),
                    )
                })?;
                overrides.push(("s", style.to_string()));
            }
            rebuilt_start(&base, &overrides, &["r", "t"])?
        }
        PackageCellValue::Blank => rebuilt_start(&base, &[("r", coordinate)], &["r", "t"])?,
    };
    if value == &PackageCellValue::Blank {
        writer
            .write_event(Event::Empty(cell))
            .context("Unable to write a blank tracker cell")?;
        return Ok(());
    }
    writer
        .write_event(Event::Start(cell))
        .context("Unable to start a tracker cell")?;
    match value {
        PackageCellValue::Text(value) => {
            writer
                .write_event(Event::Start(BytesStart::new("is")))
                .context("Unable to write a tracker text cell")?;
            let mut text = BytesStart::new("t");
            if value.trim() != value {
                text.push_attribute(("xml:space", "preserve"));
            }
            writer
                .write_event(Event::Start(text))
                .context("Unable to write a tracker text cell")?;
            writer
                .write_event(Event::Text(BytesText::new(value)))
                .context("Unable to write tracker text")?;
            writer
                .write_event(Event::End(BytesEnd::new("t")))
                .context("Unable to finish tracker text")?;
            writer
                .write_event(Event::End(BytesEnd::new("is")))
                .context("Unable to finish a tracker text cell")?;
        }
        PackageCellValue::Date(value) => {
            writer
                .write_event(Event::Start(BytesStart::new("v")))
                .context("Unable to write a tracker date cell")?;
            writer
                .write_event(Event::Text(BytesText::new(&value.to_string())))
                .context("Unable to write a tracker date")?;
            writer
                .write_event(Event::End(BytesEnd::new("v")))
                .context("Unable to finish a tracker date cell")?;
        }
        PackageCellValue::Blank => unreachable!(),
    }
    writer
        .write_event(Event::End(BytesEnd::new("c")))
        .context("Unable to finish a tracker cell")?;
    Ok(())
}

fn write_pending_before(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    row: u32,
    before_column: u32,
    pending: &mut BTreeMap<u32, PackageCellValue>,
    date_style: Option<&str>,
) -> Result<()> {
    let columns = pending
        .range(..before_column)
        .map(|(column, _)| *column)
        .collect::<Vec<_>>();
    for column in columns {
        let value = pending.remove(&column).expect("pending column disappeared");
        write_cell(writer, row, column, &value, None, date_style)?;
    }
    Ok(())
}

fn write_complete_row(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    row: u32,
    values: BTreeMap<u32, PackageCellValue>,
    date_style: Option<&str>,
) -> Result<()> {
    let last = values.keys().next_back().copied().unwrap_or(1);
    let mut start = BytesStart::new("row");
    let row_text = row.to_string();
    let spans = format!("1:{last}");
    start.push_attribute(("r", row_text.as_str()));
    start.push_attribute(("spans", spans.as_str()));
    writer
        .write_event(Event::Start(start))
        .context("Unable to insert an Excel row")?;
    for (column, value) in values {
        write_cell(writer, row, column, &value, None, date_style)?;
    }
    writer
        .write_event(Event::End(BytesEnd::new("row")))
        .context("Unable to finish an Excel row")?;
    Ok(())
}

fn extended_dimension(start: &BytesStart<'_>, source_column: u32) -> Result<BytesStart<'static>> {
    let Some(reference) = attribute(start, b"ref")? else {
        return Ok(start.to_owned());
    };
    let mut parts = reference.split(':');
    let first = parts.next().unwrap_or("A1");
    let last = parts.next().unwrap_or(first);
    let last_column = coordinate_column(last).unwrap_or(1);
    if last_column >= source_column {
        return Ok(start.to_owned());
    }
    let row = last.trim_start_matches(|value: char| value.is_ascii_alphabetic() || value == '$');
    let replacement = format!("{first}:{}{row}", column_name(source_column));
    rebuilt_start(start, &[("ref", replacement)], &["ref"])
}

fn extended_row(start: &BytesStart<'_>, source_column: u32) -> Result<BytesStart<'static>> {
    let Some(spans) = attribute(start, b"spans")? else {
        return Ok(start.to_owned());
    };
    let mut values = spans.split(':');
    let first = values
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(1);
    let last = values
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(first);
    if last >= source_column {
        return Ok(start.to_owned());
    }
    rebuilt_start(
        start,
        &[("spans", format!("{first}:{source_column}"))],
        &["spans"],
    )
}

fn hidden_column_start(
    start: &BytesStart<'_>,
    min: u32,
    max: u32,
    hidden: bool,
) -> Result<BytesStart<'static>> {
    let mut overrides = vec![("min", min.to_string()), ("max", max.to_string())];
    if hidden {
        overrides.push(("hidden", "1".into()));
        rebuilt_start(start, &overrides, &["min", "max", "hidden"])
    } else {
        rebuilt_start(start, &overrides, &["min", "max"])
    }
}

fn write_hidden_column_split(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    start: &BytesStart<'_>,
    source_column: u32,
) -> Result<bool> {
    let Some(min) = attribute(start, b"min")?.and_then(|value| value.parse::<u32>().ok()) else {
        writer
            .write_event(Event::Empty(start.to_owned()))
            .context("Unable to preserve an Excel column")?;
        return Ok(false);
    };
    let Some(max) = attribute(start, b"max")?.and_then(|value| value.parse::<u32>().ok()) else {
        writer
            .write_event(Event::Empty(start.to_owned()))
            .context("Unable to preserve an Excel column")?;
        return Ok(false);
    };
    if source_column < min || source_column > max {
        writer
            .write_event(Event::Empty(start.to_owned()))
            .context("Unable to preserve an Excel column")?;
        return Ok(false);
    }
    if min < source_column {
        writer
            .write_event(Event::Empty(hidden_column_start(
                start,
                min,
                source_column - 1,
                false,
            )?))
            .context("Unable to preserve an Excel column range")?;
    }
    writer
        .write_event(Event::Empty(hidden_column_start(
            start,
            source_column,
            source_column,
            true,
        )?))
        .context("Unable to hide the tracker provenance column")?;
    if source_column < max {
        writer
            .write_event(Event::Empty(hidden_column_start(
                start,
                source_column + 1,
                max,
                false,
            )?))
            .context("Unable to preserve an Excel column range")?;
    }
    Ok(true)
}

fn write_new_hidden_column(writer: &mut Writer<Cursor<Vec<u8>>>, source_column: u32) -> Result<()> {
    let mut column = BytesStart::new("col");
    let index = source_column.to_string();
    column.push_attribute(("min", index.as_str()));
    column.push_attribute(("max", index.as_str()));
    column.push_attribute(("width", "2"));
    column.push_attribute(("hidden", "1"));
    column.push_attribute(("customWidth", "1"));
    writer
        .write_event(Event::Empty(column))
        .context("Unable to create the hidden tracker provenance column")?;
    Ok(())
}

fn patch_worksheet_xml(
    xml: &[u8],
    mut updates: RowUpdates,
    source_column: u32,
    header_row: u32,
) -> Result<Vec<u8>> {
    let date_columns = updates
        .values()
        .flat_map(|values| values.iter())
        .filter_map(|(column, value)| matches!(value, PackageCellValue::Date(_)).then_some(*column))
        .collect::<HashSet<_>>();
    let date_style = worksheet_date_style(xml, &date_columns, header_row)?;
    updates
        .entry(header_row)
        .or_default()
        .insert(source_column, PackageCellValue::Text(SOURCE_HEADER.into()));

    let mut reader = Reader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Cursor::new(Vec::with_capacity(xml.len() + 4096)));
    let mut buffer = Vec::new();
    let mut in_columns = false;
    let mut saw_columns = false;
    let mut source_column_hidden = false;
    let mut in_sheet_data = false;
    let mut current_row = None;
    let mut pending = BTreeMap::new();
    let mut skip_cell_depth = 0usize;

    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .context("Unable to parse the target Excel worksheet")?;

        if skip_cell_depth > 0 {
            match &event {
                Event::Start(_) => skip_cell_depth += 1,
                Event::End(_) => skip_cell_depth -= 1,
                Event::Eof => {
                    return Err(AppError::Message(
                        "The target worksheet ended inside a cell.".into(),
                    ))
                }
                _ => {}
            }
            buffer.clear();
            continue;
        }

        match event {
            Event::Start(start) if local_name_is(&start, b"cols") => {
                saw_columns = true;
                in_columns = true;
                writer
                    .write_event(Event::Start(start.into_owned()))
                    .context("Unable to preserve Excel column metadata")?;
            }
            Event::End(end) if end.local_name().as_ref() == b"cols" => {
                if !source_column_hidden {
                    write_new_hidden_column(&mut writer, source_column)?;
                    source_column_hidden = true;
                }
                in_columns = false;
                writer
                    .write_event(Event::End(end.into_owned()))
                    .context("Unable to preserve Excel column metadata")?;
            }
            Event::Empty(start) if in_columns && local_name_is(&start, b"col") => {
                if write_hidden_column_split(&mut writer, &start, source_column)? {
                    source_column_hidden = true;
                }
            }
            Event::Start(start) if local_name_is(&start, b"sheetData") => {
                if !saw_columns {
                    writer
                        .write_event(Event::Start(BytesStart::new("cols")))
                        .context("Unable to add Excel column metadata")?;
                    write_new_hidden_column(&mut writer, source_column)?;
                    writer
                        .write_event(Event::End(BytesEnd::new("cols")))
                        .context("Unable to add Excel column metadata")?;
                    source_column_hidden = true;
                }
                in_sheet_data = true;
                writer
                    .write_event(Event::Start(start.into_owned()))
                    .context("Unable to preserve Excel sheet data")?;
            }
            Event::End(end) if end.local_name().as_ref() == b"sheetData" => {
                let remaining = std::mem::take(&mut updates);
                for (row, values) in remaining {
                    write_complete_row(&mut writer, row, values, date_style.as_deref())?;
                }
                in_sheet_data = false;
                writer
                    .write_event(Event::End(end.into_owned()))
                    .context("Unable to preserve Excel sheet data")?;
            }
            Event::Start(start) if in_sheet_data && local_name_is(&start, b"row") => {
                let row = attribute(&start, b"r")?
                    .and_then(|value| value.parse::<u32>().ok())
                    .ok_or_else(|| {
                        AppError::Message("An Excel row is missing its number.".into())
                    })?;
                let earlier = updates
                    .range(..row)
                    .map(|(row, _)| *row)
                    .collect::<Vec<_>>();
                for missing_row in earlier {
                    let values = updates
                        .remove(&missing_row)
                        .expect("pending Excel row disappeared");
                    write_complete_row(&mut writer, missing_row, values, date_style.as_deref())?;
                }
                pending = updates.remove(&row).unwrap_or_default();
                current_row = Some(row);
                let start = if pending.contains_key(&source_column) {
                    extended_row(&start, source_column)?
                } else {
                    start.into_owned()
                };
                writer
                    .write_event(Event::Start(start))
                    .context("Unable to preserve an Excel row")?;
            }
            Event::End(end) if end.local_name().as_ref() == b"row" => {
                if let Some(row) = current_row {
                    write_pending_before(
                        &mut writer,
                        row,
                        u32::MAX,
                        &mut pending,
                        date_style.as_deref(),
                    )?;
                }
                current_row = None;
                writer
                    .write_event(Event::End(end.into_owned()))
                    .context("Unable to preserve an Excel row")?;
            }
            Event::Start(start) if current_row.is_some() && local_name_is(&start, b"c") => {
                let coordinate = attribute(&start, b"r")?.ok_or_else(|| {
                    AppError::Message("An Excel cell is missing its coordinate.".into())
                })?;
                let column = coordinate_column(&coordinate).ok_or_else(|| {
                    AppError::Message("An Excel cell has an invalid coordinate.".into())
                })?;
                let row = current_row.expect("current row disappeared");
                write_pending_before(
                    &mut writer,
                    row,
                    column,
                    &mut pending,
                    date_style.as_deref(),
                )?;
                if let Some(value) = pending.remove(&column) {
                    write_cell(
                        &mut writer,
                        row,
                        column,
                        &value,
                        Some(&start),
                        date_style.as_deref(),
                    )?;
                    skip_cell_depth = 1;
                } else {
                    writer
                        .write_event(Event::Start(start.into_owned()))
                        .context("Unable to preserve an Excel cell")?;
                }
            }
            Event::Empty(start) if current_row.is_some() && local_name_is(&start, b"c") => {
                let coordinate = attribute(&start, b"r")?.ok_or_else(|| {
                    AppError::Message("An Excel cell is missing its coordinate.".into())
                })?;
                let column = coordinate_column(&coordinate).ok_or_else(|| {
                    AppError::Message("An Excel cell has an invalid coordinate.".into())
                })?;
                let row = current_row.expect("current row disappeared");
                write_pending_before(
                    &mut writer,
                    row,
                    column,
                    &mut pending,
                    date_style.as_deref(),
                )?;
                if let Some(value) = pending.remove(&column) {
                    write_cell(
                        &mut writer,
                        row,
                        column,
                        &value,
                        Some(&start),
                        date_style.as_deref(),
                    )?;
                } else {
                    writer
                        .write_event(Event::Empty(start.into_owned()))
                        .context("Unable to preserve an Excel cell")?;
                }
            }
            Event::Empty(start) if local_name_is(&start, b"dimension") => {
                writer
                    .write_event(Event::Empty(extended_dimension(&start, source_column)?))
                    .context("Unable to update the Excel worksheet dimension")?;
            }
            Event::Eof => break,
            other => writer
                .write_event(other.into_owned())
                .context("Unable to preserve the target Excel worksheet")?,
        }
        buffer.clear();
    }

    if !updates.is_empty() || !pending.is_empty() {
        return Err(AppError::Message(
            "Not all tracker rows could be written to the target worksheet.".into(),
        ));
    }
    if !source_column_hidden {
        return Err(AppError::Message(
            "The tracker provenance column could not be hidden safely.".into(),
        ));
    }
    Ok(writer.into_inner().into_inner())
}

fn read_zip_entry(archive: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>> {
    let mut entry = archive
        .by_name(name)
        .context(format!("The Excel package is missing {name}"))?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .context(format!("Unable to read {name} from the Excel package"))?;
    Ok(bytes)
}

fn relationship_id_for_sheet(workbook_xml: &[u8], sheet_index: usize) -> Result<String> {
    let mut reader = Reader::from_reader(Cursor::new(workbook_xml));
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut current = 0usize;
    loop {
        match reader
            .read_event_into(&mut buffer)
            .context("Unable to parse the Excel workbook manifest")?
        {
            Event::Start(start) | Event::Empty(start) if local_name_is(&start, b"sheet") => {
                if current == sheet_index {
                    for value in start.attributes().with_checks(false) {
                        let value = value.context("Unable to read an Excel sheet relationship")?;
                        if value.key.as_ref() == b"r:id" {
                            return Ok(value
                                .unescape_value()
                                .context("Unable to decode an Excel sheet relationship")?
                                .into_owned());
                        }
                    }
                    return Err(AppError::Message(
                        "The target sheet has no package relationship.".into(),
                    ));
                }
                current += 1;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Err(AppError::Message(
        "The target worksheet is missing from the Excel workbook manifest.".into(),
    ))
}

fn normalize_package_target(target: &str) -> Result<String> {
    let target = target.replace('\\', "/");
    let combined = if target.starts_with('/') {
        target.trim_start_matches('/').to_string()
    } else {
        format!("xl/{target}")
    };
    let mut parts = Vec::new();
    for part in combined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or_else(|| {
                    AppError::Message("The Excel package contains an invalid relationship.".into())
                })?;
            }
            value => parts.push(value),
        }
    }
    Ok(parts.join("/"))
}

fn worksheet_part_for_relationship(rels_xml: &[u8], relationship_id: &str) -> Result<String> {
    let mut reader = Reader::from_reader(Cursor::new(rels_xml));
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buffer)
            .context("Unable to parse the Excel workbook relationships")?
        {
            Event::Start(start) | Event::Empty(start) if local_name_is(&start, b"Relationship") => {
                if attribute(&start, b"Id")?.as_deref() == Some(relationship_id) {
                    let target = attribute(&start, b"Target")?.ok_or_else(|| {
                        AppError::Message(
                            "The target worksheet relationship has no package path.".into(),
                        )
                    })?;
                    return normalize_package_target(&target);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Err(AppError::Message(
        "The target worksheet package relationship was not found.".into(),
    ))
}

fn patch_existing_package(
    path: &Path,
    temp: &Path,
    sheet_index: usize,
    updates: RowUpdates,
    source_column: u32,
    header_row: u32,
) -> Result<()> {
    let file = File::open(path).context("Unable to open the existing Excel package")?;
    let mut archive =
        ZipArchive::new(file).context("The selected file is not a valid Excel package")?;
    let workbook_xml = read_zip_entry(&mut archive, "xl/workbook.xml")?;
    let rels_xml = read_zip_entry(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let relationship_id = relationship_id_for_sheet(&workbook_xml, sheet_index)?;
    let worksheet_part = worksheet_part_for_relationship(&rels_xml, &relationship_id)?;
    let worksheet_xml = read_zip_entry(&mut archive, &worksheet_part)?;
    let patched = patch_worksheet_xml(&worksheet_xml, updates, source_column, header_row)?;

    let output = File::create(temp).context("Unable to create the temporary Excel package")?;
    let mut writer = ZipWriter::new(output);
    let mut replaced = false;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .context("Unable to read an Excel package entry")?;
        if entry.name() == worksheet_part {
            let name = entry.name().to_string();
            let options = entry.options();
            writer
                .start_file(name, options)
                .context("Unable to replace the target Excel worksheet")?;
            writer
                .write_all(&patched)
                .context("Unable to write the target Excel worksheet")?;
            replaced = true;
        } else {
            writer
                .raw_copy_file(entry)
                .context("Unable to preserve an Excel package entry")?;
        }
    }
    if !replaced {
        return Err(AppError::Message(
            "The target worksheet package entry was not replaced.".into(),
        ));
    }
    writer
        .finish()
        .context("Unable to finish the updated Excel package")?;
    Ok(())
}

fn initialize_new(book: &mut Spreadsheet, profile: &UserProfile) -> Result<String> {
    let name = sheet_name(profile);
    let sheet = book
        .get_sheet_by_name_mut("Sheet1")
        .ok_or_else(|| AppError::Message("Unable to initialize a new workbook.".into()))?;
    sheet.set_name(&name);
    let header_row = 2;
    for (i, header) in TRACKER_HEADERS.iter().enumerate() {
        sheet
            .get_cell_mut(((i + 1) as u32, header_row))
            .set_value_string(*header);
    }
    let helper = TRACKER_HEADERS.len() as u32 + 1;
    sheet
        .get_cell_mut((helper, header_row))
        .set_value_string(SOURCE_HEADER);
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
    for (i, header) in TRACKER_HEADERS.iter().enumerate() {
        let col = (i + 1) as u32;
        sheet.get_cell_mut((col, header_row)).set_style(
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
    sheet
        .get_cell_mut((helper, header_row))
        .set_style(header_style);
    sheet
        .get_column_dimension_by_number_mut(&helper)
        .set_hidden(true);
    Ok(name)
}

fn replace_safely(temp: &Path, path: &Path, existing: bool) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Message("Choose a valid output location.".into()))?;
    fs::create_dir_all(parent).context("Unable to create the output directory")?;
    let backup = parent.join(format!(".atlas-{}.backup", Uuid::new_v4()));
    if existing {
        fs::copy(path, &backup)
            .context("Unable to create a safety copy before updating the workbook")?;
    }
    let copy_result = fs::copy(temp, path)
        .context("Unable to replace the workbook. Close it in Excel and try again");
    let _ = fs::remove_file(temp);
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

fn save_new_safely(book: &Spreadsheet, path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Message("Choose a valid output location.".into()))?;
    fs::create_dir_all(parent).context("Unable to create the output directory")?;
    let temp = parent.join(format!(".atlas-{}.tmp", Uuid::new_v4()));
    umya_spreadsheet::writer::xlsx::write(book, &temp)
        .context("Unable to write the Excel workbook")?;
    replace_safely(&temp, path, false)
}

fn export_new(
    path: &Path,
    profile: &UserProfile,
    selected: &[&Interaction],
) -> Result<ExportResult> {
    let mut book = umya_spreadsheet::new_file();
    let target_name = initialize_new(&mut book, profile)?;
    let sheet = book.get_sheet_by_name_mut(&target_name).ok_or_else(|| {
        AppError::Message("The target worksheet disappeared while exporting.".into())
    })?;
    let (header_row, mut headers) = find_headers(sheet)?;
    ensure_required_headers(&headers)?;
    let _source_column = ensure_source_column(sheet, header_row, &mut headers);
    let mut reserved = HashSet::new();
    for item in selected {
        let row = next_safe_row(sheet, &headers, header_row, None, &reserved)?;
        write_row(sheet, &headers, row, profile, item)?;
        reserved.insert(row);
    }
    save_new_safely(&book, path)?;
    Ok(ExportResult {
        path: path.to_string_lossy().to_string(),
        inserted: selected.len(),
        updated: 0,
        skipped: 0,
        queued: false,
    })
}

fn export_existing(
    path: &Path,
    profile: &UserProfile,
    selected: &[&Interaction],
) -> Result<ExportResult> {
    // umya-spreadsheet is used for workbook-aware validation and layout discovery.
    // The final save patches only the selected worksheet XML so VBA, external links,
    // validations, formulas, styles, hidden lookup tabs, and every other ZIP part stay intact.
    let book = umya_spreadsheet::reader::xlsx::read(path)
        .context("Unable to open the existing workbook. Close it in Excel and try again")?;
    let (sheet_index, target_name) = resolve_sheet(&book, profile)?;
    let sheet = book.get_sheet_by_name(&target_name).ok_or_else(|| {
        AppError::Message("The target worksheet disappeared while exporting.".into())
    })?;
    let (header_row, mut headers) = find_headers(sheet)?;
    ensure_required_headers(&headers)?;
    let source_column = headers
        .get("sourceid")
        .copied()
        .unwrap_or_else(|| headers.values().copied().max().unwrap_or(0) + 1);
    if source_column > 16_384 {
        return Err(AppError::Message(
            "The tracker has no available column for its hidden provenance IDs.".into(),
        ));
    }
    headers.insert("sourceid".into(), source_column);
    let table_end = tracker_table_end(sheet, &headers, header_row)?;

    let mut existing_sources = HashMap::new();
    for row in (header_row + 1)..=sheet.get_highest_row() {
        let id = sheet.get_value((source_column, row));
        if !id.trim().is_empty() {
            existing_sources.insert(id.to_string(), row);
        }
    }

    let mut reserved = HashSet::new();
    let mut updates = RowUpdates::new();
    let mut inserted = 0;
    let mut updated = 0;
    let mut skipped = 0;
    for item in selected {
        if let Some(row) = existing_sources.get(&item.source_id).copied() {
            if item.source_kind == SourceKind::Manual {
                skipped += 1;
                continue;
            }
            add_package_row(&mut updates, &headers, row, profile, item)?;
            updated += 1;
        } else {
            let row = next_safe_row(sheet, &headers, header_row, table_end, &reserved)?;
            add_package_row(&mut updates, &headers, row, profile, item)?;
            reserved.insert(row);
            existing_sources.insert(item.source_id.clone(), row);
            inserted += 1;
        }
    }

    let parent = path
        .parent()
        .ok_or_else(|| AppError::Message("Choose a valid output location.".into()))?;
    let temp = parent.join(format!(".atlas-{}.tmp", Uuid::new_v4()));
    if let Err(error) =
        patch_existing_package(path, &temp, sheet_index, updates, source_column, header_row)
    {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    replace_safely(&temp, path, true)?;
    Ok(ExportResult {
        path: path.to_string_lossy().to_string(),
        inserted,
        updated,
        skipped,
        queued: false,
    })
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

    if existing {
        export_existing(&path, profile, &selected)
    } else {
        export_new(&path, profile, &selected)
    }
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
    fn accepts_reviewed_ai_rows_with_verified_teams_evidence() {
        let mut v = calendar();
        v.source_kind = SourceKind::TeamsChat;
        v.source_id = "graph:teams:evidence".into();
        v.ai_suggested = true;
        assert!(validate_exportable(&v).is_ok());
        v.reviewed = false;
        assert!(validate_exportable(&v).is_err());
    }
    #[test]
    fn accepts_verified_power_automate_source_ids() {
        let mut v = calendar();
        v.source_id = "bridge:calendar:event-1".into();
        assert!(validate_exportable(&v).is_ok());
        v.source_kind = SourceKind::Email;
        v.source_id = "bridge:mail:message-1".into();
        assert!(validate_exportable(&v).is_ok());
    }
    #[test]
    fn canonicalizes_the_real_circana_header_labels() {
        assert_eq!(
            canonical_header("Reception Date/Time (mm/dd/yyyy hh:mm)"),
            "receptiondatetime"
        );
        assert_eq!(
            canonical_header("Interaction Date/Time \n(mm/dd/yyyy hh:mm)"),
            "interactiondatetime"
        );
        assert_eq!(
            canonical_header("Incident Number \n(If applies)"),
            "incidentnumber"
        );
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
        assert_eq!(sheet.get_value((22, 3)), "graph:calendar:event-1");
    }
    #[test]
    fn orange_columns_are_never_populated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Tracker.xlsx");
        export(path.to_str().unwrap(), false, &profile(), &[calendar()]).unwrap();
        let book = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
        let sheet = book.get_sheet_by_name("Test User").unwrap();
        for col in 16..=21 {
            assert!(sheet.get_value((col, 3)).is_empty());
        }
    }

    #[test]
    fn package_patch_preserves_vba_external_parts_and_orange_formulas() {
        use zip::write::SimpleFileOptions;

        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("Tracker.xlsm");
        let output = dir.path().join("Tracker-updated.xlsm");
        let workbook = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Test User" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let relationships = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let worksheet = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:U3"/><cols><col min="1" max="16384" width="8.5"/></cols><sheetData><row r="2" spans="1:21"><c r="A2" t="inlineStr"><is><t>Interaction</t></is></c><c r="U2" t="inlineStr"><is><t>IR Time</t></is></c></row><row r="3" spans="1:21"><c r="A3" s="44" t="inlineStr"><is><t>Old</t></is></c><c r="C3" s="46"/><c r="P3" s="52"><f>KEEP_ORANGE_FORMULA</f><v>0</v></c></row></sheetData></worksheet>"#;
        {
            let file = File::create(&source).unwrap();
            let mut writer = ZipWriter::new(file);
            let options = SimpleFileOptions::default();
            for (name, bytes) in [
                ("xl/workbook.xml", workbook.as_slice()),
                ("xl/_rels/workbook.xml.rels", relationships.as_slice()),
                ("xl/worksheets/sheet1.xml", worksheet.as_slice()),
                ("xl/vbaProject.bin", b"VBA-BYTES".as_slice()),
                (
                    "xl/externalLinks/externalLink1.xml",
                    b"EXTERNAL-BYTES".as_slice(),
                ),
            ] {
                writer.start_file(name, options).unwrap();
                writer.write_all(bytes).unwrap();
            }
            writer.finish().unwrap();
        }

        let mut updates = RowUpdates::new();
        updates
            .entry(3)
            .or_default()
            .insert(1, PackageCellValue::Text("Meeting".into()));
        updates
            .entry(3)
            .or_default()
            .insert(3, PackageCellValue::Date(46_536.5));
        updates
            .entry(3)
            .or_default()
            .insert(22, PackageCellValue::Text("graph:calendar:event-1".into()));
        patch_existing_package(&source, &output, 0, updates, 22, 2).unwrap();

        let mut archive = ZipArchive::new(File::open(&output).unwrap()).unwrap();
        assert_eq!(
            read_zip_entry(&mut archive, "xl/vbaProject.bin").unwrap(),
            b"VBA-BYTES"
        );
        assert_eq!(
            read_zip_entry(&mut archive, "xl/externalLinks/externalLink1.xml").unwrap(),
            b"EXTERNAL-BYTES"
        );
        let xml =
            String::from_utf8(read_zip_entry(&mut archive, "xl/worksheets/sheet1.xml").unwrap())
                .unwrap();
        assert!(xml.contains("KEEP_ORANGE_FORMULA"));
        assert!(xml.contains("r=\"V2\""));
        assert!(xml.contains("r=\"V3\""));
        assert!(xml.contains("graph:calendar:event-1"));
        assert!(xml.contains("hidden=\"1\""));
        assert!(xml.contains("r=\"C3\""));
        assert!(xml.contains("s=\"46\""));
        assert!(xml.contains("<v>46536.5</v>"));
    }

    #[test]
    fn package_patch_copies_date_style_to_new_rows() {
        let worksheet = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:E4"/><cols><col min="1" max="22" width="8.5"/></cols><sheetData><row r="2" spans="1:5"><c r="A2" t="inlineStr"><is><t>Interaction</t></is></c><c r="C2" s="77" t="inlineStr"><is><t>Reception Date/Time</t></is></c></row><row r="3" spans="1:5"><c r="C3" s="46"><v>46535</v></c></row></sheetData></worksheet>"#;
        let mut updates = RowUpdates::new();
        updates
            .entry(4)
            .or_default()
            .insert(1, PackageCellValue::Text("Meeting".into()));
        for column in 3..=5 {
            updates
                .entry(4)
                .or_default()
                .insert(column, PackageCellValue::Date(46_536.5));
        }
        let patched =
            String::from_utf8(patch_worksheet_xml(worksheet, updates, 22, 2).unwrap()).unwrap();
        for coordinate in ["C4", "D4", "E4"] {
            assert!(
                patched.contains(&format!("r=\"{coordinate}\" s=\"46\"")),
                "missing copied date style for {coordinate}: {patched}"
            );
        }
    }
}
