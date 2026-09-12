use lazydb::{
    persistence::profiles::ProfileStore, profile::import_connection_url,
    profile_compatibility::ProfileUnavailableReason,
};
use tempfile::TempDir;

#[test]
fn load_report_keeps_supported_profiles_and_classifies_unknown_kind() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("connections.toml");
    let profile = import_connection_url("sqlite::memory:", Some("supported"))
        .unwrap()
        .profile;
    ProfileStore::new(path.clone()).save(vec![profile]).unwrap();
    let contents = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        format!(
            "{contents}\n[[profiles]]\nid = \"00000000-0000-0000-0000-000000000099\"\nname = \"future\"\nkind = \"future-db\"\nfuture_option = \"keep me\"\n"
        ),
    )
    .unwrap();

    let report = ProfileStore::new(path).load_report().unwrap();

    assert_eq!(report.collection.profiles.len(), 1);
    assert_eq!(report.unavailable.len(), 1);
    assert_eq!(report.unavailable[0].name, "future");
    assert_eq!(
        report.unavailable[0].reason,
        ProfileUnavailableReason::UnsupportedKind
    );
}

#[test]
fn saving_supported_profiles_does_not_drop_unknown_profile_tables() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("connections.toml");
    let profile = import_connection_url("sqlite::memory:", Some("supported"))
        .unwrap()
        .profile;
    ProfileStore::new(path.clone())
        .save(vec![profile.clone()])
        .unwrap();
    let contents = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        format!(
            "{contents}\n[[profiles]]\nid = \"00000000-0000-0000-0000-000000000099\"\nname = \"future\"\nkind = \"future-db\"\nfuture_option = \"keep me\"\n"
        ),
    )
    .unwrap();

    ProfileStore::new(path.clone()).save(vec![profile]).unwrap();
    let saved = std::fs::read_to_string(path).unwrap();

    assert!(saved.contains("name = \"future\""));
    assert!(saved.contains("kind = \"future-db\""));
    assert!(saved.contains("future_option = \"keep me\""));
}

#[test]
fn saving_supported_profiles_preserves_known_kind_with_future_fields() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("connections.toml");
    let profile = import_connection_url("sqlite::memory:", Some("supported"))
        .unwrap()
        .profile;
    ProfileStore::new(path.clone())
        .save(vec![profile.clone()])
        .unwrap();
    let contents = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        contents.replace(
            "kind = \"sqlite\"",
            "kind = \"sqlite\"\nfuture_option = \"keep me\"",
        ),
    )
    .unwrap();

    ProfileStore::new(path.clone()).save(vec![profile]).unwrap();

    assert!(
        std::fs::read_to_string(path)
            .unwrap()
            .contains("future_option = \"keep me\"")
    );
}
