#[cfg(feature = "driver-oracle")]
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

#[cfg(feature = "driver-oracle")]
use super::{DatabaseError, ErrorCategory};

#[cfg(feature = "driver-oracle")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleClientLocation {
    pub directory: Option<PathBuf>,
    pub source: &'static str,
}

#[cfg(feature = "driver-oracle")]
static INITIALIZED: OnceLock<Mutex<Option<OracleClientLocation>>> = OnceLock::new();

#[cfg(feature = "driver-oracle")]
pub fn resolve() -> Result<OracleClientLocation, DatabaseError> {
    if let Some(value) = std::env::var_os("LAZYDB_ORACLE_CLIENT_LIB_DIR") {
        let path = PathBuf::from(value);
        validate(&path)?;
        return Ok(OracleClientLocation {
            directory: Some(path),
            source: "environment",
        });
    }
    let default = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| path.join(".local/share/lazydb/oracle/current"));
    if let Some(path) = default
        && path.exists()
    {
        validate(&path)?;
        return Ok(OracleClientLocation {
            directory: Some(path),
            source: "lazydb",
        });
    }
    Ok(OracleClientLocation {
        directory: None,
        source: "odpi-default",
    })
}

#[cfg(all(test, feature = "driver-oracle"))]
mod tests {
    use super::*;

    #[test]
    fn resolves_the_current_client_directory_when_installed() {
        let location = resolve().unwrap();
        assert!(matches!(
            location.source,
            "lazydb" | "odpi-default" | "environment"
        ));
    }
}

#[cfg(feature = "driver-oracle")]
fn validate(path: &Path) -> Result<(), DatabaseError> {
    if !path.is_dir() || !path.join("libclntsh.dylib").exists() {
        return Err(DatabaseError {
            category: ErrorCategory::Configuration,
            code: Some("oracle_client_path_invalid".into()),
            message: format!("Oracle Client directory is invalid: {}", path.display()),
            diagnostic: None,
        });
    }
    Ok(())
}

#[cfg(feature = "driver-oracle")]
pub fn initialize() -> Result<OracleClientLocation, DatabaseError> {
    let location = resolve()?;
    let state = INITIALIZED.get_or_init(|| Mutex::new(None));
    let mut initialized = state.lock().map_err(|_| DatabaseError {
        category: ErrorCategory::Internal,
        code: Some("oracle_client_state_poisoned".into()),
        message: "Oracle Client initialization state is unavailable".into(),
        diagnostic: None,
    })?;
    if let Some(previous) = initialized.as_ref() {
        if previous != &location {
            return Err(DatabaseError {
                category: ErrorCategory::Configuration,
                code: Some("oracle_client_restart_required".into()),
                message: "Oracle Client location changed; restart LazyDB to apply it".into(),
                diagnostic: None,
            });
        }
        return Ok(previous.clone());
    }
    let mut params = oracle::InitParams::new();
    if let Some(directory) = &location.directory {
        params
            .oracle_client_lib_dir(directory)
            .map_err(|error| load_error(error.to_string()))?;
    }
    params
        .init()
        .map_err(|error| load_error(error.to_string()))?;
    *initialized = Some(location.clone());
    Ok(location)
}

#[cfg(feature = "driver-oracle")]
fn load_error(message: String) -> DatabaseError {
    DatabaseError {
        category: ErrorCategory::Configuration,
        code: Some("oracle_client_load_failed".into()),
        message: super::super::security::sanitize_terminal_text(&message),
        diagnostic: None,
    }
}
