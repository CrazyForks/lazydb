use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::update::{InstallationManager, InstallationState, parse_installation_state};

#[derive(Clone, Debug)]
pub struct NativeInstallation {
    pub root: PathBuf,
    pub state_path: PathBuf,
    pub state: InstallationState,
}

pub fn discover_native(root: &Path) -> anyhow::Result<Option<NativeInstallation>> {
    let state_path = root.join("install.json");
    let input = match fs::read_to_string(&state_path) {
        Ok(input) => input,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let state = parse_installation_state(&input)?;
    if state.manager != InstallationManager::Native {
        return Ok(None);
    }
    let launcher = &state.path;
    let link = fs::read_link(launcher)?;
    let target = if link.is_absolute() {
        link
    } else {
        launcher
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(link)
    };
    let current = target
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "current"))
        .ok_or_else(|| anyhow::anyhow!("native launcher does not point through current"))?;
    let actual_root = current
        .parent()
        .ok_or_else(|| anyhow::anyhow!("native installation root is unavailable"))?;
    if !actual_root.join("releases").is_dir()
        || fs::canonicalize(actual_root)? != fs::canonicalize(root)?
    {
        return Err(anyhow::anyhow!(
            "native installation root is not the recorded directory: {}",
            actual_root.display()
        ));
    }
    Ok(Some(NativeInstallation {
        root: actual_root.to_owned(),
        state_path,
        state,
    }))
}
