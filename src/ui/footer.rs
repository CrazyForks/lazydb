use crate::{
    app::App,
    model::{update::UpdateState, workspace::ConnectionStatus},
};
use ratatui::{
    Frame,
    buffer::CellWidth,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::{HitRegion, HitTarget, Theme, UiState, shortcut_hints::ShortcutHint};

struct UpdateLabel {
    text: String,
    color: ratatui::style::Color,
    emphasized: bool,
}

fn update_label(app: &App, theme: Theme, icons: super::icons::IconSet) -> Option<UpdateLabel> {
    let marker = if icons.mode() == super::icons::IconMode::Ascii {
        "^"
    } else {
        "↑"
    };
    match &app.update_state {
        UpdateState::Checking { .. } => Some(UpdateLabel {
            text: " Checking".to_owned(),
            color: theme.muted,
            emphasized: false,
        }),
        UpdateState::Available(_) | UpdateState::ManagerActionRequired(_) => Some(UpdateLabel {
            text: format!(" {marker} Update"),
            color: theme.warning,
            emphasized: true,
        }),
        UpdateState::Installing { .. } => Some(UpdateLabel {
            text: " Updating".to_owned(),
            color: theme.action,
            emphasized: false,
        }),
        UpdateState::ReadyToRestart(_) => Some(UpdateLabel {
            text: " Restart".to_owned(),
            color: theme.success,
            emphasized: true,
        }),
        UpdateState::Failed { .. } => Some(UpdateLabel {
            text: " ! Update failed".to_owned(),
            color: theme.error,
            emphasized: false,
        }),
        UpdateState::Idle | UpdateState::UpToDate(_) => None,
    }
}

fn connection_status(app: &App, theme: Theme) -> Option<(&'static str, ratatui::style::Color)> {
    if app.is_editor_target_switch_pending() {
        Some((" TARGET ", theme.warning))
    } else {
        match app.connection.status {
            ConnectionStatus::Connecting => Some((" LINKING ", theme.warning)),
            ConnectionStatus::Failed => Some((" FAILED ", theme.error)),
            ConnectionStatus::Disconnected | ConnectionStatus::Connected => None,
        }
    }
}

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
    _sequence: Option<&crate::input::keymap::KeySequenceState>,
    state: &mut UiState,
) {
    let brand = " LAZYDB ";
    let version = format!(" v{}", env!("CARGO_PKG_VERSION"));
    let update = update_label(app, theme, state.activity_icons);
    let update_text = usize::from(update.as_ref().map_or(0, |value| value.text.cell_width()));
    let version_width =
        usize::from(brand.cell_width()) + usize::from(version.cell_width()) + update_text;
    let connection = connection_status(app, theme);
    let selection = state
        .terminal_selection_mode
        .then_some(" TERMINAL SELECTION | Esc ");
    let right_width =
        connection.map_or(0, |(text, _)| text.cell_width()) + selection.map_or(0, str::cell_width);
    let hints = crate::help::footer_shortcuts_with_bindings(
        crate::help::shortcut_context(app),
        crate::help::shortcut_capabilities(app),
        Some(&app.key_bindings),
    )
    .into_iter()
    .map(|shortcut| {
        ShortcutHint::new(
            crate::help::configured_sequence(&shortcut, Some(&app.key_bindings)),
            shortcut.description,
        )
    })
    .collect::<Vec<_>>();
    let hint_width = area
        .width
        .saturating_sub(version_width as u16)
        .saturating_sub(right_width as u16)
        .saturating_sub(2);
    let hint_line = super::shortcut_hints::line(&hints, hint_width, theme, theme.surface);

    let mut spans = vec![Span::styled(
        brand,
        Style::new()
            .fg(theme.background)
            .bg(theme.accent)
            .add_modifier(Modifier::BOLD),
    )];
    let version_start = area.x + brand.cell_width();
    spans.push(Span::styled(
        version,
        Style::new().fg(theme.muted).bg(theme.surface),
    ));
    if let Some(update) = update {
        let style = Style::new().fg(update.color).bg(theme.surface);
        spans.push(Span::styled(
            update.text,
            if update.emphasized {
                style.add_modifier(Modifier::BOLD)
            } else {
                style
            },
        ));
    }
    spans.push(Span::styled("  ", Style::new().bg(theme.surface)));
    spans.extend(hint_line.spans);
    if let Some((text, color)) = connection {
        spans.push(Span::styled(
            text,
            Style::new()
                .fg(color)
                .bg(theme.surface)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(text) = selection {
        spans.push(Span::styled(
            text,
            Style::new().fg(theme.warning).bg(theme.surface),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(theme.surface)),
        area,
    );

    let visible_width = version_width.min(usize::from(area.width));
    let brand_width = usize::from(brand.cell_width());
    let available_after_brand = usize::from(area.width.saturating_sub(brand.cell_width()));
    let hit_width = visible_width
        .saturating_sub(brand_width)
        .min(available_after_brand);
    if hit_width > 0 {
        state.hit_regions.push(HitRegion {
            area: Rect::new(version_start, area.y, hit_width as u16, 1),
            target: HitTarget::UpdateCenter,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::App,
        model::update::UpdateOperation,
        profile::import_connection_url,
        update::{InstallationManager, UpdateChannel, UpdateInspection, UpdateStage},
    };

    fn app_with_state(state: UpdateState) -> App {
        let profile = import_connection_url("sqlite::memory:", Some("footer-test"))
            .unwrap()
            .profile;
        let mut app = App::new(vec![profile]);
        app.update_state = state;
        app
    }

    fn inspection(status: crate::update::UpdateStatus) -> UpdateInspection {
        UpdateInspection {
            manager: InstallationManager::Native,
            channel: UpdateChannel::Stable,
            running_version: "0.1.6".to_owned(),
            installed_version: None,
            target_version: None,
            status,
            action: None,
            launcher_path: None,
        }
    }

    #[test]
    fn update_labels_encode_all_non_idle_states() {
        let theme = Theme::default();
        let cases = [
            (
                UpdateState::Checking {
                    request_id: 1,
                    automatic: true,
                },
                "Checking",
            ),
            (
                UpdateState::Available(inspection(crate::update::UpdateStatus::Available)),
                "↑ Update",
            ),
            (
                UpdateState::ManagerActionRequired(inspection(
                    crate::update::UpdateStatus::ManagerActionRequired,
                )),
                "↑ Update",
            ),
            (
                UpdateState::Installing {
                    request_id: 1,
                    inspection: inspection(crate::update::UpdateStatus::Available),
                    progress: crate::update::UpdateProgress {
                        stage: UpdateStage::Preparing,
                        downloaded_bytes: 0,
                        total_bytes: None,
                    },
                },
                "Updating",
            ),
            (
                UpdateState::ReadyToRestart(inspection(
                    crate::update::UpdateStatus::ReadyToRestart,
                )),
                "Restart",
            ),
            (
                UpdateState::Failed {
                    operation: UpdateOperation::Check,
                    message: "offline".to_owned(),
                },
                "Update failed",
            ),
        ];
        for (state, expected) in cases {
            let app = app_with_state(state);
            assert!(
                update_label(&app, theme, super::super::icons::IconSet::default())
                    .is_some_and(|label| label.text.contains(expected))
            );
        }
    }

    #[test]
    fn available_label_is_emphasized_and_idle_is_quiet() {
        let theme = Theme::default();
        let available = app_with_state(UpdateState::Available(inspection(
            crate::update::UpdateStatus::Available,
        )));
        assert!(
            update_label(&available, theme, super::super::icons::IconSet::default())
                .is_some_and(|label| label.emphasized && label.color == theme.warning)
        );
        let idle = app_with_state(UpdateState::Idle);
        assert!(update_label(&idle, theme, super::super::icons::IconSet::default()).is_none());
    }
}
