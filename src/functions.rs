use chrono::{DateTime, NaiveDateTime, Utc};
use crossterm::execute;
use crossterm::style::{Print, ResetColor, SetForegroundColor};
use dialoguer::console::Style;
use indicatif::ProgressBar;
use log::{log_enabled, Level};
use serde_json::Value;
use std::collections::HashMap;
use std::io::stdout;

use crate::cli::opt::Opt;
use crate::console::ERROR_COLOR;
// use crate::cli::opt::Opt;
use crate::fluree::FlureeInstance;

// I have epoch instant values like 1693403567000 but want to convert them to ISO strings like "2023-08-30T13:52:47.000Z"
pub fn instant_to_iso_string(epoch: i64) -> String {
    let naive =
        NaiveDateTime::from_timestamp_millis(epoch).expect("DateTime value is out of range");
    let date_time: DateTime<Utc> = DateTime::from_naive_utc_and_offset(naive, Utc);
    date_time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn represent_fluree_value(value: &Value, ref_type: Option<String>) -> Value {
    match value {
        Value::Object(value) => {
            let mut json = serde_json::json!({});
            json["@id"] = value["_id"].to_string().into();
            if let Some(ref_type) = ref_type {
                json["@type"] = ref_type.into();
            }
            json
        }
        Value::Array(value) => {
            let mut array: Vec<Value> = Vec::new();
            for item in value {
                array.push(represent_fluree_value(item, ref_type.clone()));
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
pub fn remove_namespace(string: &str) -> String {
    let mut split = string.split(":");
    let first = split.next();
    let second = split.next();
    match (first, second) {
        (Some(_), Some(second)) => second,
        _ => string,
    }
    .to_owned()
}

// remove leading "_" from a string, so "_fn" should return "fn", "_auth" should return "auth", etc.
// pub fn remove_underscore(string: &str) -> &str {
//     if string.starts_with("_") {
//         &string[1..]
//     } else {
//         string
//     }
// }

// standardize a string so that it goes from snake_case to CamelCase. The one exception is that if the leading char is uncapitalized, it should remain uncapitalized
pub fn case_normalize(string: &str) -> String {
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

pub fn standardize_class_name(string: &str) -> String {
    let string = remove_namespace(string);
    let string = capitalize(&string);
    let string = case_normalize(&string);
    string
}

pub fn standardize_property_name(string: &str) -> String {
    case_normalize(string)
}

pub fn parse_current_predicates(json: Value) -> Value {
    if json["current_predicates"].is_null() || json["initial_predicates"].is_null() {
        execute!(
            stdout(),
            SetForegroundColor(ERROR_COLOR),
            Print("ERROR: "),
            Print("Attempting to retrieve the schema from the database failed. If you provided an API Key, please check that it is correct. If you did not provide an API Key, please check that the database is running and that you have access to it."),
            Print("\n"),
            ResetColor
        ).unwrap();
        std::process::exit(1);
    }
    let pre_reduce_preds = json["current_predicates"].as_array().unwrap();
    let initial_predicates = json["initial_predicates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_i64().unwrap())
        .collect::<Vec<i64>>();
    let current_predicates = pre_reduce_preds
        .iter()
        .filter(|value| {
            let id = value["_id"].as_i64().unwrap();
            !initial_predicates.contains(&id)
        })
        .collect::<Vec<&Value>>();
    serde_json::json!(current_predicates)
}

pub fn create_context(
    opt: &Opt,
    source_instance: &FlureeInstance,
    is_vocab: bool,
) -> HashMap<String, String> {
    let mut context: HashMap<String, String> = HashMap::new();

    match (&opt.base, &opt.vocab) {
        (Some(base), Some(vocab)) => {
            if is_vocab {
                context.insert("@base".to_string(), vocab.clone());
            } else {
                context.insert("@base".to_string(), base.clone());
                context.insert("@vocab".to_string(), vocab.clone());
            }
        }
        (Some(base), None) => {
            if is_vocab {
                context.insert(
                    "@base".to_string(),
                    format!("{}/terms/", source_instance.url),
                );
            } else {
                context.insert("@base".to_string(), base.clone());
                context.insert(
                    "@vocab".to_string(),
                    format!("{}/terms/", source_instance.url),
                );
            }
        }
        (None, Some(vocab)) => {
            if is_vocab {
                context.insert("@base".to_string(), vocab.clone());
            } else {
                context.insert("@base".to_string(), format!("{}/ids/", source_instance.url));
                context.insert("@vocab".to_string(), vocab.clone());
            }
        }
        (None, None) => {
            if is_vocab {
                context.insert(
                    "@base".to_string(),
                    format!("{}/terms/", source_instance.url),
                );
            } else {
                context.insert("@base".to_string(), format!("{}/ids/", source_instance.url));
                context.insert(
                    "@vocab".to_string(),
                    format!("{}/terms/", source_instance.url),
                );
            }
        }
    }

    if opt.shacl {
        context.insert("sh".to_string(), "http://www.w3.org/ns/shacl#".to_string());
        context.insert(
            "xsd".to_string(),
            "http://www.w3.org/2001/XMLSchema#".to_string(),
        );
    }

    context.insert(
        "rdfs".to_string(),
        "http://www.w3.org/2000/01/rdf-schema#".to_string(),
    );
    context.insert(
        "rdf".to_string(),
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#".to_string(),
    );
    context.insert("f".to_string(), "https://ns.flur.ee/ledger#".to_string());
    context
}

pub fn create_data_context(opt: &Opt, source_instance: &FlureeInstance) -> HashMap<String, String> {
    create_context(opt, source_instance, false)
}

pub fn create_vocab_context(
    opt: &Opt,
    source_instance: &FlureeInstance,
) -> HashMap<String, String> {
    create_context(opt, source_instance, true)
}

pub fn parse_for_class_and_property_name(item: &Value) -> (String, String) {
    let item_id = item["_id"]
        .as_i64()
        .expect("An item in the JSON array does not have an _id");
    let item_name = item["name"].as_str().expect(
        format!(
            "An item in the JSON array does not have a name: {:?}",
            item_id
        )
        .as_str(),
    );
    let mut name_split = item_name.split("/");
    let name_parts: [&str; 2] = [
        name_split.next().expect(
            format!(
                "{} does not have a collection and property name (e.g. collection/property)",
                item_name
            )
            .as_str(),
        ),
        name_split.next().expect(
            format!(
                "{} does not have a collection and property name (e.g. collection/property)",
                item_name
            )
            .as_str(),
        ),
    ];

    let orig_class_name = name_parts[0].to_string();
    let orig_property_name = name_parts[1].to_string();
    (orig_class_name, orig_property_name)
}

pub fn pretty_log(level: Level, pb: &mut ProgressBar, message: &str) {
    let info_color: Style = Style::new().green().bold();
    let warn_color: Style = Style::new().yellow().bold();
    let error_color: Style = Style::new().red().bold();
    let debug_color: Style = Style::new().cyan().bold();

    if log_enabled!(level) {
        pb.println(format!(
            "{:>5}: {}",
            match level {
                Level::Info => info_color.apply_to("INFO"),
                Level::Warn => warn_color.apply_to("WARN"),
                Level::Error => error_color.apply_to("ERROR"),
                Level::Debug => debug_color.apply_to("DEBUG"),
                _ => debug_color.apply_to("DEBUG"),
            },
            message
        ))
    }
}

pub fn truncate_tail(string: &str, length: usize) -> String {
    let start_index = if string.chars().count() > length - 3 {
        string
            .char_indices()
            .nth(string.chars().count() - length)
            .unwrap()
            .0
    } else {
        0
    };

    // Slice the string safely to get the last 30 characters.
    if start_index > 0 {
        format!("...{}", &string[start_index..])
    } else {
        string.to_string()
    }
}

pub fn format_bytes(size: usize) -> String {
    let units = ["bytes", "KB", "MB", "GB", "TB", "PB", "EB"];
    let size = size as f64;
    if size < 1_f64 {
        return format!("{:.1} bytes", size);
    }
    let delimiter = 1024_f64;
    let exponent = (size.ln() / delimiter.ln()).floor() as i32;
    let pretty_size = size / delimiter.powi(exponent);
    let unit = units[exponent as usize];
    format!("{:.1} {}", pretty_size, unit)
}
