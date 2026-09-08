use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use sha2::Digest;

use crate::{
    cli::UninstallArgs,
    persistence::{
        profiles::ProfileStore,
        secrets::{NativeSecretStore, SecretStore},
    },
    profile::CredentialPolicy,
    update::{InstallationManager, UpdateLock, parse_installation_state},
};

#[derive(Debug, serde::Serialize)]
pub struct UninstallReport {
    pub schema_version: u32,
    pub status: &'static str,
    pub manager: &'static str,
    pub installation_root: Option<PathBuf>,
    pub actions: Vec<UninstallAction>,
    pub preserved: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct UninstallAction {
    pub kind: &'static str,
    pub path: PathBuf,
    pub reason: &'static str,
    pub status: &'static str,
}

#[derive(Debug)]
struct Installation {
    config_dir: PathBuf,
    state: crate::update::InstallationState,
    root: PathBuf,
    launcher: PathBuf,
}

pub async fn run(args: UninstallArgs, _config: Option<PathBuf>) -> Result<String> {
    if args.json && !args.dry_run && !args.yes {
        bail!("--json uninstall requires --yes or --dry-run");
    }

    let mut report = match discover_installation() {
        Ok(installation) => plan_installation(&installation, args.purge),
        Err(warning) => UninstallReport {
            schema_version: 1,
            status: "blocked",
            manager: "unknown",
            installation_root: None,
            actions: Vec::new(),
            preserved: Vec::new(),
            warnings: vec![warning],
        },
    };
    if report.status == "planned" && !args.dry_run {
        if !args.yes {
            if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                bail!("non-interactive uninstall requires --yes or --dry-run");
            }
            print!("Proceed with uninstall? [y/N] ");
            std::io::Write::flush(&mut std::io::stdout())?;
            let mut answer = String::new();
            std::io::stdin().read_line(&mut answer)?;
            if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                report.status = "cancelled";
                return if args.json {
                    Ok(serde_json::to_string(&report)?)
                } else {
                    Ok(format_report(&report))
                };
            }
        }
        let installation = discover_installation().map_err(anyhow::Error::msg)?;
        let _lock = UpdateLock::acquire(&installation.root)?;
        let refreshed = discover_from_candidates(vec![installation.config_dir.clone()])
            .map_err(anyhow::Error::msg)?;
        report = execute_installation(&refreshed, args.purge).await?;
    }
    if args.json {
        return Ok(serde_json::to_string(&report)?);
    }
    Ok(format_report(&report))
}

fn discover_installation() -> Result<Installation, String> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set; cannot locate LazyDB installation".to_owned())?;
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("LAZYDB_CONFIG_HOME").filter(|path| !path.is_empty()) {
        candidates.push(PathBuf::from(path));
    }
    candidates.push(home.join(".config/lazydb"));
    candidates.push(home.join(".local/share/lazydb"));

    discover_from_candidates(candidates)
}

fn discover_from_candidates(candidates: Vec<PathBuf>) -> Result<Installation, String> {
    let mut errors = Vec::new();
    for config_dir in candidates {
        let state_path = config_dir.join("install.json");
        let input = match fs::read_to_string(&state_path) {
            Ok(input) => input,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                errors.push(format!("{}: {error}", state_path.display()));
                continue;
            }
        };
        let state = match parse_installation_state(&input) {
            Ok(state) => state,
            Err(error) => {
                errors.push(format!("{}: {error}", state_path.display()));
                continue;
            }
        };
        if state.manager != InstallationManager::Native {
            errors.push(format!(
                "{}: installation manager is not native",
                state_path.display()
            ));
            continue;
        }
        let launcher = state.path.clone();
        let target = fs::read_link(&launcher).map_err(|error| {
            format!(
                "native launcher {} is not a readable symlink: {error}",
                launcher.display()
            )
        })?;
        let target = if target.is_absolute() {
            target
        } else {
            launcher
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(target)
        };
        let current = target
            .parent()
            .filter(|path| path.file_name().is_some_and(|name| name == "current"))
            .ok_or_else(|| {
                format!(
                    "native launcher does not point through current: {}",
                    target.display()
                )
            })?;
        let root = current
            .parent()
            .ok_or_else(|| "native installation root is unavailable".to_owned())?
            .to_path_buf();
        if root != config_dir || !root.join("releases").is_dir() {
            return Err(format!(
                "native installation root is not the recorded config directory: {}",
                root.display()
            ));
        }
        return Ok(Installation {
            config_dir,
            state,
            root,
            launcher,
        });
    }
    Err(if errors.is_empty() {
        "no native LazyDB installation was found".to_owned()
    } else {
        errors.join("; ")
    })
}

