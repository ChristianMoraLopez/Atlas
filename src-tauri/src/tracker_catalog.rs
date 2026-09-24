//! Tracker value catalog and row completion.
//!
//! Every tracker column a CSA fills by hand must hold a value from the
//! workbook's own dropdown lists (hidden "Backdata" tab: Interactions,
//! Client Type/Clients, Category/Subcategory, Resolution Type). The catalog
//! is read from the configured workbook so each team's lists are respected;
//! the Circana standard lists are the fallback. `complete` fills every blank
//! or out-of-list value from the row's own evidence text, so no row reaches
//! Excel with an empty column.

use crate::{
    diagnostics,
    models::{Interaction, SourceKind, TrackerDestination, TrackerDestinationKind},
};
use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::UNIX_EPOCH,
};

const DEFAULT_TASK_MINUTES: i64 = 30;
const CATALOG_CACHE_HOURS: i64 = 24;
const NO_INCIDENT: &str = "N/A";

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CategoryEntry {
    pub name: String,
    pub subcategories: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrackerCatalog {
    pub interactions: Vec<String>,
    pub client_types: Vec<String>,
    /// Client type -> end clients allowed for it.
    pub clients: BTreeMap<String, Vec<String>>,
    pub categories: Vec<CategoryEntry>,
    pub resolution_types: Vec<String>,
    pub statuses: Vec<String>,
    pub priorities: Vec<String>,
    /// "workbook" when read from the configured tracker, else "default".
    pub source: String,
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

impl Default for TrackerCatalog {
    /// Lists of the Circana Interactions Tracker (V 1.0 Backdata tab).
    fn default() -> Self {
        let category = |name: &str, subcategories: &[&str]| CategoryEntry {
            name: name.into(),
            subcategories: strings(subcategories),
        };
        let mut clients = BTreeMap::new();
        clients.insert("Capgemini".into(), strings(&["Capgemini", "Support Team"]));
        clients.insert("Circana".into(), strings(&["Circana"]));
        clients.insert(
            "End_Client".into(),
            strings(&[
                "Bath & Body Works",
                "Bayer HealthCare LLC",
                "Beiersdorf",
                "Church & Dwight Brands",
                "Clorox Company",
                "DAT",
                "Edgewell Personal Care",
                "Emerson",
                "Georgia-Pacific Corporation",
                "Global Client Service",
                "HALEON / GSK",
                "Henkel Consumer Goods, Inc",
                "Huhtamaki Inc",
                "LALA",
                "Nestle",
                "PepsiCo",
                "Perrigo",
                "Revlon",
                "Reynolds Consumer Products",
                "Sales",
                "sanofi",
                "SENECA",
                "Unilever",
                "Kenvue ",
                "Pierre ",
                "Fabre",
                "Revance",
                "Gojo",
                "Maesa",
                "Honest",
                "Hain Celestial",
                "Allergan ",
                "Goody",
                "Prestige Beauty Clients",
            ]),
        );
        clients.insert("Others".into(), strings(&["Others"]));
        Self {
            interactions: strings(&[
                "DMS Board",
                "E-Mail",
                "IRI",
                "Meeting",
                "Monthly Task",
                "Queue Monitoring",
                "Task",
                "Teams Chat",
                "Ticket Handling",
                "Training Session",
                "Weekly Task",
            ]),
            client_types: strings(&["Capgemini", "Circana", "End_Client", "Others"]),
            clients,
            categories: vec![
                category(
                    "Meeting",
                    &[
                        "Bi-Weekly Client Review",
                        "Circana Client Touchbase",
                        "Circana Daily Meeting",
                        "Client Status Discussion",
                        "Client Training",
                        "CPG Major Change Event",
                        "Custom Model Kick Off",
                        "Internal Status Meeting",
                        "New Items Review",
                        "Ops News Meeting",
                        "Placeholder/Implementation Strategies Review",
                        "RCP Office hours",
                        "T4 Meetings",
                        "Update Landing Page",
                        "Update POD",
                        "Weekly Team Meeting",
                    ],
                ),
                category(
                    "QC",
                    &[
                        "Checkout & Approval",
                        "Quality Verification ",
                        "STG Approval",
                        "Update Active Users",
                    ],
                ),
                category(
                    "Reporting",
                    &["Bi-weekly Report", "Daily Report", "One Time Report", "Weekly Report"],
                ),
                category(
                    "Supply_Chain",
                    &[
                        "Data Analysis, Process Optimization, Report Generation",
                        "Extracts",
                        "POD-G Release Tracker",
                        "Scoping Submission",
                        "Scoping UPC",
                    ],
                ),
                category(
                    "Ticket_Handling",
                    &["Iris Ticket", "OSA Ticket Tracking", "Queue Monitoring", "Ticket Handling"],
                ),
                category(
                    "Training",
                    &["Capgemini Training", "Circana Training", "Client Training"],
                ),
                category(
                    "UDB",
                    &[
                        "Description Audit",
                        "Geography Inclusion",
                        "Geography QC",
                        "Item Missing from Model",
                        "Missing Items Audit",
                        "Missing Items QC",
                        "Missing Measures Review",
                        "Move Open to Pending",
                        "NIR / New Products",
                        "QC Migration for UDB",
                        "Rules Validation",
                        "Training",
                        "UDB New User Setup",
                        "UPC Scoping",
                        "Value Changes",
                    ],
                ),
                category(
                    "Unify_Request",
                    &[
                        "New User",
                        "Scoping UPC",
                        "Update Landing Page",
                        "User Interface Changes",
                        "White Alert",
                    ],
                ),
                category(
                    "User_Management",
                    &[
                        "Access Issues",
                        "Access Request",
                        "New User Setup",
                        "User Creation",
                        "User Resign",
                    ],
                ),
            ],
            resolution_types: strings(&[
                "CI Processed",
                "Escalated (Out of Scope)",
                "Processed & Resolved",
            ]),
            statuses: strings(&["Acknowledged", "Pending", "In Progress", "Resolved"]),
            priorities: strings(&["Low", "Intermediate", "High"]),
            source: "default".into(),
        }
    }
}

/// Lowercase letters and digits only, for header and value comparison.
fn canon(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Lowercase words separated by single spaces, accents folded.
fn words(value: &str) -> String {
    let folded: String = value
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' | 'Á' => 'a',
            'é' | 'è' | 'ë' | 'ê' | 'É' => 'e',
            'í' | 'ì' | 'ï' | 'î' | 'Í' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'Ó' => 'o',
            'ú' | 'ù' | 'ü' | 'û' | 'Ú' => 'u',
            'ñ' | 'Ñ' => 'n',
            other => other,
        })
        .collect();
    folded
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn contains_phrase(text: &str, phrase: &str) -> bool {
    let phrase = words(phrase);
    !phrase.is_empty() && format!(" {text} ").contains(&format!(" {phrase} "))
}

fn find<'a>(list: &'a [String], value: &str) -> Option<&'a String> {
    let wanted = canon(value);
    if wanted.is_empty() {
        return None;
    }
    list.iter().find(|item| canon(item) == wanted)
}

