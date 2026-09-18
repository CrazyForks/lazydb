use lazydb::{
    config::{AppConfig, HelpPanelView},
    persistence::settings::SettingsStore,
};
use tempfile::tempdir;

#[test]
fn settings_store_preserves_existing_toml_and_round_trips_view() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("nested/settings.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "# keep this comment\n[ui]\nicons = \"ascii\"\nmotion = \"none\"\n\n[terminal]\nmouse = \"auto\"\ncolor = \"auto\"\n",
    )
    .unwrap();

    SettingsStore::new(path.clone())
        .save_help_panel(HelpPanelView::Omni)
        .unwrap();

    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("# keep this comment"));
    assert!(contents.contains("icons = \"ascii\""));
    assert!(contents.contains("help_panel = \"omni\""));
}

#[test]
fn settings_store_creates_missing_file_and_app_config_loads_it() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("new/settings.toml");

    SettingsStore::new(path.clone())
        .save_help_panel(HelpPanelView::Omni)
        .unwrap();

    let config = AppConfig::load(path).unwrap();
    assert_eq!(config.ui.help_panel, HelpPanelView::Omni);
}

#[test]
fn settings_store_rejects_invalid_existing_document_without_replacing_it() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("settings.toml");
    std::fs::write(&path, "[ui\ninvalid").unwrap();

    assert!(
        SettingsStore::new(path.clone())
            .save_help_panel(HelpPanelView::Help)
            .is_err()
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "[ui\ninvalid");
}
