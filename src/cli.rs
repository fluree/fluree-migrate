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
