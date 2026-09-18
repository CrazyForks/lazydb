use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use crate::config::HelpPanelView;

pub use crate::config::{AppConfig as AppSettings, ConfigError as SettingsError};

#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn save_help_panel(&self, view: HelpPanelView) -> Result<(), io::Error> {
        let mut document = match fs::read_to_string(&self.path) {
            Ok(contents) => contents
                .parse::<toml_edit::DocumentMut>()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => toml_edit::DocumentMut::new(),
            Err(error) => return Err(error),
        };
        let ui = document["ui"].or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
        let table = ui.as_table_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "settings ui must be a table")
        })?;
        table.insert("help_panel", toml_edit::value(view.as_str()));
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(".settings.{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            file.write_all(document.to_string().as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}