fn first_containing<'a>(list: &'a [String], needle: &str) -> Option<&'a String> {
    let needle = canon(needle);
    list.iter().find(|item| canon(item).contains(&needle))
}

impl TrackerCatalog {
    fn category(&self, name: &str) -> Option<&CategoryEntry> {
        let wanted = canon(name);
        self.categories.iter().find(|entry| canon(&entry.name) == wanted)
    }

    fn clients_for(&self, client_type: &str) -> &[String] {
        let wanted = canon(client_type);
        self.clients
            .iter()
            .find(|(name, _)| canon(name) == wanted)
            .map(|(_, list)| list.as_slice())
            .unwrap_or(&[])
    }

    fn pick(list: &[String], preferred: &[&str]) -> String {
        preferred
            .iter()
            .find_map(|value| find(list, value).cloned())
            .or_else(|| list.first().cloned())
            .unwrap_or_default()
    }

    /// Replaces empty lists with the defaults so every column has options.
    fn with_defaults(mut self) -> Self {
        let fallback = TrackerCatalog::default();
        if self.interactions.is_empty() {
            self.interactions = fallback.interactions;
        }
        if self.client_types.is_empty() {
            self.client_types = fallback.client_types;
        }
        if self.clients.is_empty() {
            self.clients = fallback.clients;
        }
        if self.categories.is_empty() {
            self.categories = fallback.categories;
        }
        if self.resolution_types.is_empty() {
            self.resolution_types = fallback.resolution_types;
        }
        if self.statuses.is_empty() {
            self.statuses = fallback.statuses;
        }
        if self.priorities.is_empty() {
            self.priorities = fallback.priorities;
        }
        self
    }
}

// ---------------------------------------------------------------------------
// Reading the lists from a workbook
// ---------------------------------------------------------------------------

fn read_column(sheet: &umya_spreadsheet::Worksheet, col: u32, from_row: u32) -> Vec<String> {
    let mut values = Vec::new();
    for row in from_row..=sheet.get_highest_row() {
        let value = sheet.get_value((col, row));
        if value.trim().is_empty() {
            break;
        }
        values.push(value);
    }
    values
}

fn read_pairs(
    sheet: &umya_spreadsheet::Worksheet,
    col: u32,
    from_row: u32,
) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for row in from_row..=sheet.get_highest_row() {
        let key = sheet.get_value((col, row));
        let value = sheet.get_value((col + 1, row));
        if key.trim().is_empty() && value.trim().is_empty() {
            break;
        }
        if !key.trim().is_empty() && !value.trim().is_empty() {
            pairs.push((key, value));
        }
    }
    pairs
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if find(list, &value).is_none() {
        list.push(value);
    }
}

