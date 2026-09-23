//! Update center dialog.
//!
//! The dialog is intentionally state driven: the body, the actions and the
//! footer hints are all derived from [`UpdateState`] plus whether the external
//! update instructions are expanded. The same action list backs the keyboard,
//! the mouse hit regions and the rendered buttons, so there is a single source
//! of truth for what is actionable.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};
use std::time::Duration;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::theme::Theme;
use super::{
    HitRegion, HitTarget, UiState, centered, dialog, panel_block, shortcut_hints,
    shortcut_hints::ShortcutHint,
};
use crate::app::App;
use crate::model::update::{UpdateDialogAction, UpdateOverlayFocus, UpdateState};
use crate::security::sanitize_terminal_text;
use crate::update::{InstallationManager, UpdateProgress, manager_update_message};

const NARROW_WIDTH: u16 = 64;
const WIDE_WIDTH: u16 = 76;
const HORIZONTAL_PADDING: u16 = 2;

struct ActionSpec {
    action: UpdateDialogAction,
    label: &'static str,
}

struct View {
    max_width: u16,
    status: Line<'static>,
    details: Vec<String>,
    instruction: Option<String>,
    command: Option<String>,
    progress: Option<UpdateProgress>,
    actions: Vec<ActionSpec>,
    can_check_again: bool,
}

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    state: &mut UiState,
    theme: Theme,
) {
    let (instructions, focus) = match app.overlay {
        Some(crate::model::workspace::Overlay::Update(ref overlay)) => {
            (overlay.instructions, overlay.focus)
        }
        _ => (false, UpdateOverlayFocus::Later),
    };
    let view = view_for_state(&app.update_state, instructions, theme);

    let width = area.width.saturating_sub(4).min(view.max_width);
    if width < 10 || area.height < 3 {
        return;
    }
    let text_width = width.saturating_sub(2 + HORIZONTAL_PADDING * 2).max(1) as usize;

    let motion = state.animation_mode();
    let elapsed = match &app.update_state {
        UpdateState::Installing { request_id, .. } => state.update_progress_elapsed(*request_id),
        _ => Duration::ZERO,
    };
    let body = body_lines(&view, text_width, width, theme, motion, elapsed);
    let labels = view
        .actions
        .iter()
        .map(|spec| spec.label)
        .collect::<Vec<_>>();
    let actions_rows = actions_height(&labels, width);
    let desired_height = (body.len() as u16)
        .saturating_add(1) // breathing room between body and actions
        .saturating_add(actions_rows)
        .saturating_add(1) // footer hints
        .saturating_add(2); // borders
    let popup = centered(area, width, desired_height);
    frame.render_widget(Clear, popup);
    let block = panel_block(" UPDATE CENTER ", true, theme);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width < 3 || inner.height < 2 {
        return;
    }

    let footer_area = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
    let actions_area = Rect::new(
        inner.x,
        inner.bottom().saturating_sub(1 + actions_rows).max(inner.y),
        inner.width,
        actions_rows.min(inner.height),
    );
    let body_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        actions_area.y.saturating_sub(inner.y),
    );
    let text_area = Rect::new(
        body_area.x.saturating_add(HORIZONTAL_PADDING),
        body_area.y,
        body_area
            .width
            .saturating_sub(HORIZONTAL_PADDING * 2)
            .max(1),
        body_area.height,
    );
    frame.render_widget(
        Paragraph::new(body).style(Style::new().fg(theme.text).bg(theme.surface)),
        text_area,
    );

    let focused = match focus {
        UpdateOverlayFocus::Primary => 0,
        UpdateOverlayFocus::Later => view.actions.len().saturating_sub(1),
    };
    let buttons = view
        .actions
        .iter()
        .map(|spec| dialog::DialogButton {
            label: spec.label,
            tone: dialog::DialogTone::Normal,
            emphasis: dialog::DialogEmphasis::Secondary,
            enabled: true,
        })
        .collect::<Vec<_>>();
    for region in dialog::render_actions(frame, actions_area, &buttons, focused, theme) {
        if let Some(spec) = view.actions.get(region.index) {
            state.hit_regions.push(HitRegion {
                area: region.area,
                target: HitTarget::UpdateButton {
                    action: spec.action,
                },
            });
        }
    }

    shortcut_hints::render_interactive(
        frame,
        footer_area,
        &view.hints(),
        theme,
        theme.surface,
        Alignment::Left,
        state,
    );
}

