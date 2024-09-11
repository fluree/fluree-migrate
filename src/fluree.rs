use std::collections::{HashMap, HashSet};
use std::io::stdout;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::execute;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use dialoguer::console::{Style, Term};
use dialoguer::{theme::ColorfulTheme, Input};
use indicatif::{HumanDuration, ProgressStyle};
use reqwest::{header::HeaderMap, Client, Error, Response};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::cli::opt::Opt;
use crate::cli::parser::Parser;
use crate::cli::source::Migrate;
use crate::cli::temp_files::TempFile;
use crate::console::{pretty_print, ERROR_COLOR};
use crate::functions::{
    capitalize, case_normalize, instant_to_iso_string, parse_current_predicates,
    parse_for_class_and_property_name, represent_fluree_value, standardize_class_name,
    standardize_property_name,
};

const SCHEMA_QUERY: &str = r#"{
    "initial_predicates": {
        "select": "?pred",
        "where": [
            ["?pred", "_predicate/name", "?pN"]
        ],
        "block": 1,
        "opts": {
            "limit": 9999999
        }
    },
    "current_predicates": {
        "select": {"?pred": ["*"]},
        "where": [
            ["?pred", "_predicate/name", "?pN"]
        ],
        "opts": {
            "compact": true,
            "limit": 9999999
        }
    }
}"#;

#[derive(Debug, Clone)]
pub struct FlureeInstance {
    pub url: String,
    pub network_name: String,
    pub db_name: String,
    pub is_available: bool,
    pub is_authorized: bool,
    pub api_key: Option<String>,
    pub client: Client,
    pub is_created: bool,
    pub opt: Opt,
}

impl FlureeInstance {
    pub fn new_source(opt: &Opt) -> Self {
        let url = opt.check_url(true);
        let (network_name, db_name) = Self::get_db_name(&url);
        FlureeInstance {
            url: url.to_string(),
            network_name,
            db_name,
            is_available: true,
            is_authorized: true,
            api_key: opt.source_auth.clone(),
            client: reqwest::Client::new(),
            is_created: true,
            opt: opt.clone(),
        }
    }

    pub fn new_target(opt: &Opt) -> Self {
        let url = opt.check_url(false);
        let (network_name, db_name) = Self::get_db_name(&url);
        let is_created = !opt.is_create_ledger;
        FlureeInstance {
            url: url.to_string(),
            network_name,
            db_name,
            is_available: true,
            is_authorized: true,
            api_key: opt.target_auth.clone(),
            client: reqwest::Client::new(),
            is_created,
            opt: opt.clone(),
        }
    }

    fn get_db_name(url: &str) -> (String, String) {
        let mut url_parts = url
            .split("/")
            .collect::<Vec<&str>>()
            .into_iter()
            .rev()
            .take(2);
        let db_name = url_parts.next().unwrap();
        let network_name = url_parts.next().unwrap();
        (network_name.to_string(), db_name.to_string())
    }

    pub fn prompt_fix_url(&mut self) {
        self.url = Input::with_theme(&ColorfulTheme::default())
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

    pub fn prompt_api_key(&mut self) {
        self.api_key = Some(
            Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Nexus API Key:")
                .interact_text()
                .unwrap(),
        );
    }

    pub async fn v3_transact(&mut self, body: String) -> Result<Response, Error> {
        let mut request_headers = HeaderMap::new();
        request_headers.insert("Content-Type", "application/json".parse().unwrap());
        if let Some(auth) = self.api_key.clone() {
            request_headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("{}", &auth)).unwrap(),
            );
        }

        let path = if self.is_created {
            "transact"
        } else {
            "create"
        };

        self.is_created = true;

