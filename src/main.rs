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
use std::path::{Path, PathBuf};
use std::time::Instant;
use structopt::StructOpt;

mod cli;
mod console;
mod fluree;
mod lib;

pub use cli::opt::Opt;
use cli::temp_files::TempFile;
pub use console::console::{pretty_print, ERROR_COLOR};
pub use fluree::db::get_db_name;
pub use fluree::query::SCHEMA_QUERY;
pub use lib::functions::{
    add_prefix, capitalize, instant_to_iso_string, remove_namespace, remove_underscore,
    represent_fluree_value, standardize,
};

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    let start = Instant::now();
    let green_bold = Style::new().green().bold();
    let opt = Opt::from_args();

    if opt.namespace.is_some() && opt.context.is_none() {
        panic!("You must specify a context when using the --namespace (-n) flag\n\nExample: -c schema=http://schema.org/ -n schema");
    }

    let prefix = match opt.namespace.clone() {
        Some(namespace) => format!("{}:", namespace),
        None => "".to_string(),
    };

    let mut api_key = opt.authorization;

    let mut url = match opt.url.clone() {
        Some(url) => url,
        None => Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Fluree DB URL:")
            .default("http://localhost:8090/fdb/ledger/name".to_string())
            .show_default(true)
            .validate_with({
                move |input: &String| -> Result<(), &str> {
                    if let Ok(_) = reqwest::Url::parse(input) {
                        Ok(())
                    } else {
                        Err("Please provide a valid URL")
                    }
                }
            })
            .interact_text()
            .unwrap(),
    };

    let (mut is_available, mut is_authorized) = (true, true);
    let mut response_string: Option<Value> = None;

    let pb = ProgressBar::new(2);
    pb.set_style(
        ProgressStyle::with_template(
            // note that bar size is fixed unlike cargo which is dynamic
            // and also the truncation in cargo uses trailers (`...`)
            if Term::stdout().size().1 > 80 {
                "{prefix:>12.cyan.bold} [{bar:57}] {pos}/{len} {wide_msg}"
            } else {
                "{prefix:>12.cyan.bold} [{bar:57}] {pos}/{len}"
            },
        )
        .unwrap()
        .progress_chars("=> "),
    );
    pb.set_prefix("Processing Fluree v3 Vocabulary");

    while !is_available || !is_authorized || response_string.is_none() {
        if !is_available {
            url = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Fluree DB URL:")
                .default("http://localhost:8090/fdb/ledger/name".to_string())
                .show_default(true)
                .validate_with({
                    move |input: &String| -> Result<(), &str> {
                        if let Ok(_) = reqwest::Url::parse(input) {
                            Ok(())
                        } else {
                            Err("Please provide a valid URL")
                        }
                    }
                })
                .interact_text()
                .unwrap();
        }
        if !is_authorized {
            api_key = Some(
                Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("Nexus API Key:")
                    .interact_text()
                    .unwrap(),
            );
        }
        if pb.is_finished() {
            pb.reset();
        }

        let client = reqwest::Client::new();
        let mut request_headers = HeaderMap::new();
        request_headers.insert("Content-Type", "application/json".parse().unwrap());
        if let Some(auth) = api_key.clone() {
            request_headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", &auth)).unwrap(),
            );
        }

        pb.println(format!(
            "{:>12} v2 Schema",
            green_bold.apply_to("Extracting")
        ));
        pb.inc(1);

        let response_result = client
            .post(&format!("{}/multi-query", url))
            .headers(request_headers)
            .body(SCHEMA_QUERY)
            .send()
            .await;
        (is_available, is_authorized) = match &response_result {
            Ok(response) => match response.status() {
                reqwest::StatusCode::OK => (true, true),
                reqwest::StatusCode::UNAUTHORIZED => {
                    match api_key {
                        Some(_) => {
                            pretty_print(
                                    "The API Key you provided is not authorized to access this database. Please try again.",
                                    ERROR_COLOR,
                                    true,
                                );
                        }
                        None => {
                            pb.finish_and_clear();
                            pretty_print(
                                    "It appears you need to provide an API Key to access this database. Please try again.",
                                    ERROR_COLOR,
                                    true,
                                );
                        }
                    };
                    (true, false)
                }
                _ => {
                    pretty_print(
                        &format!(
                            "The request to [{}] returned a status code of {}. Please try again.",
                            url,
                            response.status()
                        ),
                        ERROR_COLOR,
                        true,
                    );
                    (false, !api_key.is_some())
                }
            },
            Err(_) => {
                execute!(
                    stdout(),
                    SetForegroundColor(ERROR_COLOR),
                    Print("The request to the database failed. Please try again."),
                    Print("\n"),
                    ResetColor
                )
                .unwrap();
                (false, false)
            }
        };
        match (is_available, is_authorized) {
            (true, true) => {
                let awaited_response = response_result.unwrap().text().await.unwrap();
                response_string = serde_json::from_str(&awaited_response).unwrap();
                break;
            }
            _ => {
                pb.finish_and_clear();
                continue;
            }
        }
    }

    pb.println(format!(
        "{:>12} v2 Data Modeling",
        green_bold.apply_to("Parsing")
    ));
    pb.inc(1);

    let json: serde_json::Value = response_string.unwrap();
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
    let json = serde_json::json!(current_predicates);
    let mut context: HashMap<String, String> = HashMap::new();
    match opt.context {
        Some(contexts) => {
            for context_string in contexts {
                let mut context_parts = context_string.split("=");
                let context_key = context_parts.next().expect("When using --context (-c), you must specify a key and value separated by an equals sign (e.g. --context schema=http://schema.org/)");
                let context_value = context_parts.next().expect("When using --context (-c), you must specify a key and value separated by an equals sign (e.g. --context schema=http://schema.org/)");
                context.insert(context_key.to_string(), context_value.to_string());
            }
        }
        None => {}
    }

    match opt.base {
        Some(base) => {
            context.insert("@base".to_string(), base);
        }
        None => {
            if opt.namespace.is_none() {
                context.insert("@base".to_string(), format!("{}/terms/", url));
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

    let mut canonical_classes: HashMap<String, Value> = HashMap::new();
    let mut canonical_properties: HashMap<String, Value> = HashMap::new();
    let mut canonical_class_shacl_shapes: HashMap<String, Value> = HashMap::new();

    for item in json.as_array().unwrap() {
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
        let orig_class_name = name_parts[0];
        let class_name = remove_underscore(orig_class_name);
        let class_name = &capitalize(class_name);
        let class_name = standardize(class_name);
        let class_name = add_prefix(&class_name, &prefix);
        let orig_property_name = name_parts[1];
        let property_name = standardize(orig_property_name);
        let property_name = add_prefix(&property_name, &prefix);

        let keys = item.as_object().unwrap().keys();

        let class_object = canonical_classes.get(&orig_class_name.to_owned());
        let mut class_object = match class_object {
            Some(class_object) => class_object.to_owned(),
            None => Value::Object(serde_json::Map::new()),
        };

        class_object["@type"] = Value::String("rdfs:Class".to_string());

        let property_object = canonical_properties.get(orig_property_name);
        let mut property_object = match property_object {
            Some(property_object) => property_object.to_owned(),
            None => Value::Object(serde_json::Map::new()),
        };

        property_object["@type"] = Value::String("rdf:Property".to_string());

        let class_shacl_shape = canonical_class_shacl_shapes.get(orig_class_name);
        let mut class_shacl_shape = match class_shacl_shape {
            Some(class_shacl_shape) => class_shacl_shape.to_owned(),
            None => Value::Object(serde_json::Map::new()),
        };

        class_shacl_shape["@type"] = Value::String("sh:NodeShape".to_string());
        class_shacl_shape["sh:targetClass"] = serde_json::json!({ "@id": class_name });
        let shacl_properties = class_shacl_shape.get("sh:property");
        let mut shacl_properties = match shacl_properties {
            Some(shacl_properties) => {
                if let Value::Array(shacl_properties) = shacl_properties {
                    shacl_properties.to_owned()
                } else {
                    panic!(
                        "sh:property must be an array (e.g. [{{ \"sh:path\": \"schema:name\" }}])"
                    );
                }
            }
            None => Vec::<Value>::new(),
        };

        class_object["@id"] = Value::String(String::from(&class_name));
        class_object["rdfs:label"] = Value::String(String::from(remove_namespace(&class_name)));
        let mut range: Vec<Value> = match class_object.get("rdfs:range") {
            Some(range) => {
                let range: Vec<Value> = match range {
                    Value::Array(range) => range.to_owned(),
                    _ => panic!("rdfs:range must be an array (e.g. [\"xsd:string\"])"),
                };
                range
            }
            None => {
                let range: Vec<Value> = Vec::new();
                range
            }
        };
        range.push(serde_json::json!({ "@id": property_name }));
        class_object["rdfs:range"] = range.into();
        property_object["@id"] = Value::String(property_name.to_string());
        property_object["rdfs:label"] =
            Value::String(String::from(remove_namespace(&property_name)));
        let mut domain: Vec<Value> = match property_object.get("rdfs:domain") {
            Some(domain) => {
                let domain: Vec<Value> = match domain {
                    Value::Array(domain) => domain.to_owned(),
                    _ => panic!("rdfs:domain must be an array (e.g. [\"schema:Person\"])"),
                };
                domain
            }
            None => {
                let domain: Vec<Value> = Vec::new();
                domain
            }
        };
        domain.push(serde_json::json!({ "@id": class_name }));
        property_object["rdfs:domain"] = domain.into();

        let mut shacl_property = serde_json::json!({
            "sh:path": { "@id": property_name},
        });

        // if item["multi"] does not exist or is false, then add
        // shacl_property["sh:maxCount"] = Value::Number(serde_json::Number::from(1));
        if item["multi"].is_null() || !item["multi"].as_bool().unwrap() {
            shacl_property["sh:maxCount"] = Value::Number(serde_json::Number::from(1));
        }

        for key in keys {
            match key.as_str() {
                // "_id" => {}
                "doc" => {
                    property_object["rdfs:comment"] =
                        Value::String(item["doc"].as_str().unwrap().to_string());
                }
                "type" => {
                    let type_value = item["type"].as_str().unwrap();
                    match type_value {
                        "float" | "int" | "instant" | "boolean" | "long" | "string" | "ref" => {
                            let data_type = match type_value {
                                "int" => Value::String("xsd:integer".to_string()),
                                "instant" => Value::String("xsd:dateTime".to_string()),
                                "ref" => Value::String("xsd:anyURI".to_string()),
                                _ => Value::String(format!("xsd:{}", type_value)),
                            };
                            shacl_property["sh:datatype"] = json!({ "@id": data_type })
                        }
                        "tag" => {
                            // TODO: Figure out how to handle tag types
                        }
                        _ => {}
                    }
                }
                "restrictCollection" => {
                    shacl_property["sh:class"] = serde_json::json!({ "@id": add_prefix(&capitalize(&standardize(item["restrictCollection"].as_str().unwrap())), &prefix) });
                }
                "restrictTag" => {
                    // this is a boolean
                }
                "unique" => {}
                "index" => {}
                "fullText" => {}
                "upsert" => {}
                _ => {}
            }
        }
        shacl_properties.push(shacl_property);
        class_shacl_shape["sh:property"] = shacl_properties.into();
        canonical_class_shacl_shapes.insert(orig_class_name.to_string(), class_shacl_shape);
        canonical_classes.insert(orig_class_name.to_string(), class_object);
        canonical_properties.insert(orig_property_name.to_string(), property_object);

        // let id = item["_id"].as_u64().unwrap();
        // let name = item["name"].as_str().unwrap();
        // let doc = item["doc"].as_str().unwrap();
        // let type_ = item["type"].as_str().unwrap();

        // let mut id_map = serde_json::Map::new();
        // id_map.insert("@id".to_string(), Value::String(format!("{}:{}", name, id)));
        // jsonld_item_map.insert("@id".to_string(), Value::Object(id_map));

        // let mut type_map = serde_json::Map::new();
        // type_map.insert("@type".to_string(), Value::String("rdfs:Class".to_string()));
        // jsonld_item_map.insert("@type".to_string(), Value::Object(type_map));

        // let mut label_map = serde_json::Map::new();
        // label_map.insert("rdfs:label".to_string(), Value::String(name.to_string()));
        // jsonld_item_map.insert("rdfs:label".to_string(), Value::Object(label_map));

        // let mut comment_map = serde_json::Map::new();
        // comment_map.insert("rdfs:comment".to_string(), Value::String(doc.to_string()));
        // jsonld_item_map.insert("rdfs:comment".to_string(), Value::Object(comment_map));

        // jsonld.push(jsonld_item);
    }
    let classes = canonical_classes.values();
    let properties = canonical_properties.values();
    let class_shacl_shapes = canonical_class_shacl_shapes.values();

    // i need to create a results variable that is the classes and properties values, as well as the values within Vec<Value> shacl_shapes
    let results: Vec<Value> = match opt.shacl {
        true => classes
            .chain(properties)
            .chain(class_shacl_shapes)
            .map(|value| value.to_owned())
            .collect(),
        false => classes
            .chain(properties)
            .map(|value| value.to_owned())
            .collect(),
    };

    let query_classes: Vec<String> = canonical_classes.keys().map(|key| key.to_owned()).collect();

    let mut vocab_results_map = serde_json::Map::new();
    if opt.url.is_some() {
        let url = opt.url.unwrap();
        let (network_name, db_name) = get_db_name(&url);
        vocab_results_map.insert(
            "@id".to_string(),
            json!(format!("{}/{}", network_name, db_name)),
        );
    }
    let mut data_results_map = vocab_results_map.clone();
    if opt.is_create_ledger {
        vocab_results_map.insert(
            "@context".to_string(),
            json!({ "f": "https://ns.flur.ee/ledger#"}),
        );
        vocab_results_map.insert(
            "f:defaultContext".to_string(),
            Value::Object(
                context
                    .iter()
                    .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                    .collect(),
            ),
        );
    } else {
        vocab_results_map.insert(
            "@context".to_string(),
            Value::Object(
                context
                    .iter()
                    .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                    .collect(),
            ),
        );
    }
    data_results_map.insert(
        "@context".to_string(),
        Value::Object(
            context
                .iter()
                .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                .collect(),
        ),
    );
    vocab_results_map.insert("@graph".to_string(), Value::Array(results));
    data_results_map.insert("@graph".to_string(), json!([]));

    let client = reqwest::Client::new();
    let mut request_headers = HeaderMap::new();
    request_headers.insert("Content-Type", "application/json".parse().unwrap());
    if let Some(auth) = api_key.clone() {
        request_headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", &auth)).unwrap(),
        );
    }

    pb.inc_length(query_classes.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            // note that bar size is fixed unlike cargo which is dynamic
            // and also the truncation in cargo uses trailers (`...`)
            if Term::stdout().size().1 > 80 {
                "{prefix:>12.cyan.bold} [{bar:57}] {pos}/{len} {wide_msg}"
            } else {
                "{prefix:>12.cyan.bold} [{bar:57}] {pos}/{len}"
            },
        )
        .unwrap()
        .progress_chars("=> "),
    );
    pb.set_prefix("Transforming Fluree v2 Entities");

    for class_name in query_classes {
        let mut results: Vec<Value> = Vec::new();
        let mut offset: u32 = 0;

        pb.println(format!(
            "{:>12} {} Data",
            green_bold.apply_to("Transforming"),
            standardize(&capitalize(&class_name))
        ));
        pb.inc(1);
        loop {
            let query = format!(
                r#"{{
                    "select": ["*"],
                    "from": "{}",
                    "opts": {{
                        "compact": true,
                        "limit": 2000,
                        "fuel": 9999999999,
                        "offset": {}
                    }}
                }}"#,
                class_name, offset
            );

            let response_result = client
                .post(&format!("{}/query", url))
                .headers(request_headers.clone())
                .body(query)
                .send()
                .await;
            let response = response_result.unwrap().text().await.unwrap();
            let response: Value = serde_json::from_str(&response).unwrap();
            let response = response.as_array().unwrap();

            if response.len() == 0 {
                break;
            }
            results = match offset {
                0 => response.to_owned(),
                _ => results.into_iter().chain(response.to_owned()).collect(),
            };
            // let results_length = results.len();
            // println!(
            //     "In collection {}; data_size is {}",
            //     class_name,
            //     results.len()
            // );
            // if results_length > 100_000 {
            //     temp_file.write(&class_name, &results).expect(
            //         format!("Issue writing file for {} at offset {}", class_name, offset).as_str(),
            //     );
            //     results.clear();
            // }
            offset += 2000;
        }
        let mut vec_parsed_results = Vec::new();
        // let files = temp_file.get_files().expect("Could not get files");

        // p

        for result in results {
            let mut parsed_result: HashMap<String, Value> = HashMap::new();
            let string_id: String = result["_id"].to_string();
            parsed_result.insert("@id".to_string(), json!(string_id));
            parsed_result.insert(
                "@type".to_string(),
                canonical_classes
                    .get(&class_name)
                    .unwrap()
                    .get("@id")
                    .unwrap()
                    .to_owned(),
            );
            for (key, value) in result.as_object().unwrap() {
                if let Some(canonical_property) = canonical_properties.get(key) {
                    let key = canonical_property.get("@id").unwrap();
                    let shacl_shape = canonical_class_shacl_shapes.get(&class_name).unwrap();
                    let shacl_properties = shacl_shape.get("sh:property").unwrap();
                    let is_datetime = match shacl_properties.as_array().unwrap().iter().find(|&x| {
                        let shacl_path = &x["sh:path"]["@id"];
                        let y = Value::String("xsd:dateTime".to_string());
                        shacl_path == key && x["sh:datatype"]["@id"] == y
                    }) {
                        Some(_) => true,
                        None => false,
                    };
                    let value = match is_datetime {
                        true => json!(instant_to_iso_string(value.as_i64().unwrap())),
                        false => value.to_owned(),
                    };
                    let key_string = match key {
                        Value::String(value) => value.to_owned(),
                        _ => key.to_string(),
                    };
                    parsed_result.insert(key_string, represent_fluree_value(&value));
                }
            }
            vec_parsed_results.push(json!(parsed_result));
        }

        data_results_map
            .entry("@graph".to_string())
            .and_modify(|e| {
                if let Value::Array(array) = e {
                    array.extend(vec_parsed_results);
                }
            });
    }

    pb.finish_and_clear();

    match opt.output.clone() {
        Some(output) => {
            let output = output.clone();
            let output = output.to_str().unwrap();
            let path = PathBuf::from(output);
            std::fs::create_dir_all(&output).unwrap();
            let mut schema_file = File::create(path.join("vocab.jsonld")).unwrap();
            let mut data_file = File::create(path.join("data.jsonld")).unwrap();
            let mut schema_writer = io::BufWriter::new(&mut schema_file);
            let mut data_writer = io::BufWriter::new(&mut data_file);
            schema_writer
                .write_all(
                    serde_json::to_string_pretty(&vocab_results_map)
                        .unwrap()
                        .as_bytes(),
                )
                .expect("Could not write to file");
            data_writer
                .write_all(
                    serde_json::to_string_pretty(&data_results_map)
                        .unwrap()
                        .as_bytes(),
                )
                .expect("Could not write to file");
            // let mut file = File::create(output).expect("Could not create file");
            // file.write_all(
            //     serde_json::to_string_pretty(&results_map)
            //         .unwrap()
            //         .as_bytes(),
            // )
            // .expect("Could not write to file");
        }
        None => {
            println!(
                "{}",
                serde_json::to_string_pretty(&vocab_results_map).unwrap()
            );
            println!(
                "{}",
                serde_json::to_string_pretty(&data_results_map).unwrap()
            );
        }
    }

    let finish_line = match opt.output {
        Some(output) => format!("to {}/ ", output.to_str().unwrap()),
        None => "".to_string(),
    };
    println!(
        "{:>12} v3 Migration {}in {}",
        green_bold.apply_to("Finished"),
        finish_line,
        HumanDuration(start.elapsed()),
    );

    Ok(())
}