fn plan_installation(installation: &Installation, purge: bool) -> UninstallReport {
    let mut actions = vec![
        action(
            "launcher",
            installation.launcher.clone(),
            "native LazyDB launcher",
        ),
        action(
            "current",
            installation.root.join("current"),
            "native current release link",
        ),
        action(
            "state",
            installation.root.join("install.json"),
            "native installation metadata",
        ),
    ];
    if let Ok(entries) = fs::read_dir(installation.root.join("releases")) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                actions.push(action("release", entry.path(), "native release directory"));
            }
        }
    }
    let mut preserved = vec![
        installation.config_dir.join("connections.toml"),
        installation.config_dir.join("credential.key"),
        installation.config_dir.join("settings.toml"),
        installation.config_dir.join("workspace.toml"),
        installation.config_dir.join("sql"),
        installation.config_dir.join("update-check.json"),
    ];
    if purge {
        actions.extend(
            preserved
                .drain(..)
                .filter(|path| {
                    path.file_name().is_some_and(|name| {
                        name == "settings.toml"
                            || name == "workspace.toml"
                            || name == "update-check.json"
                            || name == "connections.toml"
                            || name == "credential.key"
                    })
                })
                .map(|path| action("data", path, "explicit --purge")),
        );
    }
    UninstallReport {
        schema_version: 1,
        status: "planned",
        manager: "native",
        installation_root: Some(installation.root.clone()),
        actions,
        preserved,
        warnings: vec![
            "MCP configuration is not changed by uninstall; remove project entries separately"
                .to_owned(),
            "review the action list before confirming; unknown files are preserved".to_owned(),
        ],
    }
}

async fn execute_installation(installation: &Installation, purge: bool) -> Result<UninstallReport> {
    let planned = plan_installation(installation, purge);
    let mut actions = Vec::with_capacity(planned.actions.len());
    let mut warnings = planned.warnings;
    let shell_profile = installation_shell_profile(installation);
    if purge {
        cleanup_system_credentials(installation).await?;
    }
    for item in planned.actions {
        let result = if item.kind == "release" {
            fs::remove_dir_all(&item.path)
        } else {
            remove_entry(&item.path)
        };
        match result {
            Ok(()) => actions.push(UninstallAction {
                status: "completed",
                ..item
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                actions.push(UninstallAction {
                    status: "skipped",
                    ..item
                })
            }
            Err(error) => {
                actions.push(UninstallAction {
                    status: "failed",
                    ..item
                });
                return Ok(UninstallReport {
                    schema_version: 1,
                    status: "failed",
                    manager: "native",
                    installation_root: Some(installation.root.clone()),
                    actions,
                    preserved: planned.preserved,
                    warnings: vec![format!("failed to remove an installation entry: {error}")],
                });
            }
        }
    }
    if let Some(profile) = shell_profile {
        match remove_path_block(&profile.0, &profile.1) {
            Ok(true) => actions.push(action_with_status(
                "shell-profile",
                profile.0,
                "LazyDB installer PATH block",
                "completed",
            )),
            Ok(false) => warnings.push(format!(
                "PATH configuration was not changed: {}",
                profile.0.display()
            )),
            Err(error) => warnings.push(format!("could not update PATH configuration: {error}")),
        }
    }
    Ok(UninstallReport {
        schema_version: 1,
        status: "complete",
        manager: "native",
        installation_root: Some(installation.root.clone()),
        actions,
        preserved: planned.preserved,
        warnings,
    })
}

async fn cleanup_system_credentials(installation: &Installation) -> Result<()> {
    let profiles_path = installation.config_dir.join("connections.toml");
    if !profiles_path.exists() {
        return Ok(());
    }
    let profiles = ProfileStore::new(profiles_path).load().map_err(|error| {
        anyhow::anyhow!("cannot validate stored credentials before purge: {error}")
    })?;
    let mut ids = std::collections::BTreeSet::new();
    for profile in profiles.profiles {
        if matches!(
            profile.credential_policy,
            CredentialPolicy::System(_) | CredentialPolicy::Keyring(_)
        ) {
            ids.insert(profile.id);
        }
    }
    if ids.is_empty() {
        return Ok(());
    }
    let store = NativeSecretStore;
    store
        .available()
        .await
        .map_err(|error| anyhow::anyhow!("native credential store is unavailable: {error}"))?;
    for id in ids {
        store.delete(id).await.map_err(|error| {
            anyhow::anyhow!("failed to remove credential for profile {id}: {error}")
        })?;
    }
    Ok(())
}

fn installation_shell_profile(installation: &Installation) -> Option<(PathBuf, String)> {
    installation
        .state
        .shell_profiles
        .first()
        .map(|profile| (profile.path.clone(), profile.block_sha256.clone()))
}

fn remove_path_block(profile: &Path, expected_sha256: &str) -> std::io::Result<bool> {
    let target = profile;
    let old = fs::read_to_string(target)?;
    let begin = "# >>> LazyDB installer >>>";
    let end = "# <<< LazyDB installer <<<";
    let lines: Vec<&str> = old.split_inclusive('\n').collect();
    let starts: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim_end_matches(['\r', '\n']) == begin)
        .map(|(index, _)| index)
        .collect();
    let ends: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim_end_matches(['\r', '\n']) == end)
        .map(|(index, _)| index)
        .collect();
    if starts.len() != 1 || ends.len() != 1 || starts[0] >= ends[0] {
        return Ok(false);
    }
    let block = lines[starts[0]..=ends[0]].concat();
    let actual_sha256 = format!("{:x}", sha2::Sha256::digest(block.as_bytes()));
    if actual_sha256 != expected_sha256 {
        return Ok(false);
    }
    let mut updated = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index < starts[0] || index > ends[0] {
            updated.push_str(line);
        }
    }
    if updated != old {
        fs::write(target, updated)?;
    }
    Ok(true)
}