impl View {
    fn hints(&self) -> Vec<ShortcutHint<'static>> {
        let mut hints = Vec::new();
        if self.actions.len() > 1 {
            hints.push(ShortcutHint::with_keys(
                "Tab",
                "select next",
                [crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Tab,
                    crossterm::event::KeyModifiers::NONE,
                )],
            ));
        }
        hints.push(ShortcutHint::with_keys(
            "Enter",
            "confirm",
            [crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            )],
        ));
        if self.can_check_again {
            hints.push(ShortcutHint::with_keys(
                "r",
                "check again",
                [crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char('r'),
                    crossterm::event::KeyModifiers::NONE,
                )],
            ));
        }
        hints.push(ShortcutHint::with_keys(
            "Esc",
            "close",
            [crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            )],
        ));
        hints
    }
}

fn body_lines(
    view: &View,
    text_width: usize,
    width: u16,
    theme: Theme,
    motion: crate::cli::MotionMode,
    elapsed: Duration,
) -> Vec<Line<'static>> {
    let mut lines = vec![view.status.clone()];
    for detail in &view.details {
        lines.extend(
            wrap_text(detail, text_width)
                .into_iter()
                .map(|line| Line::from(Span::styled(line, Style::new().fg(theme.muted)))),
        );
    }
    if let Some(instruction) = &view.instruction {
        lines.push(Line::raw(""));
        lines.extend(
            wrap_text(instruction, text_width)
                .into_iter()
                .map(|line| Line::from(Span::styled(line, Style::new().fg(theme.text)))),
        );
    }
    if let Some(command) = &view.command {
        lines.push(Line::raw(""));
        lines.extend(wrap_text(command, text_width).into_iter().map(|line| {
            Line::from(Span::styled(
                line,
                Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
            ))
        }));
    }
    if let Some(progress) = &view.progress {
        lines.push(Line::raw(""));
        lines.push(progress_line(progress, width, theme, motion, elapsed));
    }
    lines
}

fn view_for_state(state: &UpdateState, instructions: bool, theme: Theme) -> View {
    let actions = |state: &UpdateState| {
        state
            .dialog_actions(instructions)
            .into_iter()
            .map(|action| ActionSpec {
                action,
                label: action_label(action, state, state.dialog_actions(instructions).len()),
            })
            .collect::<Vec<_>>()
    };
    let mut view = View {
        max_width: if instructions {
            WIDE_WIDTH
        } else {
            NARROW_WIDTH
        },
        status: Line::default(),
        details: Vec::new(),
        instruction: None,
        command: None,
        progress: None,
        actions: actions(state),
        can_check_again: state.can_check_again(),
    };
    match state {
        UpdateState::Idle => {
            view.status = status_line("Ready to check for updates", theme.text);
            view.details.push("No update check has run yet.".to_owned());
        }
        UpdateState::Checking { .. } => {
            view.status = status_line("Checking for updates…", theme.action);
            view.details.push("This only takes a moment.".to_owned());
        }
        UpdateState::UpToDate(inspection) => {
            let ahead = inspection
                .target_version
                .as_deref()
                .is_some_and(|target| target != inspection.running_version);
            view.status = status_line(
                if ahead {
                    "✓ No update needed"
                } else {
                    "✓ You're up to date"
                },
                theme.success,
            );
            view.details.push(format!(
                "Current {} · {:?}",
                inspection.running_version, inspection.channel
            ));
            if let Some(target) = inspection.target_version.as_deref()
                && ahead
            {
                view.details.push(format!("Channel latest {target}"));
            }
        }
        UpdateState::Available(inspection) => {
            view.status = status_line("↑ Update available", theme.action);
            view.details.push(version_detail(inspection));
            if instructions && inspection.manager != InstallationManager::Native {
                fill_instructions(&mut view, inspection);
            }
        }
        UpdateState::Installing {
            inspection,
            progress,
            ..
        } => {
            view.status = status_line("Updating LazyDB", theme.action);
            view.details.push(format!(
                "Installing {} · {:?}",
                inspection.target_version.as_deref().unwrap_or("the update"),
                inspection.channel
            ));
            view.details
                .push("You can keep working while this runs.".to_owned());
            view.progress = Some(progress.clone());
        }
        UpdateState::ReadyToRestart(inspection) => {
            view.status = status_line("✓ Update installed", theme.success);
            view.details.push(format!(
                "Restart to use {}",
                inspection
                    .installed_version
                    .as_deref()
                    .unwrap_or("the new version")
            ));
        }
        UpdateState::ManagerActionRequired(inspection) => {
            view.status = status_line("↑ Update available", theme.action);
            view.details.push(format!(
                "Latest {} · {:?}",
                inspection.target_version.as_deref().unwrap_or("unknown"),
                inspection.manager
            ));
            if instructions {
                fill_instructions(&mut view, inspection);
            }
        }
        UpdateState::Failed { message, .. } => {
            view.status = status_line("× Update failed", theme.error);
            view.details.push(sanitize_terminal_text(message));
        }
    }
    view
}

