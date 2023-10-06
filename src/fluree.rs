use std::io::stdout;

use crossterm::execute;
use crossterm::style::{Print, ResetColor, SetForegroundColor};
use dialoguer::{theme::ColorfulTheme, Input};
use reqwest::{header::HeaderMap, Client, Error, Response};

use crate::cli::opt::Opt;
use crate::console::{pretty_print, ERROR_COLOR};

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

#[derive(Debug)]
pub struct FlureeInstance {
    pub url: String,
    pub network_name: String,
    pub db_name: String,
    pub is_available: bool,
    pub is_authorized: bool,
    pub api_key: Option<String>,
    pub client: Client,
}

impl FlureeInstance {
    pub fn new(opt: &Opt) -> Self {
        let url = opt.check_url();
        let (network_name, db_name) = Self::get_db_name(&url);
        FlureeInstance {
            url: url.to_string(),
            network_name,
            db_name,
            is_available: true,
            is_authorized: true,
            api_key: opt.authorization.clone(),
            client: reqwest::Client::new(),
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

    pub fn validate_result(&mut self, result: &Result<Response, Error>) {
        (self.is_available, self.is_authorized) = match result {
            Ok(response) => match response.status() {
                reqwest::StatusCode::OK => (true, true),
                reqwest::StatusCode::UNAUTHORIZED => {
                    match self.api_key {
                        Some(_) => {
                            pretty_print(
                                    "The API Key you provided is not authorized to access this database. Please try again.",
                                    ERROR_COLOR,
                                    true,
                                );
                        }
                        None => {
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
                            self.url,
                            response.status()
                        ),
                        ERROR_COLOR,
                        true,
                    );
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
    }
}