/// Finds the list headers ("Interactions", "Client Type" + "Clients",
/// "Category" + "Subcategory", "Resolution Type") on any tab. The tracker's
/// own header row (which also has "Category"/"Subcategory") is skipped.
pub fn from_book(book: &umya_spreadsheet::Spreadsheet) -> Option<TrackerCatalog> {
    let mut catalog = TrackerCatalog {
        interactions: Vec::new(),
        client_types: Vec::new(),
        clients: BTreeMap::new(),
        categories: Vec::new(),
        resolution_types: Vec::new(),
        statuses: Vec::new(),
        priorities: Vec::new(),
        source: "workbook".into(),
    };
    for sheet in book.get_sheet_collection() {
        let max_col = sheet.get_highest_column().min(80);
        let max_row = sheet.get_highest_row().min(15);
        for row in 1..=max_row {
            let headers: Vec<String> = (1..=max_col)
                .map(|col| canon(&sheet.get_value((col, row))))
                .collect();
            if headers.iter().any(|h| h == "interaction")
                && headers.iter().any(|h| h == "loginidcorpid")
            {
                continue;
            }
            for (index, header) in headers.iter().enumerate() {
                let col = index as u32 + 1;
                let next = headers.get(index + 1).map(String::as_str).unwrap_or("");
                match (header.as_str(), next) {
                    ("interactions", _) if catalog.interactions.is_empty() => {
                        catalog.interactions = read_column(sheet, col, row + 1);
                    }
                    ("resolutiontype", _) if catalog.resolution_types.is_empty() => {
                        catalog.resolution_types = read_column(sheet, col, row + 1);
                    }
                    ("clienttype", "clients") if catalog.clients.is_empty() => {
                        for (client_type, client) in read_pairs(sheet, col, row + 1) {
                            push_unique(&mut catalog.client_types, client_type.trim().to_string());
                            let key = find(&catalog.client_types, &client_type)
                                .cloned()
                                .unwrap_or(client_type);
                            let list = catalog.clients.entry(key).or_default();
                            push_unique(list, client);
                        }
                    }
                    ("category", "subcategory") if catalog.categories.is_empty() => {
                        for (category, subcategory) in read_pairs(sheet, col, row + 1) {
                            let wanted = canon(&category);
                            match catalog
                                .categories
                                .iter_mut()
                                .find(|entry| canon(&entry.name) == wanted)
                            {
                                Some(entry) => push_unique(&mut entry.subcategories, subcategory),
                                None => catalog.categories.push(CategoryEntry {
                                    name: category.trim().to_string(),
                                    subcategories: vec![subcategory],
                                }),
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if catalog.categories.is_empty() && catalog.interactions.is_empty() {
        return None;
    }
    Some(catalog.with_defaults())
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CachedCatalog {
    path: String,
    modified: u64,
    read_at: String,
    catalog: TrackerCatalog,
}

fn workbook_path(destination: &TrackerDestination) -> Option<PathBuf> {
    let value = match destination.kind {
        TrackerDestinationKind::LocalExisting => Some(destination.value.as_str()),
        TrackerDestinationKind::SharePoint => destination.local_path.as_deref(),
        TrackerDestinationKind::LocalNew | TrackerDestinationKind::SharePointFlow => None,
    }?;
    let path = PathBuf::from(value.strip_prefix(r"\\?\").unwrap_or(value));
    path.is_file().then_some(path)
}

fn modified_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// The lists read most recently, without opening the workbook (for quick,
/// synchronous paths such as restoring a saved preview).
pub fn cached(config_dir: &Path) -> TrackerCatalog {
    fs::read(config_dir.join("tracker-catalog.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<CachedCatalog>(&bytes).ok())
        .map(|cached| cached.catalog)
        .unwrap_or_default()
}

/// Catalog for the configured destination. Reading a large tracker takes a
/// few seconds, so the result is cached per workbook for a day (lists rarely
/// change, while every export updates the file's modification time).
pub fn load(config_dir: &Path, destination: Option<&TrackerDestination>) -> TrackerCatalog {
    let cache_path = config_dir.join("tracker-catalog.json");
    let cached: Option<CachedCatalog> = fs::read(&cache_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let Some(path) = destination.and_then(workbook_path) else {
        // Remote-only destinations reuse the lists read from the last local copy.
        return cached
            .map(|cached| cached.catalog)
            .unwrap_or_default();
    };
    let path_text = path.to_string_lossy().to_string();
    let modified = modified_secs(&path);
    if let Some(cached) = &cached {
        let fresh = DateTime::parse_from_rfc3339(&cached.read_at)
            .map(|read| Utc::now() - read.with_timezone(&Utc) < Duration::hours(CATALOG_CACHE_HOURS))
            .unwrap_or(false);
        if cached.path == path_text && (cached.modified == modified || fresh) {
            return cached.catalog.clone();
        }
    }
    match umya_spreadsheet::reader::xlsx::read(&path) {
        Ok(book) => match from_book(&book) {
            Some(catalog) => {
                let entry = CachedCatalog {
                    path: path_text,
                    modified,
                    read_at: Utc::now().to_rfc3339(),
                    catalog: catalog.clone(),
                };
                if let Ok(bytes) = serde_json::to_vec_pretty(&entry) {
                    let _ = fs::write(&cache_path, bytes);
                }
                diagnostics::info(
                    "tracker/catalog",
                    &format!(
                        "Read {} categories and {} client types from the tracker lists",
                        catalog.categories.len(),
                        catalog.client_types.len()
                    ),
                );
                catalog
            }
            None => TrackerCatalog::default(),
        },
        Err(error) => {
            diagnostics::error(
                "tracker/catalog",
                &format!("Using the saved tracker lists; the workbook could not be read: {error}"),
            );
            cached.map(|cached| cached.catalog).unwrap_or_default()
        }
    }
}

// ---------------------------------------------------------------------------
// Row completion
// ---------------------------------------------------------------------------

/// Evidence phrases that point to a category (canonical category name).
const CATEGORY_HINTS: &[(&str, &[&str])] = &[
    ("qc", &["qc", "quality", "verification", "verify", "approval", "approve", "checkout", "stg", "active users"]),
    ("reporting", &["report", "reports", "reporting", "dashboard", "reconciliation"]),
    ("supplychain", &["supply chain", "extract", "extracts", "scoping submission", "pod g", "podg", "release tracker", "process optimization", "data analysis"]),
    ("tickethandling", &["ticket", "tickets", "iris", "incident", "osa", "queue", "service request"]),
    ("training", &["training", "trainings", "onboarding", "course", "learning"]),
    ("udb", &["udb", "upc", "nir", "new product", "new products", "missing item", "missing items", "geography", "description audit", "rules validation", "value change", "value changes", "measure", "measures", "model", "coding", "hierarchy"]),
    ("unifyrequest", &["unify", "landing page", "white alert", "user interface", "liquid data"]),
    ("usermanagement", &["user", "users", "access", "account", "accounts", "permission", "permissions", "resign", "login", "password"]),
];

/// Extra phrases for subcategories whose names are not self-explanatory.
const SUBCATEGORY_HINTS: &[(&str, &[&str])] = &[
    ("t4meetings", &["t4"]),
    ("circanadailymeeting", &["daily", "standup", "stand up", "huddle"]),
    ("weeklyteammeeting", &["weekly", "team meeting"]),
    ("circanaclienttouchbase", &["touchbase", "touch base", "sync", "catch up", "check in"]),
    ("biweeklyclientreview", &["biweekly", "bi weekly"]),
    ("custommodelkickoff", &["kick off", "kickoff"]),
    ("rcpofficehours", &["office hours"]),
    ("iristicket", &["iris"]),
    ("newuser", &["new user", "add a user", "create user"]),
    ("usercreation", &["create a new user", "user id", "create user"]),
    ("accessrequest", &["access request", "request access", "grant access"]),
    ("accessissues", &["access issue", "cannot access", "can t access", "unable to access", "locked"]),
];

const STOP_WORDS: &[&str] = &["the", "and", "for", "with", "from", "into", "new", "update", "review", "meeting"];

fn hint_score(text: &str, phrases: &[&str]) -> i64 {
    phrases
        .iter()
        .filter(|phrase| contains_phrase(text, phrase))
        .count() as i64
}

/// Meaningful words of a list value: 3+ letters, or any word with a digit
/// ("T4"), without generic words.
fn name_tokens(name: &str) -> Vec<String> {
    words(name)
        .split(' ')
        .filter(|token| !token.is_empty() && !STOP_WORDS.contains(token))
        .map(str::to_string)
        .collect()
}

fn significant(token: &str) -> bool {
    token.len() >= 3 || token.chars().any(|c| c.is_ascii_digit())
}

fn token_score(text: &str, name: &str) -> i64 {
    name_tokens(name)
        .iter()
        .filter(|token| significant(token) && contains_phrase(text, token))
        .count() as i64
}

/// Words of the name found in the text count double; words that are not
/// there count against it, so "weekly report" prefers "Weekly Report" over
/// "Bi-weekly Report".
fn subcategory_score(text: &str, name: &str) -> i64 {
    let tokens = name_tokens(name);
    let matched = tokens
        .iter()
        .filter(|token| significant(token) && contains_phrase(text, token))
        .count() as i64;
    matched * 2 - (tokens.len() as i64 - matched)
}

fn best_subcategory(entry: &CategoryEntry, text: &str) -> Option<String> {
    entry
        .subcategories
        .iter()
        .map(|sub| {
            let hints = SUBCATEGORY_HINTS
                .iter()
                .find(|(key, _)| *key == canon(sub))
                .map(|(_, phrases)| hint_score(text, phrases))
                .unwrap_or(0);
            let matched = token_score(text, sub);
            let score = if hints + matched > 0 {
                hints * 3 + subcategory_score(text, sub)
            } else {
                0
            };
            (score, sub)
        })
        .filter(|(score, _)| *score > 0)
        .max_by_key(|(score, _)| *score)
        .map(|(_, sub)| sub.clone())
}

fn best_task_category<'a>(catalog: &'a TrackerCatalog, text: &str) -> Option<&'a CategoryEntry> {
    catalog
        .categories
        .iter()
        .filter(|entry| canon(&entry.name) != "meeting")
        .map(|entry| {
            let key = canon(&entry.name);
            let hints = CATEGORY_HINTS
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, phrases)| hint_score(text, phrases))
                .unwrap_or(0);
            let subs: i64 = entry
                .subcategories
                .iter()
                .map(|sub| token_score(text, sub))
                .sum();
            (hints * 3 + subs + token_score(text, &entry.name) * 3, entry)
        })
        .filter(|(score, _)| *score > 0)
        .max_by_key(|(score, _)| *score)
        .map(|(_, entry)| entry)
}

fn incident_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?i)\b(INC|REQ|RITM|SR|CHG|PRB|CS)[\s#:_-]?(\d{4,})\b")
            .expect("valid regex")
    })
}

pub fn incident_number(text: &str) -> Option<String> {
    incident_pattern()
        .captures(text)
        .map(|caps| format!("{}{}", caps[1].to_uppercase(), &caps[2]))
}

/// List words too common to identify a client on their own.
const GENERIC_CLIENT_WORDS: &[&str] = &["global", "sales", "support", "team", "client", "clients", "service", "data", "others", "prestige"];

fn end_client_in_text(catalog: &TrackerCatalog, text: &str) -> Option<(String, String)> {
    for (client_type, clients) in &catalog.clients {
        let kind = canon(client_type);
        if kind == "circana" || kind == "others" {
            continue;
        }
        for client in clients {
            let name = words(client);
            let first = name.split(' ').next().unwrap_or("");
            let generic = GENERIC_CLIENT_WORDS.contains(&name.as_str());
            let matched = (name.len() >= 3 && !generic && contains_phrase(text, &name))
                || (first.len() >= 4
                    && !GENERIC_CLIENT_WORDS.contains(&first)
                    && contains_phrase(text, first));
            if matched {
                return Some((client_type.clone(), client.clone()));
            }
        }
    }
    None
}

fn add_minutes(value: &str, minutes: i64) -> Option<String> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| (time.with_timezone(&Utc) + Duration::minutes(minutes)).to_rfc3339())
}

/// Fills every blank or out-of-list tracker value of one row with a value
/// from the catalog, chosen from the row's own evidence. Idempotent: a row
/// that is already complete and valid is left untouched.
pub fn complete(item: &mut Interaction, catalog: &TrackerCatalog, now: DateTime<Utc>) {
    let text = words(&format!(
        "{} {} {} {}",
        item.comments, item.evidence_label, item.end_client, item.incident_number
    ));

    // Interaction (input channel).
    item.interaction_type = match find(&catalog.interactions, &item.interaction_type) {
        Some(value) => value.clone(),
        None => {
            let preferred: &[&str] = match item.source_kind {
                SourceKind::Calendar => &["Meeting"],
                SourceKind::Email => &["E-Mail", "Task"],
                SourceKind::TeamsChat => &["Task", "Teams Chat"],
                SourceKind::Manual => &["Task"],
            };
            TrackerCatalog::pick(&catalog.interactions, preferred)
        }
    };
    let is_meeting = canon(&item.interaction_type) == "meeting";

    // Dates: the resolution is always recorded and later than the interaction.
    if item.reception_date_time.trim().is_empty() {
        item.reception_date_time = item.interaction_date_time.clone();
    }
    let resolution_valid = item
        .resolution_date_time
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .zip(DateTime::parse_from_rfc3339(&item.interaction_date_time).ok())
        .is_some_and(|(resolution, start)| resolution >= start);
    if !resolution_valid {
        item.resolution_date_time = add_minutes(&item.interaction_date_time, DEFAULT_TASK_MINUTES)
            .or_else(|| item.resolution_date_time.clone());
    }

    // Status: every recorded activity is closed at its resolution time,
    // except a meeting that has not happened yet.
    let future_meeting = is_meeting
        && item
            .resolution_date_time
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|end| end.with_timezone(&Utc) > now);
    item.status = if future_meeting {
        TrackerCatalog::pick(&catalog.statuses, &["In Progress", "Pending"])
    } else {
        TrackerCatalog::pick(&catalog.statuses, &["Resolved"])
    };
    if find(&catalog.resolution_types, &item.resolution_type).is_none() {
        item.resolution_type =
            TrackerCatalog::pick(&catalog.resolution_types, &["Processed & Resolved"]);
    }

    // Type of Client and End Client, always a pair from the client lists.
    let mut client_type = find(&catalog.client_types, &item.client_type).cloned();
    if let Some(kind) = &client_type {
        if canon(kind) == "endclient" && find(catalog.clients_for(kind), &item.end_client).is_none() {
            // Map an email domain or text mention to a listed client.
            match end_client_in_text(catalog, &text) {
                Some((matched_type, client)) => {
                    client_type = Some(matched_type);
                    item.end_client = client;
                }
                None => {
                    client_type = Some(TrackerCatalog::pick(&catalog.client_types, &["Others"]));
                    item.end_client.clear();
                }
            }
        }
    } else {
        client_type = Some(match end_client_in_text(catalog, &text) {
            Some((matched_type, client)) => {
                item.end_client = client;
                matched_type
            }
            None if contains_phrase(&text, "capgemini") => {
                TrackerCatalog::pick(&catalog.client_types, &["Capgemini"])
            }
            None => TrackerCatalog::pick(&catalog.client_types, &["Circana"]),
        });
    }
    let client_type = client_type.unwrap_or_default();
    let allowed = catalog.clients_for(&client_type);
    if let Some(exact) = find(allowed, &item.end_client) {
        item.end_client = exact.clone();
    } else if allowed.is_empty() {
        // A workbook without a client list for this type: name the type.
        item.end_client = client_type.clone();
    } else {
        item.end_client =
            TrackerCatalog::pick(allowed, &[client_type.as_str(), "Circana", "Others"]);
    }
    item.client_type = client_type;

    // Category and Subcategory, always a listed pair.
    let category = match catalog.category(&item.category) {
        Some(entry) => Some(entry),
        None if is_meeting => catalog.category("Meeting"),
        None => best_task_category(catalog, &text).or_else(|| {
            catalog
                .category("Ticket_Handling")
                .or_else(|| catalog.categories.iter().find(|e| canon(&e.name) != "meeting"))
        }),
    }
    .or_else(|| catalog.categories.first());
    if let Some(entry) = category {
        item.category = entry.name.clone();
        if let Some(exact) = find(&entry.subcategories, &item.subcategory) {
            item.subcategory = exact.clone();
        } else {
            let internal = matches!(canon(&item.client_type).as_str(), "circana" | "capgemini");
            let fallback: &[&str] = match canon(&entry.name).as_str() {
                "meeting" if internal => &["Internal Status Meeting", "Weekly Team Meeting"],
                "meeting" => &["Client Status Discussion", "Circana Client Touchbase"],
                "tickethandling" => &["Ticket Handling"],
                "reporting" => &["One Time Report"],
                "qc" => &["Quality Verification", "Checkout & Approval"],
                "supplychain" => &["Data Analysis, Process Optimization, Report Generation"],
                "training" => &["Circana Training"],
                "udb" => &["Rules Validation"],
                "unifyrequest" => &["User Interface Changes"],
                "usermanagement" => &["Access Request"],
                _ => &[],
            };
            item.subcategory = best_subcategory(entry, &text)
                .unwrap_or_else(|| TrackerCatalog::pick(&entry.subcategories, fallback));
            if item.subcategory.trim().is_empty() {
                item.subcategory = entry.name.clone();
            }
        }
    }

    // Priority, Incident Number, Comments.
    item.priority = match find(&catalog.priorities, &item.priority) {
        Some(value) => value.clone(),
        None => TrackerCatalog::pick(&catalog.priorities, &["Intermediate", "Medium"]),
    };
    if item.incident_number.trim().is_empty() {
        item.incident_number = incident_number(&format!("{} {}", item.comments, item.evidence_label))
            .unwrap_or_else(|| NO_INCIDENT.into());
    }
    if item.comments.trim().is_empty() {
        item.comments = if item.evidence_label.trim().is_empty() {
            item.subcategory.clone()
        } else {
            item.evidence_label.trim().to_string()
        };
    }
}

/// Every tracker column that must never be empty.
pub fn missing_fields(item: &Interaction) -> Vec<&'static str> {
    let checks = [
        ("Interaction", item.interaction_type.as_str()),
        ("Reception Date/Time", item.reception_date_time.as_str()),
        ("Interaction Date/Time", item.interaction_date_time.as_str()),
        ("Resolution Date/Time", item.resolution_date_time.as_deref().unwrap_or("")),
        ("Type of Client", item.client_type.as_str()),
        ("End Client", item.end_client.as_str()),
        ("Status", item.status.as_str()),
        ("Resolution Type", item.resolution_type.as_str()),
        ("Category", item.category.as_str()),
        ("Subcategory", item.subcategory.as_str()),
        ("Priority", item.priority.as_str()),
        ("Incident Number", item.incident_number.as_str()),
        ("Comments", item.comments.as_str()),
    ];
    checks
        .iter()
        .filter(|(_, value)| value.trim().is_empty())
        .map(|(name, _)| *name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: SourceKind, interaction: &str, comments: &str) -> Interaction {
        Interaction {
            source_kind: kind,
            source_id: "bridge:test".into(),
            interaction_type: interaction.into(),
            reception_date_time: "2026-09-22T14:00:00+00:00".into(),
            interaction_date_time: "2026-09-22T14:00:00+00:00".into(),
            resolution_date_time: None,
            client_type: String::new(),
            end_client: String::new(),
            status: "In Progress".into(),
            resolution_type: String::new(),
            category: String::new(),
            subcategory: String::new(),
            priority: String::new(),
            incident_number: String::new(),
            comments: comments.into(),
            selected: true,
            reviewed: true,
            manual_authored: false,
            ai_suggested: true,
            evidence_label: String::new(),
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-23T15:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn assert_valid(item: &Interaction, catalog: &TrackerCatalog) {
        assert!(missing_fields(item).is_empty(), "blank: {:?}", missing_fields(item));
        assert!(find(&catalog.interactions, &item.interaction_type).is_some());
        assert!(find(&catalog.client_types, &item.client_type).is_some());
        assert!(find(catalog.clients_for(&item.client_type), &item.end_client).is_some());
        let category = catalog.category(&item.category).expect("listed category");
        assert!(find(&category.subcategories, &item.subcategory).is_some());
        assert!(find(&catalog.statuses, &item.status).is_some());
        assert!(find(&catalog.resolution_types, &item.resolution_type).is_some());
        assert!(find(&catalog.priorities, &item.priority).is_some());
    }

    #[test]
    fn every_column_is_filled_with_listed_values() {
        let catalog = TrackerCatalog::default();
        let cases = [
            row(SourceKind::TeamsChat, "Task", "Validate UDB rules for the new items of Bayer"),
            row(SourceKind::Email, "Task", "Create a new user ID for Unilever in Unify"),
            row(SourceKind::TeamsChat, "Task", "Send the weekly report to the client"),
            row(SourceKind::Calendar, "Meeting", "T4 Meetings"),
            row(SourceKind::Calendar, "Meeting", "Daily standup Circana"),
            row(SourceKind::Manual, "", ""),
            row(SourceKind::TeamsChat, "Task", "Follow up on INC0012345 raised by the client"),
            row(SourceKind::TeamsChat, "Task", "Something without any known keyword"),
        ];
        for mut item in cases {
            complete(&mut item, &catalog, now());
            assert_valid(&item, &catalog);
        }
    }

    #[test]
    fn categories_and_subcategories_follow_the_evidence() {
        let catalog = TrackerCatalog::default();
        let mut udb = row(SourceKind::TeamsChat, "Task", "Rules validation for UDB missing items");
        complete(&mut udb, &catalog, now());
        assert_eq!(udb.category, "UDB");
        let mut report = row(SourceKind::TeamsChat, "Task", "Prepare the weekly report");
        complete(&mut report, &catalog, now());
        assert_eq!((report.category.as_str(), report.subcategory.as_str()), ("Reporting", "Weekly Report"));
        let mut access = row(SourceKind::Email, "Task", "User cannot access the portal, access issue");
        complete(&mut access, &catalog, now());
        assert_eq!((access.category.as_str(), access.subcategory.as_str()), ("User_Management", "Access Issues"));
        let mut t4 = row(SourceKind::Calendar, "Meeting", "T4 sync");
        t4.resolution_date_time = Some("2026-09-22T14:30:00+00:00".into());
        complete(&mut t4, &catalog, now());
        assert_eq!((t4.category.as_str(), t4.subcategory.as_str()), ("Meeting", "T4 Meetings"));
        assert_eq!(t4.status, "Resolved");
    }

    #[test]
    fn clients_come_from_the_lists() {
        let catalog = TrackerCatalog::default();
        let mut bayer = row(SourceKind::Email, "Task", "Refresh the Bayer dashboard");
        bayer.client_type = "End_Client".into();
        bayer.end_client = "bayer".into();
        complete(&mut bayer, &catalog, now());
        assert_eq!((bayer.client_type.as_str(), bayer.end_client.as_str()), ("End_Client", "Bayer HealthCare LLC"));
        let mut unknown = row(SourceKind::Email, "Task", "Answer TetraPak about the file");
        unknown.client_type = "End_Client".into();
        complete(&mut unknown, &catalog, now());
        assert_eq!((unknown.client_type.as_str(), unknown.end_client.as_str()), ("Others", "Others"));
        let mut internal = row(SourceKind::TeamsChat, "Task", "Prepare the file");
        complete(&mut internal, &catalog, now());
        assert_eq!((internal.client_type.as_str(), internal.end_client.as_str()), ("Circana", "Circana"));
        let mut kenvue = row(SourceKind::TeamsChat, "Task", "Kenvue item coding");
        complete(&mut kenvue, &catalog, now());
        // Exact list value, including the trailing space the workbook uses.
        assert_eq!(kenvue.end_client, "Kenvue ");
    }

    #[test]
    fn dates_status_incident_and_resolution_are_never_blank() {
        let catalog = TrackerCatalog::default();
        let mut task = row(SourceKind::TeamsChat, "Task", "Follow up REQ 445566 for the client");
        complete(&mut task, &catalog, now());
        assert_eq!(task.incident_number, "REQ445566");
        assert_eq!(task.status, "Resolved");
        assert_eq!(task.resolution_type, "Processed & Resolved");
        let start = DateTime::parse_from_rfc3339(&task.interaction_date_time).unwrap();
        let end = DateTime::parse_from_rfc3339(task.resolution_date_time.as_deref().unwrap()).unwrap();
        assert_eq!((end - start).num_minutes(), DEFAULT_TASK_MINUTES);
        let mut plain = row(SourceKind::TeamsChat, "Task", "Prepare the file");
        complete(&mut plain, &catalog, now());
        assert_eq!(plain.incident_number, NO_INCIDENT);
        let mut future = row(SourceKind::Calendar, "Meeting", "Weekly team meeting");
        future.interaction_date_time = "2026-09-24T14:00:00+00:00".into();
        future.reception_date_time = future.interaction_date_time.clone();
        complete(&mut future, &catalog, now());
        assert_eq!(future.status, "In Progress");
        assert!(missing_fields(&future).is_empty());
    }

    #[test]
    fn completion_is_idempotent() {
        let catalog = TrackerCatalog::default();
        let mut item = row(SourceKind::TeamsChat, "Task", "Geography QC for Nestle");
        complete(&mut item, &catalog, now());
        let first = serde_json::to_string(&item).unwrap();
        complete(&mut item, &catalog, now());
        assert_eq!(first, serde_json::to_string(&item).unwrap());
    }

    #[test]
    fn lists_are_read_from_the_workbook_backdata_tab() {
        let mut book = umya_spreadsheet::new_file();
        let data = book.get_sheet_by_name_mut("Sheet1").unwrap();
        for (col, header) in ["Interaction", "Login ID (Corp ID)", "Category", "Subcategory"].iter().enumerate() {
            data.get_cell_mut((col as u32 + 1, 2)).set_value(header.to_string());
        }
        let backdata = book.new_sheet("Backdata").unwrap();
        for (coordinate, value) in [
            ("K3", "Client Type"), ("L3", "Clients"), ("N3", "Interactions"), ("P3", "Resolution Type"),
            ("T3", "Category"), ("U3", "Subcategory"),
            ("K4", "Circana"), ("L4", "Circana"), ("K5", "End_Client"), ("L5", "Acme"),
            ("N4", "Meeting"), ("N5", "Task"), ("P4", "Processed & Resolved"),
            ("T4", "Meeting"), ("U4", "Team Sync"), ("T5", "Audits"), ("U5", "Item Audit"), ("T6", "Audits"), ("U6", "Store Audit"),
        ] {
            backdata.get_cell_mut(coordinate).set_value(value.to_string());
        }
        let catalog = from_book(&book).unwrap();
        assert_eq!(catalog.source, "workbook");
        assert_eq!(catalog.interactions, vec!["Meeting", "Task"]);
        assert_eq!(catalog.client_types, vec!["Circana", "End_Client"]);
        assert_eq!(catalog.clients_for("End_Client"), &["Acme".to_string()]);
        assert_eq!(catalog.categories.len(), 2);
        assert_eq!(catalog.category("Audits").unwrap().subcategories, vec!["Item Audit", "Store Audit"]);
        // Statuses and priorities are fixed lists in the tracker validations.
        assert_eq!(catalog.priorities, vec!["Low", "Intermediate", "High"]);
        let mut item = row(SourceKind::TeamsChat, "Task", "Store audit for Acme");
        complete(&mut item, &catalog, now());
        assert_eq!((item.category.as_str(), item.subcategory.as_str()), ("Audits", "Store Audit"));
        assert_eq!((item.client_type.as_str(), item.end_client.as_str()), ("End_Client", "Acme"));
    }
}
