#![cfg(unix)]

use std::fs;

#[test]
fn directory_resolution_fixture_documents_supported_cases() {
    let fixture = include_str!("fixtures/app-home-cases.json");
    let cases: serde_json::Value = serde_json::from_str(fixture).unwrap();
    assert_eq!(cases.as_array().unwrap().len(), 5);
    assert!(
        cases[0]["expected"]
            .as_str()
            .unwrap()
            .ends_with("home/lazydb")
    );
    assert_eq!(cases[4]["error"], "multiple");
    assert!(fs::metadata("tests/fixtures/app-home-cases.json").is_ok());
}