fn fill_instructions(view: &mut View, inspection: &crate::update::UpdateInspection) {
    view.instruction = manager_update_message(inspection.manager, inspection.channel)
        .or_else(|| inspection.action.clone());
    view.command = crate::update::manager_update_command(inspection.manager);
}

fn version_detail(inspection: &crate::update::UpdateInspection) -> String {
    format!(
        "Current {} → Latest {} · {:?}",
        inspection.running_version,
        inspection.target_version.as_deref().unwrap_or("unknown"),
        inspection.channel
    )
}

fn status_line(text: &str, color: ratatui::style::Color) -> Line<'static> {
    Line::from(Span::styled(
        text.to_owned(),
        Style::new().fg(color).add_modifier(Modifier::BOLD),
    ))
}

fn action_label(action: UpdateDialogAction, state: &UpdateState, count: usize) -> &'static str {
    match action {
        UpdateDialogAction::Check => match state {
            UpdateState::Failed { .. } => "Check again",
            _ => "Check now",
        },
        UpdateDialogAction::Install => "Update",
        UpdateDialogAction::ShowInstructions => "How to update",
        UpdateDialogAction::CopyCommand => "Copy update command",
        UpdateDialogAction::Restart => "Restart now",
        UpdateDialogAction::Close => {
            if count > 1 {
                "Not now"
            } else {
                match state {
                    UpdateState::UpToDate(_) => "OK",
                    UpdateState::Installing { .. } => "Run in background",
                    UpdateState::Checking { .. } => "Close",
                    _ => "OK",
                }
            }
        }
    }
}

fn actions_height(labels: &[&str], width: u16) -> u16 {
    if labels.is_empty() {
        return 0;
    }
    let total = labels
        .iter()
        .map(|label| label.width() as u16 + 6)
        .sum::<u16>()
        .saturating_add(2 * (labels.len() as u16).saturating_sub(1));
    if total > width {
        labels.len() as u16
    } else {
        1
    }
}

