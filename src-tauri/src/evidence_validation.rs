//! Offline validation of the bundled evidence contract. Never returns evidence in errors.
use chrono::{DateTime, NaiveDate};
use serde_json::Value;

pub const SCHEMA: &str = include_str!("../../power-automate/atlas-evidence.schema.json");

pub fn valid(value: &Value) -> bool {
    matches_schema(
        value,
        &serde_json::from_str(SCHEMA).expect("bundled schema"),
    )
}

// The contract uses only these JSON Schema keywords. Tests guard its vocabulary so
// a future schema change cannot silently weaken validation. No remote $ref loading.
fn matches_schema(value: &Value, schema: &Value) -> bool {
    if let Some(constant) = schema.get("const") {
        if value != constant {
            return false;
        }
    }
    match schema["type"].as_str() {
        Some("object") => {
            let Some(object) = value.as_object() else {
                return false;
            };
            let Some(properties) = schema["properties"].as_object() else {
                return false;
            };
            if schema["required"].as_array().is_some_and(|keys| {
                keys.iter()
                    .any(|key| !object.contains_key(key.as_str().unwrap_or("")))
            }) {
                return false;
            }
            object.iter().all(|(key, child)| match properties.get(key) {
                Some(child_schema) => matches_schema(child, child_schema),
                None => schema["additionalProperties"] != false,
            })
        }
        Some("array") => value.as_array().is_some_and(|items| {
            schema["maxItems"]
                .as_u64()
                .is_none_or(|max| items.len() as u64 <= max)
                && items
                    .iter()
                    .all(|item| matches_schema(item, &schema["items"]))
        }),
        Some("string") => value.as_str().is_some_and(|s| {
            schema["minLength"]
                .as_u64()
                .is_none_or(|min| s.chars().count() as u64 >= min)
                && match schema["format"].as_str() {
                    Some("date-time") => DateTime::parse_from_rfc3339(s).is_ok(),
                    Some("date") => {
                        s.len() == 10 && NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
                    }
                    None => true,
                    _ => false,
                }
        }),
        Some("boolean") => value.is_boolean(),
        None => schema.get("const").is_some(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn example() -> Value {
        serde_json::from_str(include_str!(
            "../../power-automate/atlas-evidence.example.json"
        ))
        .unwrap()
    }
    #[test]
    fn example_matches_exact_contract() {
        assert!(valid(&example()));
    }
    #[test]
    fn rejects_missing_extra_wrong_types_dates_and_limits() {
        let example = example();
        for key in [
            "calendar",
            "mail",
            "teams",
            "targetDate",
            "schemaVersion",
            "exportedAt",
        ] {
            let mut v = example.clone();
            v.as_object_mut().unwrap().remove(key);
            assert!(!valid(&v));
        }
        for (key, value) in [
            ("secret", serde_json::json!("never logged")),
            ("schemaVersion", serde_json::json!(2)),
            ("targetDate", serde_json::json!("2026-02-30")),
            ("exportedAt", serde_json::json!("2026-09-09T12:00:00")),
            ("teams", serde_json::json!({})),
        ] {
            let mut v = example.clone();
            v[key] = value;
            assert!(!valid(&v));
        }
        let mut v = example.clone();
        v["calendar"][0]["id"] = serde_json::json!("");
        assert!(!valid(&v));
        let mut v = example.clone();
        v["calendar"][0]["extra"] = serde_json::json!(true);
        assert!(!valid(&v));
        let mut v = example.clone();
        v["calendar"] = serde_json::json!(vec![example["calendar"][0].clone(); 501]);
        assert!(!valid(&v));
    }
    #[test]
    fn schema_vocabulary_is_supported() {
        fn check(schema: &Value) {
            for key in schema.as_object().unwrap().keys() {
                assert!(
                    [
                        "$schema",
                        "$id",
                        "title",
                        "type",
                        "additionalProperties",
                        "required",
                        "properties",
                        "const",
                        "format",
                        "maxItems",
                        "items",
                        "minLength"
                    ]
                    .contains(&key.as_str()),
                    "Unsupported schema keyword {key}"
                );
            }
            if let Some(properties) = schema["properties"].as_object() {
                for child in properties.values() {
                    check(child);
                }
            }
            if let Some(items) = schema.get("items") {
                check(items);
            }
        }
        check(&serde_json::from_str(SCHEMA).unwrap());
    }
}