fn action_with_status(
    kind: &'static str,
    path: PathBuf,
    reason: &'static str,
    status: &'static str,
) -> UninstallAction {
    UninstallAction {
        kind,
        path,
        reason,
        status,
    }
}

fn remove_entry(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || metadata.file_type().is_file() => {
            fs::remove_file(path)
        }
        Ok(metadata) if metadata.file_type().is_dir() => fs::remove_dir(path),
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "unsupported installation entry",
        )),
        Err(error) => Err(error),
    }
}

fn action(kind: &'static str, path: PathBuf, reason: &'static str) -> UninstallAction {
    UninstallAction {
        kind,
        path,
        reason,
        status: "planned",
    }
}

fn format_report(report: &UninstallReport) -> String {
    let mut output = format!("LazyDB uninstall {}\n", report.status);
    if let Some(root) = &report.installation_root {
        output.push_str(&format!("installation root: {}\n", root.display()));
    }
    for action in &report.actions {
        output.push_str(&format!(
            "remove: {} ({})\n",
            action.path.display(),
            action.reason
        ));
    }
    for path in &report.preserved {
        output.push_str(&format!("preserve: {}\n", path.display()));
    }
    for warning in &report.warnings {
        output.push_str(&format!("warning: {warning}\n"));
    }
    output
}

#[cfg(test)]
mod tests {
    use std::fs;

    use sha2::Digest;
    use tempfile::tempdir;

    use super::{discover_from_candidates, plan_installation, run};
    use crate::cli::UninstallArgs;
    use crate::update::InstallationManager;

    #[tokio::test]
    async fn json_requires_non_interactive_confirmation_or_preview() {
        let error = run(
            UninstallArgs {
                dry_run: false,
                yes: false,
                purge: false,
                json: true,
            },
            None,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("--json uninstall requires"));
    }

