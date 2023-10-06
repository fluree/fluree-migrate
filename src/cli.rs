pub mod opt {
    use dialoguer::{theme::ColorfulTheme, Input};
    use std::path::PathBuf;
    use structopt::StructOpt;

    #[derive(Debug, StructOpt)]
    #[structopt(
        name = "jsonld",
        about = "Converts Fluree v2 schema JSON to Fluree v3 JSON-LD"
    )]
    pub struct Opt {
        /// Input URL
        #[structopt(short, long, conflicts_with = "input")]
        pub url: Option<String>,

        /// Authorization token for Nexus ledgers
        #[structopt(short, long, conflicts_with = "input", requires = "url")]
        pub authorization: Option<String>,

        /// Output file path
        #[structopt(short, long, parse(from_os_str))]
        pub output: Option<PathBuf>,

        /// @base value for @context
        #[structopt(short, long)]
        pub base: Option<String>,

        /// @context value(s)
        #[structopt(short, long)]
        pub context: Option<Vec<String>>,

        /// @context namespace to use for new classes and properties
        #[structopt(short, long, requires = "context")]
        pub namespace: Option<String>,

        // I want to add a flag --shacl that is by default false, but if it is true, then it will add the shacl shapes to the output
        #[structopt(short, long)]
        pub shacl: bool,

        #[structopt(long = "create-ledger")]
        pub is_create_ledger: bool,
    }

    impl Opt {
        pub fn check_url(&self) -> String {
            match &self.url {
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

        pub fn validate_namespace_and_context(&self) {
            if self.namespace.is_some() && self.context.is_none() {
                panic!("You must specify a context when using the --namespace (-n) flag\n\nExample: -c schema=http://schema.org/ -n schema");
            }
        }
    }
}

pub mod temp_files {
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};

    use serde_json::Value;

    pub struct TempFile {
        directory: PathBuf,
        current_file: Option<File>,
        current_file_size: u64,
        file_counter: u32,
    }

    impl TempFile {
        pub fn new(directory: &Path) -> io::Result<Self> {
            // I want to check if a dir exists and if so, I want to delete all of its contents. If it does not delete, I want to create it
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

            files.sort(); // Sort the files to read them in order

            Ok(files.to_owned())

            // for file in files {
            //     let mut file_data = Vec::new();
            //     File::open(&file)?.read_to_end(&mut file_data)?;
            //     data.push(file_data);
            // }

            // Ok(data)
        }
    }
}

pub mod parser {
    use std::collections::HashMap;

    use serde_json::{Map, Value};

    use crate::{
        fluree::FlureeInstance,
        functions::{create_context, standardize_class_name},
    };

    use self::jsonld::{Class, Property, ShaclShape};

    use super::opt::Opt;

    pub struct Parser {
        pub classes: HashMap<String, Class>,
        pub properties: HashMap<String, Property>,
        pub shacl_shapes: HashMap<String, ShaclShape>,
        pub prefix: String,
        pub context: HashMap<String, String>,
        pub network_name: String,
        pub db_name: String,
    }

    impl Parser {
        pub fn new(prefix: String, opt: &Opt, source_instance: &FlureeInstance) -> Self {
            let context = create_context(opt, source_instance);
            Parser {
                classes: HashMap::new(),
                properties: HashMap::new(),
                shacl_shapes: HashMap::new(),
                prefix,
                context,
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
                "f:ledger".to_string(),
                serde_json::json!(format!("{}/{}", self.network_name, self.db_name)),
            );

            if opt.is_create_ledger {
                vocab_results_map.insert(
                    "@context".to_string(),
                    serde_json::json!({ "f": "https://ns.flur.ee/ledger#"}),
                );
                vocab_results_map.insert(
                    "f:defaultContext".to_string(),
                    Value::Object(
                        self.context
                            .iter()
                            .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                            .collect(),
                    ),
                );
            } else {
                vocab_results_map.insert(
                    "@context".to_string(),
                    Value::Object(
                        self.context
                            .iter()
                            .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                            .collect(),
                    ),
                );
            }

            vocab_results_map.insert("@graph".to_string(), Value::Array(results));

            vocab_results_map
        }

        pub fn get_or_create_class(&self, orig_class_name: &str) -> Class {
            let class_name = &standardize_class_name(orig_class_name, &self.prefix);
            let class_object = self.classes.get(orig_class_name);
            let class_object = match class_object {
                Some(class_object) => class_object.to_owned(),
                None => Class::new(class_name),
            };
            class_object
        }

        pub fn get_or_create_property(&self, property_name: &str) -> Property {
            let property_object = self.properties.get(property_name);
            let property_object = match property_object {
                Some(property_object) => property_object.to_owned(),
                None => Property::new(property_name, &self.prefix),
            };
            property_object
        }

        pub fn get_or_create_shacl_shape(&self, class_name: &str) -> ShaclShape {
            let shacl_shape = self.shacl_shapes.get(class_name);
            let shacl_shape = match shacl_shape {
                Some(shacl_shape) => shacl_shape.to_owned(),
                None => ShaclShape::new(class_name),
            };
            shacl_shape
        }
    }

    pub mod jsonld {
        use std::collections::HashMap;

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
        }

        impl Property {
            pub fn new(property_name: &str, prefix: &str) -> Self {
                let standard_property_name = standardize_property_name(property_name, prefix);
                Property {
                    id: standard_property_name.clone(),
                    type_: "rdf:Property".to_string(),
                    label: remove_namespace(&standard_property_name),
                    comment: String::new(),
                    domain: Vec::new(),
                }
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
        }

        impl ShaclShape {
            pub fn new(class_name: &str) -> Self {
                ShaclShape {
                    id: String::new(),
                    type_: "sh:NodeShape".to_string(),
                    // new HashMap with initial values { "id": class_name }
                    target_class: {
                        let mut map = HashMap::new();
                        map.insert("@id".to_string(), class_name.to_string());
                        map
                    },
                    target_property: String::new(),
                    property: Vec::new(),
                    datatype: String::new(),
                    node_kind: String::new(),
                }
            }

            pub fn set_property(
                &mut self,
                property_object: &mut Property,
                item: &Value,
                prefix: &str,
            ) {
                let mut shacl_property = ShaclProperty::new(&property_object.id);

                if item["multi"].is_null() || !item["multi"].as_bool().unwrap() {
                    shacl_property.max_count = Some(1);
                }

                let keys = item.as_object().unwrap().keys();

                for key in keys {
                    match key.as_str() {
                        // "_id" => {}
                        "doc" => {
                            property_object.comment = item["doc"].as_str().unwrap().to_string();
                        }
                        "type" => {
                            let type_value = item["type"].as_str().unwrap();
                            match type_value {
                                "float" | "int" | "instant" | "boolean" | "long" | "string"
                                | "ref" => {
                                    let data_type = match type_value {
                                        "int" => "xsd:integer".to_string(),
                                        "instant" => "xsd:dateTime".to_string(),
                                        "ref" => "xsd:anyURI".to_string(),
                                        _ => format!("xsd:{}", type_value),
                                    };
                                    shacl_property.datatype =
                                        Some(HashMap::from([("@id".to_string(), data_type)]));
                                }
                                "tag" => {
                                    // TODO: Figure out how to handle tag types
                                }
                                _ => {}
                            }
                        }
                        "restrictCollection" => {
                            shacl_property.class = Some(HashMap::from([(
                                "@id".to_string(),
                                standardize_class_name(
                                    item["restrictCollection"].as_str().unwrap(),
                                    &prefix,
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