        self.client
            .post(&format!("{}/fluree/{}", self.url, path))
            .headers(request_headers)
            .body(body)
            .send()
            .await
    }

    pub async fn v3_query(&mut self, body: String) -> Result<Response, Error> {
        let mut request_headers = HeaderMap::new();
        request_headers.insert("Content-Type", "application/json".parse().unwrap());
        if let Some(auth) = self.api_key.clone() {
            request_headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("{}", &auth)).unwrap(),
            );
        }

        self.client
            .post(&format!("{}/fluree/query", self.url))
            .headers(request_headers)
            .body(body)
            .send()
            .await
    }

    pub async fn issue_initial_query(&self) -> Result<Response, Error> {
        let mut request_headers = HeaderMap::new();
        request_headers.insert("Content-Type", "application/json".parse().unwrap());
        if let Some(auth) = self.api_key.clone() {
            request_headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", &auth)).unwrap(),
            );
        }
        self.client
            .post(&format!("{}/multi-query", self.url))
            .headers(request_headers)
            .body(SCHEMA_QUERY)
            .send()
            .await
    }

    pub async fn issue_data_query(&self, query: String) -> Result<Response, Error> {
        let mut request_headers = HeaderMap::new();
        request_headers.insert("Content-Type", "application/json".parse().unwrap());
        if let Some(auth) = self.api_key.clone() {
            request_headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", &auth)).unwrap(),
            );
        }
        self.client
            .post(&format!("{}/query", self.url))
            .headers(request_headers.clone())
            .body(query)
            .send()
            .await
    }

    pub fn validate_result(&mut self, result: &Result<Response, Error>) -> Result<(), String> {
        let mut final_result = Ok(());
        (self.is_available, self.is_authorized) = match result {
            Ok(response) => match response.status() {
                reqwest::StatusCode::OK | reqwest::StatusCode::CREATED => (true, true),
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                    println!("Response: {:?}", response.error_for_status_ref());
                    match self.api_key {
                        Some(_) => {
                            final_result = Err(format!("The API Key you provided is not authorized to access this database. Please try again."));
                        }
                        None => {
                            final_result = Err(format!("It appears you need to provide an API Key to access this database. Please try again."));
                        }
                    };
                    (true, false)
                }
                _ => {
                    let url = response.url();
                    final_result = Err(format!(
                        "The request to [{}] returned a status code of {}. Please try again.",
                        url,
                        response.status()
                    ));
                    (false, !self.api_key.is_some())
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
                (false, true)
            }
        };
        final_result
    }
}

#[async_trait::async_trait]
impl Migrate for FlureeInstance {
    async fn migrate(&mut self) {
        let start = Instant::now();
        let green_bold = Style::new().green().bold();
        let yellow_bold = Style::new().yellow().bold();
        let red_bold = Style::new().red().bold();

        let mut response_string: Option<Value> = None;

        let mut source_instance = self.clone();
        let opt = self.opt.clone();

        opt.pb.set_style(
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
            // .tick_strings(&["🌲", "🎄", "🎄"])
            .progress_chars("=> "),
        );
        opt.pb.set_prefix("Processing Fluree v3 Vocabulary");

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
            if opt.pb.is_finished() {
                opt.pb.reset();
            }

            opt.pb.println(format!(
                "{:>12} v2 Schema",
                green_bold.apply_to("Extracting")
            ));
            opt.pb.inc(1);

            let response_result = source_instance.issue_initial_query().await;

            let validate_attempt = source_instance.validate_result(&response_result);

            if let Err(e) = validate_attempt {
                opt.pb
                    .println(format!("{:>12} {}", red_bold.apply_to("ERROR"), e));
            }

            if source_instance.is_available && source_instance.is_authorized {
                let awaited_response = response_result.unwrap().text().await.unwrap();
                response_string = serde_json::from_str(&awaited_response).unwrap();
                break;
            } else {
                opt.pb.finish_and_clear();
                continue;
            }
        }

        opt.pb.println(format!(
            "{:>12} v2 Data Modeling",
            green_bold.apply_to("Parsing")
        ));
        opt.pb.inc(1);

        let json = parse_current_predicates(response_string.unwrap());

        let mut parser = Parser::new(&opt, &source_instance);

        let json_results = json.as_array().unwrap();

        for item in json_results {
            let (orig_class_name, orig_property_name) = parse_for_class_and_property_name(item);

            let class_object = parser.get_or_create_class(&orig_class_name);

            let type_value = item["type"].as_str().unwrap();

            let property_obj = parser.get_or_create_property(&orig_property_name, type_value);

            parser
                .classes
                .insert(orig_class_name.to_string(), class_object);
            parser
                .properties
                .insert(orig_property_name.to_string(), property_obj);
        }

