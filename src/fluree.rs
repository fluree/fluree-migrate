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
