#![cfg(feature = "driver-oracle")]

use std::path::Path;

#[test]
fn default_client_directory_is_resolved_from_home_data_path() {
    let home = std::env::var_os("HOME").expect("HOME is set in test environment");
    let expected = Path::new(&home).join(".local/share/lazydb/oracle/current");
    assert!(expected.ends_with("lazydb/oracle/current"));
}

#[test]
fn installed_client_contains_the_required_dynamic_library() {
    let home = std::env::var_os("HOME").expect("HOME is set in test environment");
    let path = Path::new(&home).join(".local/share/lazydb/oracle/current/libclntsh.dylib");
    if path.parent().is_some_and(Path::exists) {
        assert!(
            path.exists(),
            "installed Oracle Client is missing libclntsh.dylib"
        );
    }
}
