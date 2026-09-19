use lazydb::update::{UpdateProgress, UpdateStage};
use lazydb::{
    action::{Action, Command},
    app::App,
    model::{
        update::{UpdateDialogAction, UpdateState},
        workspace::Overlay,
    },
    update::{InstallationManager, UpdateChannel, UpdateInspection, UpdateStatus},
};

fn inspection(status: UpdateStatus) -> UpdateInspection {
    UpdateInspection {
        manager: InstallationManager::Native,
        channel: UpdateChannel::Stable,
        running_version: env!("CARGO_PKG_VERSION").to_owned(),
        installed_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        target_version: Some("9.9.9".to_owned()),
        status,
        action: None,
        launcher_path: Some("/tmp/lazydb".into()),
    }
}

#[test]
fn opening_update_center_starts_only_one_check() {
    let mut app = App::new(Vec::new());
    assert!(matches!(
        app.update(Action::OpenUpdateCenter).as_slice(),
        [Command::CheckForUpdate {
            request_id: 1,
            automatic: false
        }]
    ));
    assert!(matches!(
        app.update_state,
        UpdateState::Checking {
            request_id: 1,
            automatic: false
        }
    ));
    assert!(app.update(Action::OpenUpdateCenter).is_empty());
    assert!(matches!(
        app.update_state,
        UpdateState::Checking { request_id: 1, .. }
    ));
}

#[test]
fn stale_completion_is_ignored() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenUpdateCenter);
    app.update(Action::UpdateCheckCompleted {
        request_id: 99,
        inspection: inspection(UpdateStatus::Available),
    });
    assert!(matches!(
        app.update_state,
        UpdateState::Checking { request_id: 1, .. }
    ));
}

#[test]
fn later_closes_overlay_and_keeps_available_state() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenUpdateCenter);
    app.update(Action::UpdateCheckCompleted {
        request_id: 1,
        inspection: inspection(UpdateStatus::Available),
    });
    app.update(Action::UpdateOverlayConfirm);
    assert!(app.overlay.is_none());
    assert!(matches!(app.update_state, UpdateState::Available(_)));
}

#[test]
fn primary_confirmation_starts_native_install() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenUpdateCenter);
    app.update(Action::UpdateCheckCompleted {
        request_id: 1,
        inspection: inspection(UpdateStatus::Available),
    });
    app.update(Action::UpdateOverlayToggleFocus);
    assert!(matches!(
        app.update(Action::UpdateOverlayConfirm).as_slice(),
        [Command::InstallUpdate {
            request_id: 2,
            channel: UpdateChannel::Stable
        }]
    ));
    assert!(matches!(
        app.update_state,
        UpdateState::Installing { request_id: 2, .. }
    ));
}

#[test]
fn install_failure_is_retryable() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenUpdateCenter);
    app.update(Action::UpdateCheckCompleted {
        request_id: 1,
        inspection: inspection(UpdateStatus::Available),
    });
    app.update(Action::UpdateOverlayToggleFocus);
    app.update(Action::UpdateOverlayConfirm);
    app.update(Action::UpdateInstallFailed {
        request_id: 2,
        message: "network down".into(),
    });
    assert!(matches!(app.update_state, UpdateState::Failed { .. }));
    assert!(matches!(app.overlay, Some(Overlay::Update(_))));
}

#[test]
fn unknown_installation_requires_manual_action_without_starting_install() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenUpdateCenter);
    let mut result = inspection(UpdateStatus::ManagerActionRequired);
    result.manager = InstallationManager::Unknown;
    result.action = Some("update using the original installation method".into());
    result.launcher_path = None;
    app.update(Action::UpdateCheckCompleted {
        request_id: 1,
        inspection: result,
    });
    assert!(matches!(
        app.update_state,
        UpdateState::ManagerActionRequired(_)
    ));
    app.update(Action::UpdateOverlayToggleFocus);
    assert!(app.update(Action::UpdateOverlayConfirm).is_empty());
}

fn progress(downloaded: u64, total: Option<u64>) -> UpdateProgress {
    UpdateProgress {
        stage: UpdateStage::Downloading,
        downloaded_bytes: downloaded,
        total_bytes: total,
    }
}

/// Regression: the default focus is the safe "Not now" action, but activating
/// the install button directly (as a mouse click does) must still install.
#[test]
fn activating_install_directly_installs_regardless_of_keyboard_focus() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenUpdateCenter);
    app.update(Action::UpdateCheckCompleted {
        request_id: 1,
        inspection: inspection(UpdateStatus::Available),
    });
    let commands = app.update(Action::UpdateOverlayActivate(UpdateDialogAction::Install));
    assert!(matches!(
        commands.as_slice(),
        [Command::InstallUpdate { request_id: 2, .. }]
    ));
    assert!(matches!(
        app.update_state,
        UpdateState::Installing { request_id: 2, .. }
    ));
}

/// Enter on the single "OK" action must not silently start another check.
#[test]
fn up_to_date_confirm_closes_without_rechecking() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenUpdateCenter);
    app.update(Action::UpdateCheckCompleted {
        request_id: 1,
        inspection: inspection(UpdateStatus::UpToDate),
    });
    assert!(app.update(Action::UpdateOverlayConfirm).is_empty());
    assert!(app.overlay.is_none());
    assert!(matches!(app.update_state, UpdateState::UpToDate(_)));
}

