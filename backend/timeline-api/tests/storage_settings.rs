//! `StorageSettings`: the two storage names the upload-processing Lambda
//! reads, and the same names as `AwsSettings` reads them. See the migration
//! plan's §V2e, E2 (and C31: the processing Lambda must not need Cognito
//! settings it never uses).

use std::collections::HashMap;

use timeline_api::aws_settings::{AwsSettings, MissingSettings, StorageSettings};

const STORAGE_VARS: [&str; 2] = ["TIMELINE_UPLOADS_BUCKET", "TIMELINE_CONVERSATIONS_TABLE"];

fn storage_env() -> HashMap<&'static str, String> {
    HashMap::from([
        ("TIMELINE_UPLOADS_BUCKET", "uploads".to_string()),
        ("TIMELINE_CONVERSATIONS_TABLE", "conversations".to_string()),
    ])
}

fn read(env: &HashMap<&'static str, String>) -> Result<StorageSettings, MissingSettings> {
    StorageSettings::from_lookup(|name| env.get(name).cloned())
}

#[test]
fn the_two_storage_names_are_read_without_any_cognito_settings() {
    let s = read(&storage_env()).unwrap();
    assert_eq!(s.uploads_bucket.as_str(), "uploads");
    assert_eq!(s.conversations_table.as_str(), "conversations");
}

#[test]
fn each_missing_storage_name_is_named() {
    for name in STORAGE_VARS {
        let mut env = storage_env();
        env.remove(name);
        assert_eq!(read(&env), Err(MissingSettings(vec![name])), "{name}");
    }
}

#[test]
fn an_empty_storage_name_counts_as_missing_and_all_are_reported_at_once() {
    let mut env = storage_env();
    env.insert("TIMELINE_UPLOADS_BUCKET", " ".to_string());
    assert_eq!(
        read(&env),
        Err(MissingSettings(vec!["TIMELINE_UPLOADS_BUCKET"]))
    );
    assert_eq!(
        read(&HashMap::new()),
        Err(MissingSettings(STORAGE_VARS.to_vec()))
    );
}

#[test]
fn the_api_settings_storage_half_matches_what_the_processing_lambda_reads() {
    let mut env = storage_env();
    env.insert(
        "TIMELINE_COGNITO_USER_POOL_ID",
        "us-east-1_Pool".to_string(),
    );
    env.insert("TIMELINE_COGNITO_CLIENT_ID", "client".to_string());
    env.insert("AWS_REGION", "us-east-1".to_string());
    let api = AwsSettings::from_lookup(|name| env.get(name).cloned()).unwrap();
    assert_eq!(api.storage(), read(&env).unwrap());
}
