

pub mod console {
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

    pub const ERROR_COLOR: Color = Color::Yellow;
    pub fn pretty_print(string: &str, color: Color, newline: bool) {
    let newline = match newline {
        true => "\n",
        false => "",
    };
    execute!(
        io::stdout(),
        SetForegroundColor(color),
        Print(string),
        Print(newline),
        ResetColor
    )
    .expect("ERROR: stdout unavailable");
}

}