        for item in json_results {
            let (orig_class_name, orig_property_name) = parse_for_class_and_property_name(item);

            let mut class_object = parser.get_or_create_class(&orig_class_name);

            let type_value = item["type"].as_str().unwrap();

            let mut property_object =
                parser.get_or_create_property(&orig_property_name, type_value);

            let class_name = standardize_class_name(&orig_class_name);
            let property_name = standardize_property_name(&orig_property_name);

            let mut class_shacl_shape =
                parser.get_or_create_shacl_shape(&class_name, opt.closed_shapes);

            class_object.set_property_range(&property_name);
            property_object.set_class_domain(&class_name);

            // TODO: if another shacl_shape in parser.shacl_shapes has the same property name, and if it has a different datatype, then I need to log a warning and I need to update the property name to be the Class/Property (e.g. Person/age and Animal/age)

            let attempt_set_property = class_shacl_shape.set_property(&mut property_object, item);

            if let Err(e) = attempt_set_property {
                for error in e {
                    opt.pb
                        .println(format!("{:>12} {}", yellow_bold.apply_to("WARNING"), error));
                }
            }

            parser
                .shacl_shapes
                .insert(class_name.to_string(), class_shacl_shape);
            parser
                .classes
                .insert(orig_class_name.to_string(), class_object);
            parser
                .properties
                .insert(orig_property_name.to_string(), property_object);
        }

        let vocab_results_map = parser.get_vocab_json(&opt);
        if !opt.print && opt.output.is_some() {
            std::fs::remove_dir_all(opt.output.clone().unwrap()).unwrap_or_else(|why| {
                if why.kind() != std::io::ErrorKind::NotFound {
                    panic!("Unable to remove existing output directory: {}", why);
                }
            });
        }

        let mut target_instance = opt
            .write_or_print(
                "0_vocab.jsonld",
                serde_json::to_string_pretty(&vocab_results_map).unwrap(),
                None,
            )
            .await;

        let query_classes: Vec<String> = parser.classes.keys().map(|key| key.to_owned()).collect();

        let mut data_results_map = serde_json::Map::new();

        let ledger_name = match &opt.ledger_name {
            Some(ledger_name) => ledger_name.to_string(),
            None => format!("{}/{}", self.network_name, self.db_name),
        };

        data_results_map.insert("ledger".to_string(), json!(ledger_name));

        data_results_map.insert(
            "@context".to_string(),
            Value::Object(
                parser
                    .data_context
                    .iter()
                    .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                    .collect(),
            ),
        );

        data_results_map.insert("insert".to_string(), json!([]));

