pub mod opt {
    use crossterm::{
        execute,
        style::{Print, ResetColor},
    };
    use dialoguer::{console::Style, theme::ColorfulTheme, Input};
    use indicatif::ProgressBar;
    use serde_json::Value;
    use std::{
        fs::File,
        io::{self, stdout, Write},
        path::PathBuf,
    };
    use structopt::StructOpt;

    use crate::fluree::FlureeInstance;

    #[derive(Debug, StructOpt)]
    #[structopt(
        name = "jsonld",
        about = "Converts Fluree v2 schema JSON to Fluree v3 JSON-LD"
    )]
    pub struct Opt {
        /// Accessible URL for v2 Fluree DB. This will be used to fetch the schema and data state
        #[structopt(short, long, conflicts_with = "input")]
        pub source: Option<String>,

        /// Authorization token for Nexus ledgers
        /// e.g. 796b******854d
        #[structopt(long, conflicts_with = "input", requires = "source")]
        pub source_auth: Option<String>,

        /// Output file path
        #[structopt(short, long, parse(from_os_str), default_value = "output")]
        pub output: PathBuf,

        /// The v3 instance to transact resulting JSON-LD data into
        /// e.g. http://localhost:58090
        #[structopt(
            short,
            long = "target",
            conflicts_with = "output",
            conflicts_with = "print"
        )]
        pub target: Option<String>,

        /// Authorization token for the target v3 instance (if hosted on Nexus)
        #[structopt(long, requires = "target")]
        pub target_auth: Option<String>,

        /// If set, then the output will be printed to stdout instead of written to local files (Conflicts with --output)
        #[structopt(long, conflicts_with = "output", conflicts_with = "target")]
        pub print: bool,

        /// @base value for @context
        /// e.g. http://flur.ee/ids/
        /// This will be used as a default IRI prefix for all data entities
        #[structopt(short, long)]
        pub base: Option<String>,

        /// @vocab value for @context
        /// e.g. http://flur.ee/terms/
        /// This will be used as a default IRI prefix for all vocab entities
        #[structopt(short, long)]
        pub vocab: Option<String>,

        /// If set, then the result vocab JSON-LD will include SHACL shapes for each class
        #[structopt(long)]
        pub shacl: bool,

        /// This depends on the --shacl flag being used
        /// If set, then the resulting SHACL shapes will be "closed" (i.e. no additional properties can be added to instances of the class)
        #[structopt(long = "closed-shapes", requires = "shacl")]
        pub closed_shapes: bool,

        /// This depends on the --target flag being used.
        /// If set, then the first transaction issued against the target will attempt to create the ledger
        #[structopt(long = "create-ledger", requires = "target")]
        pub is_create_ledger: bool,
        // TODO: Implement this correctly
        #[structopt(skip = ProgressBar::new(2))]
        pub pb: ProgressBar,
    }

    impl Opt {
        pub fn check_url(&self, is_source: bool) -> String {
            let url = if is_source {
                self.source.clone()
            } else {
                self.target.clone()
            };
            match url {
                Some(url) => url.to_owned(),
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
            }
        }

        pub async fn write_or_print<P>(
            &self,
            file_name: P,
            data: String,
            target_instance: Option<FlureeInstance>,
        ) -> Option<FlureeInstance>
        where
            P: AsRef<std::path::Path>,
        {
            if self.print {
                let mut stdout = stdout();
                execute!(stdout, Print(data), ResetColor).unwrap();
                None
            } else if self.target.is_some() {
                let mut target_instance = match target_instance {
                    None => FlureeInstance::new_target(&self),
                    Some(fi) => fi,
                };

                let response_string: Option<Value> = None;

                let green_bold = Style::new().green().bold();
                let red_bold = Style::new().red().bold();

                while !target_instance.is_available
                    || !target_instance.is_authorized
                    || response_string.is_none()
                {
                    if !target_instance.is_available {
                        target_instance.prompt_fix_url();
                    }

                    if !target_instance.is_authorized {
                        target_instance.prompt_api_key();
                    }
                    if self.pb.is_finished() {
                        self.pb.reset();
                    }

                    let is_vocab_file = file_name.as_ref().to_str().unwrap().contains("vocab");

                    if is_vocab_file {
                        self.pb.println(format!(
                            "{:>12} Vocab Data to v3 Ledger",
                            green_bold.apply_to("Transacting")
                        ));
                    };

                    // let response_result = target_instance.issue_initial_query().await;
                    let response_result = target_instance.v3_transact(data.clone()).await;

                    let validate_attempt = target_instance.validate_result(&response_result);

                    if let Err(e) = validate_attempt {
                        self.pb
                            .println(format!("{:>12} {}", red_bold.apply_to("ERROR"), e));
                    }

                    // let awaited_response = response_result.unwrap().text().await.unwrap();
                    let awaited_response = match response_result {
                        Ok(response) => response.text().await.unwrap(),
                        Err(_) => {
                            self.pb.finish_and_clear();
                            continue;
                        }
                    };

                    if target_instance.is_available && target_instance.is_authorized {
                        // let awaited_response = response_result.unwrap().text().await.unwrap();
                        // response_string = serde_json::from_str(&awaited_response).unwrap();
                        // println!("Response: {:?}", response_string);
                        break;
                    } else {
                        let error = serde_json::from_str::<Value>(&awaited_response);
                        if let Ok(error) = error {
                            if let Some(error) = error["error"].as_str() {
                                self.pb.println(format!(
                                    "{:>12} {}",
                                    red_bold.apply_to("ERROR"),
                                    error
                                ));
                            }
                        }
                        self.pb.finish_and_clear();
                        continue;
                    }
                }

                Some(target_instance)
            } else {
                let base_path = self.output.clone();
                std::fs::create_dir_all(&base_path).unwrap_or_else(|why| {
                    if why.kind() != std::io::ErrorKind::AlreadyExists {
                        panic!("Unable to create output directory: {}", why);
                    }
                });

                let mut file =
                    File::create(&base_path.join(file_name)).expect("Unable to create file");
                let mut data_writer = io::BufWriter::new(&mut file);
                data_writer
                    .write_all(data.as_bytes())
                    .expect("Unable to write data");
                None
            }
        }
    }
}