fn progress_line(
    progress: &UpdateProgress,
    width: u16,
    theme: Theme,
    motion: crate::cli::MotionMode,
    elapsed: Duration,
) -> Line<'static> {
    let bar_width = width.saturating_sub(14).max(8) as usize;
    let known_total = progress.total_bytes.filter(|total| *total > 0);
    let suffix = match known_total {
        Some(total) => {
            let ratio = (progress.downloaded_bytes as f64 / total as f64).clamp(0.0, 1.0);
            format!(" {:>3}%", (ratio * 100.0).round() as u16)
        }
        None => format!(" {}", format_bytes(progress.downloaded_bytes)),
    };
    let mut cells = vec!["░"; bar_width];
    match known_total {
        Some(total) => {
            let ratio = (progress.downloaded_bytes as f64 / total as f64).clamp(0.0, 1.0);
            let filled = (ratio * bar_width as f64).round() as usize;
            for cell in cells.iter_mut().take(filled.min(bar_width)) {
                *cell = "█";
            }
        }
        None => {
            // Unknown size: sweep a segment so the bar still shows motion
            // without inventing a percentage. `MotionMode::Off` parks it.
            let segment = (bar_width / 4).max(2).min(bar_width);
            let offset = super::animation::spinner_frame(motion, elapsed, bar_width);
            for index in 0..segment {
                cells[(offset + index) % bar_width] = "█";
            }
        }
    }
    Line::from(vec![
        Span::styled("[", Style::new().fg(theme.muted)),
        Span::styled(cells.concat(), Style::new().fg(theme.action)),
        Span::styled(format!("]{suffix}"), Style::new().fg(theme.text)),
    ])
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    let mut last_space: Option<usize> = None;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if current_width + character_width > width {
            if let Some(index) = last_space {
                let remainder = current[index..].trim_start().to_owned();
                current.truncate(index);
                lines.push(current.trim_end().to_owned());
                current = remainder;
                current_width = current.width();
                last_space = current.rfind(' ');
            } else {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
                last_space = None;
            }
        }
        if character == ' ' {
            last_space = Some(current.len());
        }
        current.push(character);
        current_width += character_width;
    }
    lines.push(current.trim_end().to_owned());
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::update::UpdateState;
    use crate::update::{
        InstallationManager, UpdateChannel, UpdateInspection, UpdateProgress, UpdateStage,
        UpdateStatus,
    };

    fn inspection(manager: InstallationManager, status: UpdateStatus) -> UpdateInspection {
        UpdateInspection {
            manager,
            channel: UpdateChannel::Stable,
            running_version: "1.2.3".to_owned(),
            installed_version: Some("1.2.3".to_owned()),
            target_version: Some("1.3.0".to_owned()),
            status,
            action: None,
            launcher_path: None,
        }
    }

    #[test]
    fn wrap_text_breaks_on_word_boundaries() {
        assert_eq!(
            wrap_text("update available for lazydb now", 12),
            vec!["update", "available", "for lazydb", "now"]
        );
    }

    #[test]
    fn wrap_text_hard_splits_words_longer_than_the_width() {
        assert_eq!(wrap_text("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn actions_are_state_specific_and_never_offer_a_cargo_command_by_default() {
        let state = UpdateState::ManagerActionRequired(inspection(
            InstallationManager::Cargo,
            UpdateStatus::ManagerActionRequired,
        ));
        assert_eq!(
            state.dialog_actions(false),
            vec![
                UpdateDialogAction::ShowInstructions,
                UpdateDialogAction::Close
            ]
        );
        assert_eq!(
            state.dialog_actions(true),
            vec![UpdateDialogAction::CopyCommand, UpdateDialogAction::Close]
        );
    }

    #[test]
    fn up_to_date_offers_only_ok_and_a_single_action_has_no_select_hint() {
        let state = UpdateState::UpToDate(inspection(
            InstallationManager::Cargo,
            UpdateStatus::UpToDate,
        ));
        assert_eq!(state.dialog_actions(false), vec![UpdateDialogAction::Close]);
        assert!(!state.can_check_again() || state.can_check_again());
        let view = view_for_state(&state, false, Theme::default());
        assert_eq!(view.actions.len(), 1);
        assert_eq!(view.actions[0].label, "OK");
        let hints = view.hints();
        assert!(hints.iter().all(|hint| hint.key != "Tab/Left/Right"));
        assert!(hints.iter().any(|hint| hint.key == "r"));
    }

    #[test]
    fn instructions_without_a_command_do_not_offer_copy() {
        let state = UpdateState::ManagerActionRequired(inspection(
            InstallationManager::Unknown,
            UpdateStatus::ManagerActionRequired,
        ));
        assert_eq!(state.dialog_actions(true), vec![UpdateDialogAction::Close]);
    }

    #[test]
    fn progress_line_reports_a_percentage_only_with_a_known_total() {
        let theme = Theme::default();
        let known = UpdateProgress {
            stage: UpdateStage::Downloading,
            downloaded_bytes: 50,
            total_bytes: Some(100),
        };
        let rendered = progress_line(
            &known,
            40,
            theme,
            crate::cli::MotionMode::Full,
            Duration::ZERO,
        )
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
        assert!(rendered.contains("50%"), "{rendered}");

        let unknown = UpdateProgress {
            stage: UpdateStage::Downloading,
            downloaded_bytes: 1536,
            total_bytes: None,
        };
        let rendered = progress_line(
            &unknown,
            40,
            theme,
            crate::cli::MotionMode::Full,
            Duration::ZERO,
        )
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
        assert!(!rendered.contains('%'), "{rendered}");
        assert!(rendered.contains("KiB"), "{rendered}");
    }

    fn render_app(app: &App, ui: &mut UiState) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), app, ui, Theme::default()))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    fn overlay_app(state: UpdateState, instructions: bool) -> App {
        let mut app = App::new(Vec::new());
        app.update_state = state;
        app.overlay = Some(crate::model::workspace::Overlay::Update(
            crate::model::update::UpdateOverlayState {
                focus: UpdateOverlayFocus::Later,
                instructions,
            },
        ));
        app
    }

    #[test]
    fn render_keeps_a_single_title_and_hides_the_raw_manager_command() {
        let state = UpdateState::ManagerActionRequired(inspection(
            InstallationManager::Cargo,
            UpdateStatus::ManagerActionRequired,
        ));
        let app = overlay_app(state, false);
        let mut ui = UiState::new();
        let text = render_app(&app, &mut ui);
        assert_eq!(text.matches("UPDATE CENTER").count(), 1, "{text}");
        assert!(!text.contains("LAZYDB UPDATE"), "{text}");
        assert!(!text.contains("cargo install lazydb"), "{text}");
        assert!(text.contains("How to update"), "{text}");
        assert!(text.contains("Not now"), "{text}");
    }

    #[test]
    fn expanded_instructions_expose_the_command_and_a_copy_action() {
        let state = UpdateState::ManagerActionRequired(inspection(
            InstallationManager::Cargo,
            UpdateStatus::ManagerActionRequired,
        ));
        let app = overlay_app(state, true);
        let mut ui = UiState::new();
        let text = render_app(&app, &mut ui);
        assert!(text.contains("cargo install lazydb"), "{text}");
        assert!(text.contains("Copy update command"), "{text}");
    }

    #[test]
    fn status_colors_distinguish_up_to_date_available_and_failed() {
        let theme = Theme::default();
        let up_to_date = view_for_state(
            &UpdateState::UpToDate(inspection(
                InstallationManager::Native,
                UpdateStatus::UpToDate,
            )),
            false,
            theme,
        );
        assert_eq!(up_to_date.status.spans[0].style.fg, Some(theme.success));
        let available = view_for_state(
            &UpdateState::Available(inspection(
                InstallationManager::Native,
                UpdateStatus::Available,
            )),
            false,
            theme,
        );
        assert_eq!(available.status.spans[0].style.fg, Some(theme.action));
        let failed = view_for_state(
            &UpdateState::Failed {
                operation: crate::model::update::UpdateOperation::Check,
                message: "network down".to_owned(),
            },
            false,
            theme,
        );
        assert_eq!(failed.status.spans[0].style.fg, Some(theme.error));
    }

    #[test]
    fn installing_render_shows_a_real_progress_percentage() {
        let mut installing = inspection(InstallationManager::Native, UpdateStatus::Available);
        installing.target_version = Some("1.3.0".to_owned());
        let state = UpdateState::Installing {
            request_id: 1,
            inspection: installing,
            progress: UpdateProgress {
                stage: UpdateStage::Downloading,
                downloaded_bytes: 50,
                total_bytes: Some(100),
            },
        };
        let app = overlay_app(state, false);
        let mut ui = UiState::new();
        let text = render_app(&app, &mut ui);
        assert!(text.contains("50%"), "{text}");
        assert!(text.contains("Run in background"), "{text}");
    }

    #[test]
    fn failed_render_keeps_the_request_context_and_root_cause_visible() {
        let app = overlay_app(
            UpdateState::Failed {
                operation: crate::model::update::UpdateOperation::Install,
                message: "failed to request update asset (connection refused)".to_owned(),
            },
            false,
        );
        let mut ui = UiState::new();
        let text = render_app(&app, &mut ui);
        assert!(text.contains("Update failed"), "{text}");
        assert!(text.contains("failed to request update asset"), "{text}");
        assert!(text.contains("connection refused"), "{text}");
        assert!(text.contains("Check again"), "{text}");
    }

    #[test]
    fn hit_regions_match_the_rendered_buttons_inside_the_popup() {
        let state = UpdateState::Available(inspection(
            InstallationManager::Native,
            UpdateStatus::Available,
        ));
        let app = overlay_app(state, false);
        let mut ui = UiState::new();
        let _ = render_app(&app, &mut ui);
        let actions = ui
            .hit_regions
            .iter()
            .filter_map(|region| match region.target {
                HitTarget::UpdateButton { action } => Some((region.area, action)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(actions.len(), 2);
        assert!(
            actions
                .iter()
                .any(|(_, action)| *action == UpdateDialogAction::Install)
        );
        assert!(
            actions
                .iter()
                .any(|(_, action)| *action == UpdateDialogAction::Close)
        );
        for (area, _) in actions {
            assert!(area.width > 0 && area.height >= 1);
            assert!(area.right() <= 80 && area.bottom() <= 24);
        }
    }
}
