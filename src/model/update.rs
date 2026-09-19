use crate::update::{
    InstallationManager, UpdateInspection, UpdateProgress, UpdateStatus, manager_update_command,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateOperation {
    Check,
    Install,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking {
        request_id: u64,
        automatic: bool,
    },
    UpToDate(UpdateInspection),
    Available(UpdateInspection),
    Installing {
        request_id: u64,
        inspection: UpdateInspection,
        progress: UpdateProgress,
    },
    ReadyToRestart(UpdateInspection),
    ManagerActionRequired(UpdateInspection),
    Failed {
        operation: UpdateOperation,
        message: String,
    },
}

impl UpdateState {
    pub fn from_inspection(inspection: UpdateInspection) -> Self {
        match inspection.status {
            UpdateStatus::UpToDate => Self::UpToDate(inspection),
            UpdateStatus::Available => Self::Available(inspection),
            UpdateStatus::ReadyToRestart => Self::ReadyToRestart(inspection),
            UpdateStatus::ManagerActionRequired => Self::ManagerActionRequired(inspection),
            UpdateStatus::Error => Self::Failed {
                operation: UpdateOperation::Check,
                message: inspection
                    .action
                    .clone()
                    .unwrap_or_else(|| "update check failed".into()),
            },
        }
    }

    /// Actions available in the update dialog, in display order. Shared by the
    /// renderer, the keyboard and the mouse so every entry point agrees on what
    /// is actionable for the current state.
    pub fn dialog_actions(&self, instructions: bool) -> Vec<UpdateDialogAction> {
        match self {
            Self::Idle => vec![UpdateDialogAction::Check, UpdateDialogAction::Close],
            Self::Checking { .. } => vec![UpdateDialogAction::Close],
            Self::UpToDate(_) => vec![UpdateDialogAction::Close],
            Self::Available(inspection) => {
                if instructions {
                    Self::instruction_actions(inspection)
                } else if inspection.manager == InstallationManager::Native {
                    vec![UpdateDialogAction::Install, UpdateDialogAction::Close]
                } else {
                    vec![
                        UpdateDialogAction::ShowInstructions,
                        UpdateDialogAction::Close,
                    ]
                }
            }
            Self::ManagerActionRequired(inspection) => {
                if instructions {
                    Self::instruction_actions(inspection)
                } else {
                    vec![
                        UpdateDialogAction::ShowInstructions,
                        UpdateDialogAction::Close,
                    ]
                }
            }
            Self::Installing { .. } => vec![UpdateDialogAction::Close],
            Self::ReadyToRestart(_) => {
                vec![UpdateDialogAction::Restart, UpdateDialogAction::Close]
            }
            Self::Failed { .. } => vec![UpdateDialogAction::Check, UpdateDialogAction::Close],
        }
    }

    fn instruction_actions(inspection: &UpdateInspection) -> Vec<UpdateDialogAction> {
        if manager_update_command(inspection.manager).is_some() {
            vec![UpdateDialogAction::CopyCommand, UpdateDialogAction::Close]
        } else {
            vec![UpdateDialogAction::Close]
        }
    }

    /// Whether the `r` shortcut may start another check without discarding a
    /// meaningful result (installing, ready to restart, or checking).
    pub fn can_check_again(&self) -> bool {
        matches!(self, Self::Idle | Self::UpToDate(_) | Self::Failed { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateOverlayFocus {
    Later,
    Primary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateDialogAction {
    Check,
    Install,
    ShowInstructions,
    CopyCommand,
    Restart,
    Close,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateOverlayState {
    pub focus: UpdateOverlayFocus,
    pub instructions: bool,
}
