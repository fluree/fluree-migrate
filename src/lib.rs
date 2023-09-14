pub mod functions {
    use chrono::{DateTime, NaiveDateTime, Utc};
    use crossterm::style::{Print, ResetColor, SetForegroundColor};
    use crossterm::{execute, style::Color};
    use dialoguer::console::{Style, Term};
    use dialoguer::{theme::ColorfulTheme, Input};
    use indicatif::{HumanDuration, ProgressBar, ProgressStyle};
    use reqwest::header::HeaderMap;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::{self, prelude::*, stdout};
    use std::path::PathBuf;
    use std::time::Instant;
    use structopt::StructOpt;

    // I have epoch instant values like 1693403567000 but want to convert them to ISO strings like "2023-08-30T13:52:47.000Z"
    pub fn instant_to_iso_string(epoch: i64) -> String {
        let naive =
            NaiveDateTime::from_timestamp_millis(epoch).expect("DateTime value is out of range");
        let date_time: DateTime<Utc> = DateTime::from_naive_utc_and_offset(naive, Utc);
        date_time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }

    pub fn represent_fluree_value(value: &Value) -> Value {
        match value {
            Value::Object(value) => serde_json::json!({ "@id": value["_id"].to_string() }),
            Value::Array(value) => {
                let mut array: Vec<Value> = Vec::new();
                for item in value {
                    array.push(represent_fluree_value(item));
                }
                Value::Array(array)
            }
            Value::String(value) => Value::String(value.to_string()),
            Value::Number(value) => Value::Number(value.to_owned()),
            Value::Bool(value) => Value::Bool(value.to_owned()),
            Value::Null => Value::Null,
        }
    }

    // a function that expects strings. If the string has a pattern of substr:substr separated by ":", then it will return the second substr
    pub fn remove_namespace(string: &str) -> &str {
        let mut split = string.split(":");
        let first = split.next();
        let second = split.next();
        match (first, second) {
            (Some(_), Some(second)) => second,
            _ => string,
        }
    }

    // remove leading "_" from a string, so "_fn" should return "fn", "_auth" should return "auth", etc.
    pub fn remove_underscore(string: &str) -> &str {
        if string.starts_with("_") {
            &string[1..]
        } else {
            string
        }
    }

    // standardize a string so that it goes from snake_case to CamelCase. The one exception is that if the leading char is uncapitalized, it should remain uncapitalized
    pub fn standardize(string: &str) -> String {
        let mut split = string.split("_");
        let mut result = String::new();
        let first = split.next();
        let first = match first {
            Some(first) => first,
            None => "",
        };
        result.push_str(first);
        for part in split {
            result.push_str(&capitalize(part));
        }
        result
    }

    pub fn capitalize(string: &str) -> String {
        let mut chars = string.chars();
        match chars.next() {
            None => String::new(),
            Some(first_char) => first_char.to_uppercase().collect::<String>() + chars.as_str(),
        }
    }

    pub fn add_prefix(string: &str, prefix: &str) -> String {
        format!("{}{}", prefix, string)
    }
}