#[test]
fn copy_command_copies_the_manager_command_without_installing() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenUpdateCenter);
    let mut result = inspection(UpdateStatus::ManagerActionRequired);
    result.manager = InstallationManager::Cargo;
    result.action = Some("cargo install lazydb".into());
    app.update(Action::UpdateCheckCompleted {
        request_id: 1,
        inspection: result,
    });
    app.update(Action::UpdateOverlayActivate(
        UpdateDialogAction::ShowInstructions,
    ));
    let commands = app.update(Action::UpdateOverlayActivate(
        UpdateDialogAction::CopyCommand,
    ));
    match commands.as_slice() {
        [Command::WriteClipboard(payload)] => {
            assert_eq!(payload.text, "cargo install lazydb");
        }
        other => panic!("expected a clipboard command, got {other:?}"),
    }
    assert!(matches!(
        app.update_state,
        UpdateState::ManagerActionRequired(_)
    ));
}

/// Copy is only meaningful on the expanded instruction view.
#[test]
fn copy_command_without_expanded_instructions_does_nothing() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenUpdateCenter);
    let mut result = inspection(UpdateStatus::ManagerActionRequired);
    result.manager = InstallationManager::Cargo;
    app.update(Action::UpdateCheckCompleted {
        request_id: 1,
        inspection: result,
    });
    assert!(
        app.update(Action::UpdateOverlayActivate(
            UpdateDialogAction::CopyCommand
        ))
        .is_empty()
    );
}

#[test]
fn recheck_does_not_discard_ready_to_restart() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenUpdateCenter);
    app.update(Action::UpdateCheckCompleted {
        request_id: 1,
        inspection: inspection(UpdateStatus::ReadyToRestart),
    });
    assert!(matches!(app.update_state, UpdateState::ReadyToRestart(_)));
    assert!(
        app.update(Action::StartUpdateCheck { automatic: false })
            .is_empty()
    );
    assert!(matches!(app.update_state, UpdateState::ReadyToRestart(_)));
}

#[test]
fn install_progress_updates_only_the_current_request() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenUpdateCenter);
    app.update(Action::UpdateCheckCompleted {
        request_id: 1,
        inspection: inspection(UpdateStatus::Available),
    });
    app.update(Action::UpdateOverlayActivate(UpdateDialogAction::Install));
    app.update(Action::UpdateInstallProgress {
        request_id: 2,
        progress: progress(10, Some(100)),
    });
    match &app.update_state {
        UpdateState::Installing { progress, .. } => assert_eq!(progress.downloaded_bytes, 10),
        other => panic!("expected installing state, got {other:?}"),
    }
    // A stale request must not touch the active installation.
    app.update(Action::UpdateInstallProgress {
        request_id: 99,
        progress: progress(80, Some(100)),
    });
    match &app.update_state {
        UpdateState::Installing { progress, .. } => assert_eq!(progress.downloaded_bytes, 10),
        other => panic!("expected installing state, got {other:?}"),
    }
}

#[test]
fn late_progress_after_completion_is_ignored() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenUpdateCenter);
    app.update(Action::UpdateCheckCompleted {
        request_id: 1,
        inspection: inspection(UpdateStatus::Available),
    });
    app.update(Action::UpdateOverlayActivate(UpdateDialogAction::Install));
    let mut installed = inspection(UpdateStatus::ReadyToRestart);
    installed.installed_version = Some("9.9.9".to_owned());
    app.update(Action::UpdateInstalled {
        request_id: 2,
        inspection: installed,
    });
    assert!(matches!(app.update_state, UpdateState::ReadyToRestart(_)));
    app.update(Action::UpdateInstallProgress {
        request_id: 2,
        progress: progress(90, Some(100)),
    });
    assert!(matches!(app.update_state, UpdateState::ReadyToRestart(_)));
}

/// A concurrent process may finish the install first, in which case the
/// verified installed version simply matches the running one. That is a
/// success, not a failure.
#[test]
fn install_completed_in_another_process_is_not_reported_as_failure() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenUpdateCenter);
    app.update(Action::UpdateCheckCompleted {
        request_id: 1,
        inspection: inspection(UpdateStatus::Available),
    });
    app.update(Action::UpdateOverlayActivate(UpdateDialogAction::Install));
    let mut already_installed = inspection(UpdateStatus::UpToDate);
    already_installed.installed_version = Some(env!("CARGO_PKG_VERSION").to_owned());
    app.update(Action::UpdateInstalled {
        request_id: 2,
        inspection: already_installed,
    });
    assert!(matches!(app.update_state, UpdateState::UpToDate(_)));
}

#[test]
fn verified_install_with_a_newer_version_offers_restart() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenUpdateCenter);
    app.update(Action::UpdateCheckCompleted {
        request_id: 1,
        inspection: inspection(UpdateStatus::Available),
    });
    app.update(Action::UpdateOverlayActivate(UpdateDialogAction::Install));
    let mut installed = inspection(UpdateStatus::UpToDate);
    installed.installed_version = Some("9.9.9".to_owned());
    app.update(Action::UpdateInstalled {
        request_id: 2,
        inspection: installed,
    });
    assert!(matches!(app.update_state, UpdateState::ReadyToRestart(_)));
}
