use crate::model::pane_navigation::PaneDirection;
use crate::{
    action::Action,
    app::App,
    model::{
        redis_browser::RedisBrowserFocus,
        tab::WorkspaceTab,
        workspace::{Focus, redis_pane_resize},
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SmartPaneCommand {
    Left,
    Down,
    Up,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SmartResizePaneCommand {
    Left,
    Down,
    Up,
    Right,
}

impl SmartResizePaneCommand {
    pub(crate) const ALL: [Self; 4] = [Self::Left, Self::Down, Self::Up, Self::Right];
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Left => "smart-resize-pane-left",
            Self::Down => "smart-resize-pane-down",
            Self::Up => "smart-resize-pane-up",
            Self::Right => "smart-resize-pane-right",
        }
    }
    pub(crate) fn direction(self) -> PaneDirection {
        match self {
            Self::Left => PaneDirection::Left,
            Self::Down => PaneDirection::Down,
            Self::Up => PaneDirection::Up,
            Self::Right => PaneDirection::Right,
        }
    }
}

impl SmartPaneCommand {
    pub(crate) const ALL: [Self; 4] = [Self::Left, Self::Down, Self::Up, Self::Right];
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Left => "smart-focus-pane-left",
            Self::Down => "smart-focus-pane-down",
            Self::Up => "smart-focus-pane-up",
            Self::Right => "smart-focus-pane-right",
        }
    }
    pub(crate) fn direction(self) -> PaneDirection {
        match self {
            Self::Left => PaneDirection::Left,
            Self::Down => PaneDirection::Down,
            Self::Up => PaneDirection::Up,
            Self::Right => PaneDirection::Right,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PaneCommand {
    Left,
    Down,
    Up,
    Right,
    ToggleMaximized,
    ResetSizes,
}

impl PaneCommand {
    pub(crate) const ALL: [Self; 6] = [
        Self::Left,
        Self::Down,
        Self::Up,
        Self::Right,
        Self::ToggleMaximized,
        Self::ResetSizes,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Left => "focus-pane-left",
            Self::Down => "focus-pane-down",
            Self::Up => "focus-pane-up",
            Self::Right => "focus-pane-right",
            Self::ToggleMaximized => "toggle-pane-maximized",
            Self::ResetSizes => "reset-pane-sizes",
        }
    }

    pub(crate) fn direction(self) -> Option<char> {
        match self {
            Self::Left => Some('h'),
            Self::Down => Some('j'),
            Self::Up => Some('k'),
            Self::Right => Some('l'),
            Self::ToggleMaximized | Self::ResetSizes => None,
        }
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.name() == name)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PaneDispatch {
    Consumed,
    Action(Box<Action>),
}

pub(crate) fn dispatch(command: PaneCommand, app: &App) -> PaneDispatch {
    if let Some(WorkspaceTab::RedisBrowser(tab)) = app.tabs.get(app.active_tab)
        && app.focus == Focus::Results
        && tab.focus == RedisBrowserFocus::Preview
        && let Some(direction) = command.direction()
    {
        let keys_focused = tab.focus == RedisBrowserFocus::Keys;
        let resize = redis_pane_resize(Focus::Results, keys_focused, direction, 1);
        return match resize {
            Some(resize) => PaneDispatch::Action(Box::new(Action::ResizePane(resize))),
            _ => PaneDispatch::Consumed,
        };
    }

    match command {
        PaneCommand::Left => match app.focus {
            Focus::Editor | Focus::Results => {
                PaneDispatch::Action(Box::new(Action::Focus(Focus::Explorer)))
            }
            Focus::Explorer => PaneDispatch::Consumed,
        },
        PaneCommand::Down if app.focus == Focus::Editor => {
            PaneDispatch::Action(Box::new(Action::Focus(Focus::Results)))
        }
        PaneCommand::Up if app.focus == Focus::Results && !app.is_active_relation_tab() => {
            PaneDispatch::Action(Box::new(Action::Focus(Focus::Editor)))
        }
        PaneCommand::Right if app.focus == Focus::Explorer => PaneDispatch::Action(Box::new(
            Action::Focus(if app.active_console_opt().is_none() {
                Focus::Results
            } else {
                Focus::Editor
            }),
        )),
        PaneCommand::ToggleMaximized => PaneDispatch::Action(Box::new(Action::TogglePaneMaximized)),
        PaneCommand::ResetSizes => PaneDispatch::Action(Box::new(Action::ResetPaneSizes)),
        _ => PaneDispatch::Consumed,
    }
}

pub(crate) fn action_for_help(name: &str, app: &App) -> Option<Action> {
    let command = PaneCommand::from_name(name)?;
    match dispatch(command, app) {
        PaneDispatch::Action(action) => Some(*action),
        PaneDispatch::Consumed => None,
    }
}
