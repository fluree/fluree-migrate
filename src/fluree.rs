pub mod query {
    pub const SCHEMA_QUERY: &str = r#"{
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
}

pub mod db {
    pub fn get_db_name(url: &str) -> (String, String) {
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
}
