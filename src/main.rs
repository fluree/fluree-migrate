use dialoguer::console::{Style, Term};
use indicatif::{HumanDuration, ProgressBar, ProgressStyle};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use std::vec;
use structopt::StructOpt;

mod cli;
mod console;
mod fluree;
mod functions;

use cli::opt::Opt;
use cli::parser::Parser;
use cli::temp_files::TempFile;
use fluree::FlureeInstance;
use functions::{
    capitalize, case_normalize, instant_to_iso_string, parse_current_predicates,
    parse_for_class_and_property_name, represent_fluree_value, standardize_class_name,
    standardize_property_name, write_or_print,
};

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    let start = Instant::now();
    let green_bold = Style::new().green().bold();
    let opt = Opt::from_args();

    opt.validate_namespace_and_context();

    let prefix = match opt.namespace.clone() {
        Some(namespace) => format!("{}:", namespace),
        None => "".to_string(),
    };

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

    let mut source_instance = FlureeInstance::new(&opt);

    while !source_instance.is_available
        || !source_instance.is_authorized
        || response_string.is_none()
    {
        if !source_instance.is_available {
            source_instance.prompt_fix_url();
        }

        if !source_instance.is_authorized {
            source_instance.prompt_api_key();
        }
        if pb.is_finished() {
            pb.reset();
        }

        pb.println(format!(
            "{:>12} v2 Schema",
            green_bold.apply_to("Extracting")
        ));
        pb.inc(1);

        let response_result = source_instance.issue_initial_query().await;

        source_instance.validate_result(&response_result);

        if source_instance.is_available && source_instance.is_authorized {
            let awaited_response = response_result.unwrap().text().await.unwrap();
            response_string = serde_json::from_str(&awaited_response).unwrap();
            break;
        } else {
            pb.finish_and_clear();
            continue;
        }
    }

    pb.println(format!(
        "{:>12} v2 Data Modeling",
        green_bold.apply_to("Parsing")
    ));
    pb.inc(1);

    let json = parse_current_predicates(response_string.unwrap());

    let mut parser = Parser::new(prefix, &opt, &source_instance);

    for item in json.as_array().unwrap() {
        let (orig_class_name, orig_property_name) = parse_for_class_and_property_name(item);

        let mut class_object = parser.get_or_create_class(&orig_class_name);

        let mut property_object = parser.get_or_create_property(&orig_property_name);

        let mut class_shacl_shape = parser.get_or_create_shacl_shape(&orig_class_name);

        let class_name = standardize_class_name(&orig_class_name, &parser.prefix);
        let property_name = standardize_property_name(&orig_property_name, &parser.prefix);

        class_object.set_property_range(&property_name);
        property_object.set_class_domain(&class_name);

        class_shacl_shape.set_property(&mut property_object, item, &parser.prefix);

        parser
            .shacl_shapes
            .insert(orig_class_name.to_string(), class_shacl_shape);
        parser
            .classes
            .insert(orig_class_name.to_string(), class_object);
        parser
            .properties
            .insert(orig_property_name.to_string(), property_object);
    }

    let vocab_results_map = parser.get_vocab_json(&opt);
    if opt.output.is_some() {
        std::fs::remove_dir_all(opt.output.clone().unwrap()).unwrap_or_else(|why| {
            if why.kind() != std::io::ErrorKind::NotFound {
                panic!("Unable to remove existing output directory: {}", why);
            }
        });
    }
    write_or_print(
        &opt.output,
        "0_vocab.jsonld",
        serde_json::to_string_pretty(&vocab_results_map).unwrap(),
    );

    let query_classes: Vec<String> = parser.classes.keys().map(|key| key.to_owned()).collect();

    let mut data_results_map = serde_json::Map::new();
    data_results_map.insert(
        "f:ledger".to_string(),
        json!(format!(
            "{}/{}",
            source_instance.network_name, source_instance.db_name
        )),
    );

    data_results_map.insert(
        "@context".to_string(),
        Value::Object(
            parser
                .context
                .iter()
                .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                .collect(),
        ),
    );

    data_results_map.insert("@graph".to_string(), json!([]));

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

    let temp_dir = Path::new(".tmp");
    let mut temp_file = TempFile::new(temp_dir).expect("Could not create temp file");

    for class_name in query_classes {
        let mut results: Vec<Value> = Vec::new();
        let mut offset: u32 = 0;

        pb.println(format!(
            "{:>12} {} Data",
            green_bold.apply_to("Transforming"),
            case_normalize(&capitalize(&class_name))
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

            let response_result = source_instance.issue_data_query(query).await;
            let response = response_result.unwrap().text().await.unwrap();
            let response: Value = serde_json::from_str(&response).unwrap();
            let response = response.as_array().unwrap();

            if response.len() == 0 {
                temp_file
                    .write(&class_name, &results)
                    .expect(format!("Issue writing file for {}", class_name).as_str());
                results.clear();
                break;
            }

            results = match offset {
                0 => response.to_owned(),
                _ => results.into_iter().chain(response.to_owned()).collect(),
            };

            let results_length = results.len();

            if results_length > 5_000 {
                temp_file.write(&class_name, &results).expect(
                    format!("Issue writing file for {} at offset {}", class_name, offset).as_str(),
                );
                results.clear();
            }

            offset += 2000;
        }
    }
    let mut vec_parsed_results = Vec::new();
    let files = temp_file.get_files().expect("Could not get files");

    let mut result_size: u64 = 0;
    let mut file_num: u64 = 1;

    for file in files {
        result_size += file.metadata().expect("Could not get metadata").len();

        let file_bytes = std::fs::read(&file).expect("Could not read file");
        let file_string = String::from_utf8(file_bytes).expect("Could not convert to string");
        let results: Vec<Value> = serde_json::from_str(&file_string).expect("Could not parse JSON");
        let orig_class_name = file
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .split("__")
            .collect::<Vec<&str>>()
            .last()
            .unwrap()
            .to_string();

        for result in results {
            let mut parsed_result: HashMap<String, Value> = HashMap::new();
            let string_id: String = result["_id"].to_string();
            parsed_result.insert("@id".to_string(), json!(string_id));

            let class_name = match parser.classes.get(&orig_class_name) {
                Some(class) => class.id.to_owned(),
                None => panic!("Could not find class {}", orig_class_name),
            };

            parsed_result.insert("@type".to_string(), serde_json::json!(&class_name));
            for (key, value) in result.as_object().unwrap() {
                if let Some(canonical_property) = parser.properties.get(key) {
                    let key = canonical_property.id.to_owned();
                    let shacl_shape = parser.shacl_shapes.get(&orig_class_name).unwrap();
                    let shacl_properties = &shacl_shape.property;
                    let is_datetime = match shacl_properties.iter().find(|&x| {
                        let shacl_path = x.path.get("@id").unwrap();
                        let y = "xsd:dateTime";
                        if x.datatype.is_none() {
                            return false;
                        }
                        shacl_path == &key && x.datatype.clone().unwrap().get("@id").unwrap() == y
                    }) {
                        Some(_) => true,
                        None => false,
                    };
                    let value = match is_datetime {
                        true => json!(instant_to_iso_string(value.as_i64().unwrap())),
                        false => value.to_owned(),
                    };
                    // let key_string = match key {
                    //     Value::String(value) => value.to_owned(),
                    //     _ => key.to_string(),
                    // };
                    parsed_result.insert(key, represent_fluree_value(&value));
                }
            }
            vec_parsed_results.push(json!(parsed_result));
        }

        data_results_map
            .entry("@graph".to_string())
            .and_modify(|e| {
                if let Value::Array(array) = e {
                    array.extend(vec_parsed_results.clone());
                }
            });

        vec_parsed_results.clear();

        // if result_size is greater than 5mb, write to file
        if result_size > 2_500_000 {
            write_or_print(
                &opt.output,
                format!("{}_data.jsonld", file_num),
                serde_json::to_string_pretty(&data_results_map).unwrap(),
            );

            result_size = 0;
            file_num += 1;
            vec_parsed_results.clear();
            data_results_map
                .entry("@graph".to_string())
                .and_modify(|e| {
                    *e = serde_json::json!([]);
                });
        }

        // delete file
        std::fs::remove_file(file).expect("Could not remove file");
    }
    std::fs::remove_dir_all(temp_dir).expect("Could not remove temp directory");

    write_or_print(
        &opt.output,
        format!("{}_data.jsonld", file_num),
        serde_json::to_string_pretty(&data_results_map).unwrap(),
    );

    pb.finish_and_clear();

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