pub mod temp_files {
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};

    use serde_json::Value;

    #[derive(Debug)]
    pub struct TempFile {
        directory: PathBuf,
        current_file: Option<File>,
        current_file_size: u64,
        file_counter: u32,
    }

    impl TempFile {
        pub fn new(directory: &Path) -> io::Result<Self> {
            if directory.exists() {
                fs::remove_dir_all(directory)?;
            }
            fs::create_dir_all(directory)?;
            Ok(TempFile {
                directory: directory.to_path_buf(),
                current_file: None,
                current_file_size: 0,
                file_counter: 0,
            })
        }

        pub fn write(&mut self, collection_name: &str, data: &Vec<Value>) -> io::Result<()> {
            let pretty_string = serde_json::to_string_pretty(data).unwrap();
            let bytes_data = pretty_string.as_bytes();
            self.create_new_file(collection_name)?;
            if let Some(file) = &mut self.current_file {
                file.write_all(bytes_data)?;
                self.current_file_size += data.len() as u64;
            }
            Ok(())
        }

        fn create_new_file(&mut self, collection_name: &str) -> io::Result<()> {
            let file_name = format!("{}__{}", self.file_counter, collection_name);
            let file_path = self.directory.join(&file_name);
            self.file_counter += 1;
            self.current_file_size = 0;
            self.current_file = Some(
                OpenOptions::new()
                    .write(true)
                    .create(true)
                    .open(file_path)?,
            );
            Ok(())
        }

        pub fn get_files(&self) -> io::Result<Vec<PathBuf>> {
            let mut files: Vec<PathBuf> = fs::read_dir(&self.directory)?
                .filter_map(|entry| {
                    if let Ok(entry) = entry {
                        let path = entry.path();
                        if path.is_file() {
                            Some(path)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();

            files.sort();

            Ok(files.to_owned())
        }
    }
}

pub mod parser {
    use std::collections::HashMap;

    use serde_json::{Map, Value};

    use crate::{
        fluree::FlureeInstance,
        functions::{create_data_context, create_vocab_context, standardize_class_name},
    };

    use self::jsonld::{Class, Property, ShaclShape};

    use super::opt::Opt;

    pub struct Parser {
        pub classes: HashMap<String, Class>,
        pub properties: HashMap<String, Property>,
        pub shacl_shapes: HashMap<String, ShaclShape>,
        pub vocab_context: HashMap<String, String>,
        pub data_context: HashMap<String, String>,
        pub network_name: String,
        pub db_name: String,
    }

    impl Parser {
        pub fn new(opt: &Opt, source_instance: &FlureeInstance) -> Self {
            Parser {
                classes: HashMap::new(),
                properties: HashMap::new(),
                shacl_shapes: HashMap::new(),
                vocab_context: create_vocab_context(opt, source_instance),
                data_context: create_data_context(opt, source_instance),
                network_name: source_instance.network_name.to_owned(),
                db_name: source_instance.db_name.to_owned(),
            }
        }

        pub fn get_vocab_json(&self, opt: &Opt) -> Map<String, Value> {
            let classes = self
                .classes
                .values()
                .map(|class| serde_json::to_value(class).unwrap());

            let properties: Vec<Value> = self
                .properties
                .values()
                .map(|property| serde_json::to_value(property).unwrap())
                .collect();

            let class_shacl_shapes: Vec<Value> = self
                .shacl_shapes
                .values()
                .map(|shape| serde_json::to_value(shape).unwrap())
                .collect();

            let results = match opt.shacl {
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

            let mut vocab_results_map = serde_json::Map::new();

            vocab_results_map.insert(
                "ledger".to_string(),
                serde_json::json!(format!("{}/{}", self.network_name, self.db_name)),
            );

            vocab_results_map.insert(
                "@context".to_string(),
                Value::Object(
                    self.vocab_context
                        .iter()
                        .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                        .collect(),
                ),
            );

            vocab_results_map.insert("insert".to_string(), Value::Array(results));

            vocab_results_map
        }

        pub fn get_or_create_class(&self, orig_class_name: &str) -> Class {
            let class_name = &standardize_class_name(orig_class_name);
            let class_object = self.classes.get(orig_class_name);
            let class_object = match class_object {
                Some(class_object) => class_object.to_owned(),
                None => Class::new(class_name),
            };
            class_object
        }

        pub fn get_or_create_property(&self, property_name: &str, type_value: &str) -> Property {
            let property_object = self.properties.get(property_name);
            let property_object = match property_object {
                Some(property_object) => property_object.update_types_and_own(type_value),
                None => Property::new(property_name, type_value),
            };
            property_object
        }

        pub fn get_or_create_shacl_shape(
            &self,
            class_name: &str,
            closed_shapes: bool,
        ) -> ShaclShape {
            let shacl_shape = self.shacl_shapes.get(class_name);
            let shacl_shape = match shacl_shape {
                Some(shacl_shape) => shacl_shape.to_owned(),
                None => ShaclShape::new(class_name, closed_shapes),
            };
            shacl_shape
        }

        // TODO: if another shacl_shape in parser.shacl_shapes has the same property name, and if it has a different datatype, then I need to log a warning and I need to update the property name to be the Class/Property (e.g. Person/age and Animal/age)
    }

    pub mod jsonld {
        use std::collections::{HashMap, HashSet};

        use serde::{Deserialize, Serialize};
        use serde_json::Value;

        use crate::functions::{
            remove_namespace, standardize_class_name, standardize_property_name,
        };

        #[derive(Debug, Clone, Deserialize, Serialize)]
        pub struct Class {
            #[serde(rename = "@id")]
            pub id: String,
            #[serde(rename = "@type")]
            pub type_: String,
            #[serde(rename = "rdfs:label")]
            pub label: String,
            #[serde(rename = "rdfs:comment", skip_serializing_if = "Option::is_none")]
            pub comment: Option<String>,
            #[serde(rename = "rdfs:subClassOf", skip_serializing_if = "Option::is_none")]
            pub sub_class_of: Option<Vec<HashMap<String, String>>>,
            #[serde(rename = "rdfs:range")]
            pub range: Vec<HashMap<String, String>>,
        }

        impl Class {
            pub fn new(class_name: &str) -> Self {
                Class {
                    id: class_name.to_string(),
                    type_: "rdfs:Class".to_string(),
                    label: remove_namespace(class_name).to_string(),
                    comment: None,
                    sub_class_of: None,
                    range: Vec::new(),
                }
            }

            pub fn set_property_range(&mut self, property_name: &str) {
                self.range.push(HashMap::from([(
                    "@id".to_string(),
                    property_name.to_string(),
                )]));
            }
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct Property {
            #[serde(rename = "@id")]
            pub id: String,
            #[serde(rename = "@type")]
            pub type_: String,
            #[serde(rename = "rdfs:label")]
            pub label: String,
            #[serde(rename = "rdfs:comment")]
            pub comment: String,
            #[serde(rename = "rdfs:domain")]
            pub domain: Vec<HashMap<String, String>>,
            #[serde(skip_serializing)]
            pub data_types: HashSet<String>,
        }

        impl Property {
            pub fn new(property_name: &str, type_value: &str) -> Self {
                let standard_property_name = standardize_property_name(property_name);
                let data_type = Self::normalize_type_value(type_value);
                let data_types: HashSet<String> = match data_type {
                    Some(data_type) => vec![data_type].into_iter().collect(),
                    None => HashSet::new(),
                };
                Property {
                    id: standard_property_name.clone(),
                    type_: "rdf:Property".to_string(),
                    label: remove_namespace(&standard_property_name),
                    comment: String::new(),
                    domain: Vec::new(),
                    data_types,
                }
            }

            pub fn normalize_type_value(type_value: &str) -> Option<String> {
                match type_value {
                    "float" | "int" | "instant" | "boolean" | "long" | "string" => {
                        let data_type = match type_value {
                            "int" => "xsd:integer".to_string(),
                            "instant" => "xsd:dateTime".to_string(),
                            // "ref" => "xsd:anyURI".to_string(),
                            _ => format!("xsd:{}", type_value),
                        };
                        Some(data_type)
                    }
                    "tag" => {
                        // TODO: Figure out how to handle tag types
                        None
                    }
                    _ => None,
                }
            }

            pub fn update_types_and_own(&self, type_value: &str) -> Self {
                let mut property = self.to_owned();
                let data_type = Self::normalize_type_value(type_value);
                match data_type {
                    Some(data_type) => {
                        property.data_types.insert(data_type);
                    }
                    None => {}
                }
                property
            }

            pub fn set_class_domain(&mut self, class_name: &str) {
                self.domain
                    .push(HashMap::from([("@id".to_string(), class_name.to_string())]));
            }
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct ShaclShape {
            #[serde(rename = "@id", skip_serializing_if = "String::is_empty")]
            pub id: String,
            #[serde(rename = "@type")]
            pub type_: String,
            #[serde(rename = "sh:targetClass")]
            pub target_class: HashMap<String, String>,
            #[serde(rename = "sh:targetProperty", skip_serializing_if = "String::is_empty")]
            pub target_property: String,
            #[serde(rename = "sh:property")]
            pub property: Vec<ShaclProperty>,
            #[serde(rename = "sh:datatype", skip_serializing_if = "String::is_empty")]
            pub datatype: String,
            #[serde(rename = "sh:nodeKind", skip_serializing_if = "String::is_empty")]
            pub node_kind: String,
            #[serde(rename = "sh:closed", skip_serializing_if = "Option::is_none")]
            pub closed: Option<bool>,
            #[serde(rename = "sh:ignoredProperties", skip_serializing_if = "Vec::is_empty")]
            pub ignored_properties: Vec<HashMap<String, String>>,
        }

        impl ShaclShape {
            pub fn new(class_name: &str, closed: bool) -> Self {
                let closed = match closed {
                    true => Some(true),
                    false => None,
                };
                let ignored_properties = match closed {
                    Some(true) => {
                        let mut map = HashMap::new();
                        map.insert("@id".to_string(), "@type".to_string());
                        vec![map]
                    }
                    _ => Vec::new(),
                };
                ShaclShape {
                    id: String::new(),
                    type_: "sh:NodeShape".to_string(),
                    target_class: {
                        let mut map = HashMap::new();
                        map.insert("@id".to_string(), class_name.to_string());
                        map
                    },
                    target_property: String::new(),
                    property: Vec::new(),
                    datatype: String::new(),
                    node_kind: String::new(),
                    closed,
                    ignored_properties,
                }
            }

            pub fn set_property(
                &mut self,
                property_object: &mut Property,
                item: &Value,
            ) -> Result<(), Vec<String>> {
                let mut result = Ok(());
                let mut shacl_property = ShaclProperty::new(&property_object.id);

                if item["multi"].is_null() || !item["multi"].as_bool().unwrap() {
                    shacl_property.max_count = Some(1);
                }

                let keys = item.as_object().unwrap().keys();

                for key in keys {
                    match key.as_str() {
                        "doc" => {
                            property_object.comment = item["doc"].as_str().unwrap().to_string();
                        }
                        "type" => {
                            let property_types = &property_object.data_types;
                            if property_types.len() > 1 {
                                let p = &property_object.id;
                                let c = self.target_class.get("@id").unwrap();
                                let data_type =
                                    Property::normalize_type_value(item["type"].as_str().unwrap())
                                        .unwrap();
                                let other_data_types = property_types
                                    .iter()
                                    .filter(|s| s != &&data_type)
                                    .collect::<Vec<_>>();

                                // pretty_print(
                                //     &format!("[WARN] Inconsistent Datatype Usage: Property, \"{p}\", in class, \"{c}\", is defined with datatype, \"{data_type}\", but also used with different datatypes [{other_data_types}]. Proceeding with SHACL NodeShape but skipping \"sh:datatype\" for \"{p}\".", other_data_types = other_data_types.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")),
                                //     crossterm::style::Color::DarkYellow,
                                //     true
                                // );
                                let error_vec = vec![
                                    format!("Property, \"{p}\", in class, \"{c}\", is defined with datatype, \"{data_type}\", but also used with different datatypes [{other_data_types}].", other_data_types = other_data_types.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")),
                                    format!("Proceeding with SHACL NodeShape but skipping \"sh:datatype\" for \"{p}\"."),
                                ];
                                result = Err(error_vec);
                            } else {
                                match property_types.iter().next() {
                                    Some(data_type) => {
                                        shacl_property.datatype = Some(HashMap::from([(
                                            "@id".to_string(),
                                            data_type.to_string(),
                                        )]));
                                    }
                                    None => {}
                                }
                            }
                        }
                        "restrictCollection" => {
                            shacl_property.class = Some(HashMap::from([(
                                "@id".to_string(),
                                standardize_class_name(
                                    item["restrictCollection"].as_str().unwrap(),
                                ),
                            )]));
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
                self.property.push(shacl_property);
                result
            }
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct ShaclProperty {
            #[serde(rename = "@id", skip_serializing_if = "String::is_empty")]
            pub id: String,
            #[serde(rename = "@type", skip_serializing_if = "String::is_empty")]
            pub type_: String,
            #[serde(rename = "rdfs:label", skip_serializing_if = "String::is_empty")]
            pub label: String,
            #[serde(rename = "rdfs:comment", skip_serializing_if = "String::is_empty")]
            pub comment: String,
            #[serde(rename = "sh:path", skip_serializing_if = "HashMap::is_empty")]
            pub path: HashMap<String, String>,
            #[serde(rename = "sh:class", skip_serializing_if = "Option::is_none")]
            pub class: Option<HashMap<String, String>>,
            #[serde(rename = "sh:minCount", skip_serializing_if = "Option::is_none")]
            pub min_count: Option<u32>,
            #[serde(rename = "sh:maxCount", skip_serializing_if = "Option::is_none")]
            pub max_count: Option<u32>,
            #[serde(rename = "sh:datatype", skip_serializing_if = "Option::is_none")]
            pub datatype: Option<HashMap<String, String>>,
            #[serde(rename = "sh:nodeKind", skip_serializing_if = "String::is_empty")]
            pub node_kind: String,
            #[serde(rename = "sh:pattern", skip_serializing_if = "String::is_empty")]
            pub pattern: String,
        }

        impl ShaclProperty {
            pub fn new(property_name: &str) -> Self {
                ShaclProperty {
                    id: String::new(),
                    type_: String::new(),
                    label: String::new(),
                    comment: String::new(),
                    path: HashMap::from([("@id".to_string(), property_name.to_string())]),
                    class: None,
                    min_count: None,
                    max_count: None,
                    datatype: None,
                    node_kind: String::new(),
                    pattern: String::new(),
                }
            }
        }
    }
}