    fn install_tree(root: &std::path::Path, launcher: &std::path::Path) {
        fs::create_dir_all(root.join("releases/0.1.0")).unwrap();
        fs::write(root.join("releases/0.1.0/lazydb"), "binary").unwrap();
        std::os::unix::fs::symlink(root.join("releases/0.1.0"), root.join("current")).unwrap();
        fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(root.join("current/lazydb"), launcher).unwrap();
        fs::write(
            root.join("install.json"),
            serde_json::json!({
                "schema": 1,
                "product": "lazydb",
                "manager": "native",
                "channel": "stable",
                "version": "0.1.0",
                "target": "x86_64-unknown-linux-gnu",
                "path": launcher,
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn discovers_native_installation_and_plans_data_preservation() {
        let directory = tempdir().unwrap();
        let root = directory.path().join(".config/lazydb");
        let launcher = directory.path().join(".local/bin/lazydb");
        install_tree(&root, &launcher);
        for name in ["connections.toml", "credential.key", "settings.toml"] {
            fs::write(root.join(name), "preserve").unwrap();
        }

        let installation = discover_from_candidates(vec![root.clone()]).unwrap();
        assert_eq!(installation.root, root);
        assert_eq!(installation.launcher, launcher);
        let report = plan_installation(&installation, false);
        assert_eq!(report.manager, "native");
        assert_eq!(report.actions.len(), 4);
        assert!(
            report
                .preserved
                .iter()
                .any(|path| path.ends_with("credential.key"))
        );
    }

    #[test]
    fn rejects_launcher_pointing_outside_recorded_installation_root() {
        let directory = tempdir().unwrap();
        let root = directory.path().join(".config/lazydb");
        let external = directory.path().join("external");
        let launcher = directory.path().join(".local/bin/lazydb");
        install_tree(&external, &launcher);
        fs::create_dir_all(&root).unwrap();
        fs::copy(external.join("install.json"), root.join("install.json")).unwrap();
        let error = discover_from_candidates(vec![root]).unwrap_err();
        assert!(error.contains("not the recorded config directory"));
        assert!(external.join("releases/0.1.0/lazydb").exists());
    }

    #[test]
    fn rejects_non_native_installation_state() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("config");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("install.json"),
            serde_json::json!({
                "schema": 1,
                "product": "lazydb",
                "manager": "cargo",
                "channel": "stable",
                "version": "0.1.0",
                "target": "x86_64-unknown-linux-gnu",
                "path": "/tmp/lazydb",
            })
            .to_string(),
        )
        .unwrap();
        let error = discover_from_candidates(vec![root]).unwrap_err();
        assert!(error.contains("installation state manager must be native"));
        let _ = InstallationManager::Native;
    }

    #[tokio::test]
    async fn executes_default_uninstall_and_preserves_user_data() {
        let directory = tempdir().unwrap();
        let root = directory.path().join(".config/lazydb");
        let launcher = directory.path().join(".local/bin/lazydb");
        install_tree(&root, &launcher);
        fs::write(root.join("connections.toml"), "preserve").unwrap();
        fs::write(root.join("credential.key"), "preserve").unwrap();
        fs::write(root.join("unknown.txt"), "preserve").unwrap();

        let installation = discover_from_candidates(vec![root.clone()]).unwrap();
        let report = super::execute_installation(&installation, false)
            .await
            .unwrap();
        assert_eq!(report.status, "complete");
        assert!(!launcher.exists());
        assert!(!root.join("current").exists());
        assert!(!root.join("releases/0.1.0").exists());
        assert!(!root.join("install.json").exists());
        assert_eq!(
            fs::read_to_string(root.join("connections.toml")).unwrap(),
            "preserve"
        );
        assert!(root.join("credential.key").exists());
        assert!(root.join("unknown.txt").exists());
    }

    #[tokio::test]
    async fn purge_removes_allowlisted_data_but_keeps_unknown_files() {
        let directory = tempdir().unwrap();
        let root = directory.path().join(".config/lazydb");
        let launcher = directory.path().join(".local/bin/lazydb");
        install_tree(&root, &launcher);
        fs::write(
            root.join("connections.toml"),
            "version = 6\ngroups = []\nprofiles = []\n",
        )
        .unwrap();
        for name in [
            "credential.key",
            "settings.toml",
            "workspace.toml",
            "update-check.json",
        ] {
            fs::write(root.join(name), "preserve").unwrap();
        }
        fs::write(root.join("user-data.db"), "keep").unwrap();

        let installation = discover_from_candidates(vec![root.clone()]).unwrap();
        let report = super::execute_installation(&installation, true)
            .await
            .unwrap();
        assert_eq!(report.status, "complete");
        for name in [
            "connections.toml",
            "credential.key",
            "settings.toml",
            "workspace.toml",
            "update-check.json",
        ] {
            assert!(!root.join(name).exists(), "{name} should be purged");
        }
        assert!(root.join("user-data.db").exists());
    }

    #[test]
    fn removes_only_an_intact_lazydb_path_block() {
        let directory = tempdir().unwrap();
        let profile = directory.path().join(".bashrc");
        fs::write(
            &profile,
            "export PATH=\"/usr/local/bin:$PATH\"\n# >>> LazyDB installer >>>\nexport PATH=/tmp/lazydb:$PATH\n# <<< LazyDB installer <<<\n# user config\n",
        )
        .unwrap();
        let block = "# >>> LazyDB installer >>>\nexport PATH=/tmp/lazydb:$PATH\n# <<< LazyDB installer <<<\n";
        let digest = format!("{:x}", sha2::Sha256::digest(block.as_bytes()));
        assert!(super::remove_path_block(&profile, &digest).unwrap());
        assert_eq!(
            fs::read_to_string(profile).unwrap(),
            "export PATH=\"/usr/local/bin:$PATH\"\n# user config\n"
        );
    }

    #[test]
    fn leaves_duplicate_or_incomplete_path_blocks_unchanged() {
        let directory = tempdir().unwrap();
        for contents in [
            "# >>> LazyDB installer >>>\n# <<< LazyDB installer <<<\n# >>> LazyDB installer >>>\n# <<< LazyDB installer <<<\n",
            "# >>> LazyDB installer >>>\nexport PATH=/tmp/lazydb:$PATH\n",
        ] {
            let profile = directory.path().join(contents.len().to_string());
            fs::write(&profile, contents).unwrap();
            assert!(!super::remove_path_block(&profile, "bad").unwrap());
            assert_eq!(fs::read_to_string(profile).unwrap(), contents);
        }
    }
}
