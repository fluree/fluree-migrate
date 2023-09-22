pub mod opt {
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
}

pub mod temp_files {
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Read, Write};
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