        opt.pb.inc_length(query_classes.len() as u64);
        opt.pb.set_style(
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
        opt.pb.set_prefix("Transforming Fluree v2 Entities");

        let temp_dir = Path::new(".tmp");
        let temp_file = TempFile::new(temp_dir).expect("Could not create temp file");
        let temp_file: Arc<_> = Arc::new(Mutex::new(temp_file));

        let mut handles = vec![];
        let semaphore = tokio::sync::Semaphore::new(10);
        let shared_fluree_instance = Arc::new(source_instance);
        let (output, target) = (opt.output.clone(), opt.target.clone());

        // processing should be a vec of the first 4 class names in query_classes
        let print_classes = query_classes
            .iter()
            .map(|x| x.to_owned())
            .take(4)
            .collect::<Vec<String>>();
        let message = print_classes.join(", ");
        let full_message = if print_classes.len() > 3 {
            format!("[{}...]", message)
        } else if print_classes.len() > 0 {
            format!("[{}]", message)
        } else {
            "".to_string()
        };
        opt.pb.set_message(full_message);

        let shared_opt = Arc::new(opt);
        let entity_map: HashMap<String, HashSet<i64>> = HashMap::new();
        let shared_entity_map = Arc::new(Mutex::new(entity_map));
        let processing = Arc::new(Mutex::new(
            query_classes
                .iter()
                .map(|x| x.to_owned())
                .collect::<Vec<String>>(),
        ));

        for class_name in query_classes {
            let permit = semaphore.acquire().await.expect("semaphore error");

            let handle = tokio::task::spawn({
                let source_instance = Arc::clone(&shared_fluree_instance);
                let temp_file = Arc::clone(&temp_file);
                let class_name = class_name.clone();
                let opt = Arc::clone(&shared_opt);
                let green_bold = Style::new().green().bold();
                let entity_map = Arc::clone(&shared_entity_map);
                let processing = Arc::clone(&processing);
                async move {
                    let mut results: Vec<Value> = Vec::new();
                    let mut offset: u32 = 0;

                    loop {
                        let query = format!(
                            r#"{{
                    "select": ["*"],
                    "from": "{}",
                    "opts": {{
                        "compact": true,
                        "limit": 5000,
                        "fuel": 9999999999,
                        "offset": {}
                    }}
                }}"#,
                            class_name, offset
                        );
                        let response_result = source_instance.issue_data_query(query).await;
                        let response = response_result.unwrap().text().await.unwrap();

                        let response: Value = match serde_json::from_str(&response) {
                            Ok(response) => response,
                            Err(e) => {
                                pretty_print(&format!("[ERROR] {}", e), Color::DarkRed, true);
                                pretty_print(
                                    &format!("Fluree Response: {}", response),
                                    Color::DarkRed,
                                    true,
                                );
                                serde_json::json!([])
                            }
                        };
                        let response = response.as_array().unwrap();

                        let mut entity_map_guard = entity_map.lock().await;

                        // let class_hash_set = entity_map_guard
                        //     .entry(class_name.clone())
                        //     .or_insert_with(HashSet::new);
                        let response_entity_ids = response
                            .iter()
                            .map(|value| value["_id"].as_i64().unwrap())
                            .collect::<HashSet<i64>>();

                        let all_entities_already_exist =
                            if let Some(class_hash_set) = entity_map_guard.get_mut(&class_name) {
                                let result = class_hash_set.is_superset(&response_entity_ids);
                                class_hash_set.extend(response_entity_ids);
                                result
                            } else {
                                entity_map_guard.insert(class_name.clone(), response_entity_ids);
                                false
                            };

                        drop(entity_map_guard);

                        if response.len() == 0 || all_entities_already_exist {
                            temp_file
                                .lock()
                                .await
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

                        if results_length > 12_500 {
                            temp_file.lock().await.write(&class_name, &results).expect(
                                format!(
                                    "Issue writing file for {} at offset {}",
                                    class_name, offset
                                )
                                .as_str(),
                            );
                            results.clear();
                        }

                        offset += 5000;
                    }

                    let mut processing_guard = processing.lock().await;
                    opt.pb.println(format!(
                        "{:>12} {} Data",
                        green_bold.apply_to("Transforming"),
                        case_normalize(&capitalize(&class_name))
                    ));
                    opt.pb.inc(1);
                    processing_guard.retain(|x| x != &class_name);
                    let print_classes = processing_guard
                        .iter()
                        .map(|x| x.to_owned())
                        .take(4)
                        .collect::<Vec<String>>();
                    let message = print_classes.join(", ");
                    let full_message = if print_classes.len() > 3 {
                        format!("[{}...]", message)
                    } else if print_classes.len() > 0 {
                        format!("[{}]", message)
                    } else {
                        "".to_string()
                    };
                    opt.pb.set_message(full_message);
                    drop(processing_guard);
                }
            });
            drop(permit);
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let mut vec_parsed_results = Vec::new();
        let files = temp_file
            .lock()
            .await
            .get_files()
            .expect("Could not get files");

        let mut result_size: u64 = 0;
        let mut file_num: u64 = 1;

        let opt = Arc::clone(&shared_opt);
        opt.pb.reset();
        opt.pb.inc_length(files.len() as u64);
        opt.pb.enable_steady_tick(Duration::from_millis(400));
        opt.pb.set_message(format!("{:3}%", 100 * 1 / files.len()));
        opt.pb.set_style(
            ProgressStyle::with_template(
                // note that bar size is fixed unlike cargo which is dynamic
                // and also the truncation in cargo uses trailers (`...`)
                if Term::stdout().size().1 > 80 {
                    "{prefix:>12.cyan.bold} [{bar:57}]{msg}  {spinner:.white}"
                } else {
                    "{prefix:>12.cyan.bold} [{bar:57}]{msg}"
                },
            )
            .unwrap()
            .tick_strings(&["🌲🎄🌲", "🎄🌲🎄", "🎄🎄🎄"])
            .progress_chars("=> "),
        );
        opt.pb.set_prefix("Writing v3 Data");

        for (index, file) in files.iter().enumerate() {
            opt.pb.inc(1);
            opt.pb
                .set_message(format!("{:3}%", 100 * (index + 1) / files.len()));
            result_size += file.metadata().expect("Could not get metadata").len();

            let file_bytes = std::fs::read(&file).expect("Could not read file");
            let file_string = String::from_utf8(file_bytes).expect("Could not convert to string");
            let results: Vec<Value> =
                serde_json::from_str(&file_string).expect("Could not parse JSON");
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
                        let shacl_shape = parser.shacl_shapes.get(&class_name).unwrap();
                        let shacl_properties = &shacl_shape.property;
                        let is_datetime = match shacl_properties.iter().find(|&x| {
                            let shacl_path = x.path.get("@id").unwrap();
                            let y = "xsd:dateTime";
                            if x.datatype.is_none() {
                                return false;
                            }
                            shacl_path == &key
                                && x.datatype.clone().unwrap().get("@id").unwrap() == y
                        }) {
                            Some(_) => true,
                            None => false,
                        };
                        let value = match is_datetime {
                            true => json!(instant_to_iso_string(value.as_i64().unwrap())),
                            false => value.to_owned(),
                        };
                        let ref_type = match shacl_properties.iter().find(|&x| {
                            let shacl_path = x.path.get("@id").unwrap();
                            let shacl_class = x.class.is_some();
                            (shacl_path == &key) && shacl_class
                        }) {
                            Some(x) => {
                                Some(x.class.clone().unwrap().get("@id").unwrap().to_string())
                            }
                            None => None,
                        };
                        parsed_result.insert(key, represent_fluree_value(&value, ref_type));
                    }
                }
                vec_parsed_results.push(json!(parsed_result));
            }

            data_results_map
                .entry("insert".to_string())
                .and_modify(|e| {
                    if let Value::Array(array) = e {
                        array.extend(vec_parsed_results.clone());
                    }
                });

            vec_parsed_results.clear();

            if result_size > 2_500_000 {
                target_instance = shared_opt
                    .write_or_print(
                        format!("{}_data.jsonld", file_num),
                        serde_json::to_string_pretty(&data_results_map).unwrap(),
                        target_instance,
                    )
                    .await;

                result_size = 0;
                file_num += 1;
                vec_parsed_results.clear();
                data_results_map
                    .entry("insert".to_string())
                    .and_modify(|e| {
                        *e = serde_json::json!([]);
                    });
            }

            std::fs::remove_file(file).expect("Could not remove file");
        }
        std::fs::remove_dir_all(temp_dir).expect("Could not remove temp directory");

        let _ = shared_opt
            .write_or_print(
                format!("{}_data.jsonld", file_num),
                serde_json::to_string_pretty(&data_results_map).unwrap(),
                target_instance,
            )
            .await;

        shared_opt.pb.finish_and_clear();

        // let finish_line = match opt.print {
        //     false => format!("to {}/ ", opt.output.to_str().unwrap()),
        //     true => "".to_string(),
        // };

        let finish_line = match (output, target) {
            (_, Some(target)) => format!("to Target Ledger [{}] ", target),
            (output, _) => match output {
                Some(output) => format!("to {}/ ", output.to_str().unwrap()),
                None => "".to_string(),
            },
        };
        println!(
            "{:>12} v3 Migration {}in {}",
            green_bold.apply_to("Finished"),
            finish_line,
            HumanDuration(start.elapsed()),
        );
    }
}
