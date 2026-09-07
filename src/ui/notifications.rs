use ratatui::layout::Position;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::App,
    model::notification::{
        HistorySearchPhase, Notification, NotificationDetailState, NotificationHistoryState,
        NotificationLevel,
    },
    model::text_detail::TextDetailRequest,
    security::sanitize_terminal_text,
    ui::{HitRegion, HitTarget, UiState, icons::IconSet, theme::Theme},
};

const MAX_CARDS: usize = 4;
const MIN_WIDTH: u16 = 24;
const MAX_WIDTH: u16 = 56;

pub(crate) fn render(
    frame: &mut Frame<'_>,
    viewport: Rect,
    app: &App,
    theme: Theme,
    state: &mut UiState,
    icons: IconSet,
) {
    if viewport.width < MIN_WIDTH || viewport.height < 4 {
        return;
    }

    let entries = app
        .notifications
        .live()
        .iter()
        .rev()
        .take(MAX_CARDS)
        .filter_map(|live| app.notifications.get(live.notification_id))
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return;
    }

    let width = card_width(viewport.width, &entries);
    let mut y = viewport.y.saturating_add(1);
    for notification in entries {
        if y >= viewport.bottom() {
            break;
        }
        let height = card_height(notification, width);
        let height = height.min(viewport.bottom().saturating_sub(y));
        if height == 0 {
            break;
        }
        let area = Rect::new(viewport.right().saturating_sub(width), y, width, height);
        let close_area = draw_card(frame, area, notification, theme, icons);
        state.hit_regions.push(HitRegion {
            area,
            target: HitTarget::OpenNotificationHistoryAt(notification.id),
        });
        if let Some(area) = close_area {
            state.hit_regions.push(HitRegion {
                area,
                target: HitTarget::DismissNotification(notification.id),
            });
        }
        y = y.saturating_add(height).saturating_add(1);
    }
}

pub(crate) fn render_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    detail: &NotificationDetailState,
    theme: Theme,
) {
    let popup = centered_history(area, area.width < 72);
    frame.render_widget(ratatui::widgets::Clear, popup);
    let block = Block::default()
        .title(" NOTIFICATION DETAIL ")
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.border))
        .style(Style::new().bg(theme.surface_raised));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let Some(notification) = app.notifications.get(detail.notification_id) else {
        return;
    };
    if inner.height < 2 {
        return;
    }
    let mut lines = vec![
        Line::from(Span::styled(&notification.title, theme.title(true))),
        Line::raw(format!(
            "{}  {}  source: {}",
            notification.level,
            notification.created_at.format("%Y-%m-%d %H:%M:%S"),
            notification
                .source
                .map_or("unknown".to_owned(), |source| source.to_string())
        )),
        Line::raw(""),
    ];
    lines.extend(
        notification
            .body
            .lines()
            .map(|line| Line::raw(sanitize_terminal_text(line))),
    );
    let footer = "j/k scroll  y copy  Esc/q back";
    let content_height = usize::from(inner.height.saturating_sub(1));
    let visible = lines
        .into_iter()
        .skip(detail.scroll)
        .take(content_height)
        .collect::<Vec<_>>();
    let content = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(visible).wrap(Wrap { trim: false }),
        content[0],
    );
    frame.render_widget(
        Paragraph::new(footer).style(Style::new().fg(theme.muted)),
        content[1],
    );
}

pub(crate) fn render_history(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    history: &NotificationHistoryState,
    theme: Theme,
    state: &mut UiState,
) {
    let narrow = area.width < 72;
    let popup = centered_history(area, narrow);
    frame.render_widget(ratatui::widgets::Clear, popup);
    let block = Block::default()
        .title(" NOTIFICATION HISTORY ")
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.border))
        .style(Style::new().bg(theme.surface_raised));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.is_empty() {
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let body = if narrow {
        vec![chunks[1]]
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
            .split(chunks[1])
            .to_vec()
    };
    let entries = app.notifications.history().cloned().collect::<Vec<_>>();
    let matches = history.matching_indices(&entries).collect::<Vec<_>>();
    let search = if history.phase == HistorySearchPhase::Editing {
        "/"
    } else {
        ""
    };
    frame.render_widget(
        Paragraph::new(format!("{search}{}", history.query.value()))
            .style(Style::new().fg(theme.action)),
        chunks[0],
    );
    let selected = history
        .selected_id
        .and_then(|id| {
            entries
                .iter()
                .position(|notification| notification.id == id)
        })
        .unwrap_or_else(|| history.selected.min(entries.len().saturating_sub(1)));
    let visible_height = usize::from(body[0].height);
    let mut start = history
        .list_offset
        .min(entries.len().saturating_sub(visible_height));
    if visible_height > 0 {
        if selected < start {
            start = selected;
        } else if selected >= start + visible_height {
            start = selected.saturating_sub(visible_height - 1);
        }
    }
    let mut lines = Vec::new();
    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "No notifications",
            Style::new().fg(theme.muted),
        )));
    } else {
        for (index, notification) in entries.iter().enumerate().skip(start).take(visible_height) {
            let marker = if index == selected { ">" } else { " " };
            let style = if index == selected {
                Style::new().fg(theme.text).bg(theme.selection)
            } else {
                Style::new().fg(theme.text)
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "{marker} {}  {:<7} {}",
                    notification.created_at.format("%H:%M:%S"),
                    notification.level,
                    sanitize_terminal_text(&notification.title)
                ),
                style,
            )));
        }
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), body[0]);
    if narrow && let Some(notification) = entries.get(selected) {
        state.hit_regions.push(HitRegion {
            area: body[0],
            target: HitTarget::OpenTextDetail(notification_detail_request(notification)),
        });
    }
    if !entries.is_empty() {
        for row in 0..visible_height.min(entries.len().saturating_sub(start)) {
            state.hit_regions.push(HitRegion {
                area: Rect::new(body[0].x, body[0].y + row as u16, body[0].width, 1),
                target: HitTarget::NotificationHistoryRow(start + row),
            });
        }
    }
    if !narrow && let Some(notification) = entries.get(selected) {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(&notification.title, theme.title(true))),
                Line::raw(format!(
                    "{}  {}",
                    notification.level,
                    notification.created_at.format("%Y-%m-%d %H:%M:%S")
                )),
                Line::raw(format!(
                    "source: {}",
                    notification
                        .source
                        .map_or("unknown".to_owned(), |source| source.to_string())
                )),
                Line::raw(""),
                Line::raw(sanitize_terminal_text(&notification.body)),
            ])
            .wrap(Wrap { trim: true }),
            body[1],
        );
    }
    let footer = if history.clear_confirm {
        "Clear all notifications? y confirm  n/Esc cancel"
    } else {
        "j/k select  / search  n/N next/previous  c clear  Esc/q close"
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::new().fg(if history.clear_confirm {
            theme.warning
        } else {
            theme.muted
        })),
        chunks[2],
    );
    if history.phase == HistorySearchPhase::Editing {
        let x = chunks[0]
            .x
            .saturating_add(history.query.value().width() as u16)
            .min(chunks[0].right().saturating_sub(1));
        state.cursor = Some(crate::ui::CursorSpec {
            position: Position::new(x, chunks[0].y),
            style: crate::ui::CursorStyle::Bar,
        });
    }
    let _ = matches;
}

fn notification_detail_request(notification: &Notification) -> TextDetailRequest {
    let body = sanitize_terminal_text(&notification.body);
    TextDetailRequest::new(
        notification.title.clone(),
        uuid::Uuid::nil(),
        0,
        body.clone(),
        body,
        None,
    )
}

fn centered_history(area: Rect, narrow: bool) -> Rect {
    let width = if narrow {
        area.width.saturating_sub(2)
    } else {
        100.min(area.width.saturating_sub(4))
    };
    let height = 18.min(area.height.saturating_sub(2)).max(6);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn card_width(viewport_width: u16, entries: &[&Notification]) -> u16 {
    if viewport_width == 0 {
        return 0;
    }
    let wanted = entries
        .iter()
        .map(|notification| notification.title.width().max(notification.body.width()) as u16 + 8)
        .max()
        .unwrap_or(MIN_WIDTH);
    wanted
        .min(MAX_WIDTH)
        .min(viewport_width)
        .max(MIN_WIDTH.min(viewport_width))
}

fn card_height(notification: &Notification, width: u16) -> u16 {
    let content_width = usize::from(width.saturating_sub(2));
    let body_lines = wrapped_line_count(&sanitize_terminal_text(&notification.body), content_width);
    (body_lines + 4).clamp(4, 8) as u16
}

fn draw_card(
    frame: &mut Frame<'_>,
    area: Rect,
    notification: &Notification,
    theme: Theme,
    icons: IconSet,
) -> Option<Rect> {
    let color = level_color(notification.level, theme);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(color))
        .style(Style::new().bg(theme.surface_raised));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() {
        return None;
    }
    let icon = icons.notification(notification.level);
    let title = sanitize_terminal_text(&notification.title);
    let body = sanitize_terminal_text(&notification.body);
    let time = notification.created_at.format("%H:%M:%S").to_string();
    let close = icons.close();
    let close_width = (close.width().max(1) as u16 + 2).min(inner.width);
    let close_area = Rect::new(inner.right() - close_width, inner.y, close_width, 1);
    let header = Rect::new(inner.x, inner.y, inner.width - close_width, 1);
    let level = notification.level.to_string().to_uppercase();
    let title_width = usize::from(header.width).saturating_sub(icon.width() + level.width() + 2);
    let title = truncate_cells(&title, title_width);
    let line = Line::from(vec![
        Span::styled(
            format!("{icon} "),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{level} "),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            title,
            Style::new().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), header);
    frame.render_widget(
        Paragraph::new(format!(" {close} ")).style(Style::new().fg(theme.muted)),
        close_area,
    );
    let text = vec![
        Line::from(Span::styled(body, Style::new().fg(theme.text))),
        Line::from(Span::styled(time, Style::new().fg(theme.muted))),
    ];
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }),
        Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1),
    );
    Some(close_area)
}

fn level_color(level: NotificationLevel, theme: Theme) -> ratatui::style::Color {
    match level {
        NotificationLevel::Info => theme.action,
        NotificationLevel::Success => theme.accent,
        NotificationLevel::Warning => theme.warning,
        NotificationLevel::Error => theme.error,
    }
}

fn wrapped_line_count(value: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    value
        .lines()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum::<usize>()
        .max(1)
}

fn truncate_cells(value: &str, width: usize) -> String {
    let mut used = 0;
    value
        .chars()
        .take_while(|character| {
            let next = character.width().unwrap_or(0);
            if used + next > width {
                false
            } else {
                used += next;
                true
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::CellWidth;
    use std::time::Instant;

    #[test]
    fn visible_close_button_dismisses_only_its_notification() {
        use crate::{action::Action, input::mouse::map_mouse, ui::icons::IconMode};
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        for mode in [IconMode::Ascii, IconMode::Unicode, IconMode::NerdFont] {
            for width in [24, 80] {
                for title in [
                    "Query",
                    "Very long notification title that must be truncated",
                    "查询消息",
                ] {
                    let icons = IconSet::new(mode);
                    let mut app = App::new(Vec::new());
                    let older = app.notifications.push(
                        NotificationLevel::Info,
                        "Older",
                        "body",
                        Instant::now(),
                    );
                    let id = app.notifications.push(
                        NotificationLevel::Warning,
                        title,
                        "No SQL scope at cursor AUTO",
                        Instant::now(),
                    );
                    let mut ui = UiState::new();
                    let mut terminal =
                        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 24))
                            .unwrap();
                    terminal
                        .draw(|frame| {
                            render(frame, frame.area(), &app, Theme::default(), &mut ui, icons)
                        })
                        .unwrap();
                    let buffer = terminal.backend().buffer();
                    let x = (0..width)
                        .find(|&x| buffer[(x, 2)].symbol() == icons.close())
                        .expect("visible close icon on header row");
                    assert_eq!(x, width - 2 - icons.close().width() as u16);
                    let action = map_mouse(
                        MouseEvent {
                            kind: MouseEventKind::Down(MouseButton::Left),
                            column: x,
                            row: 2,
                            modifiers: KeyModifiers::NONE,
                        },
                        &ui,
                        &app,
                    )
                    .unwrap();
                    assert_eq!(action, Action::DismissNotification(id));
                    app.update(action);
                    assert!(app.overlay.is_none());
                    assert!(app.notifications.get(id).is_some());
                    assert_eq!(app.notifications.live().len(), 1);
                    assert_eq!(app.notifications.live()[0].notification_id, older);
                }
            }
        }
    }

    #[test]
    fn clicking_card_opens_history_at_its_stable_id() {
        use crate::{action::Action, input::mouse::map_mouse, model::workspace::Overlay};
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let mut app = App::new(Vec::new());
        let id =
            app.notifications
                .push(NotificationLevel::Info, "Selected", "body", Instant::now());
        app.notifications
            .push(NotificationLevel::Info, "Newer", "body", Instant::now());
        let mut ui = UiState::new();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    &app,
                    Theme::default(),
                    &mut ui,
                    IconSet::default(),
                )
            })
            .unwrap();
        let area = ui
            .hit_regions
            .iter()
            .find(|region| region.target == HitTarget::OpenNotificationHistoryAt(id))
            .unwrap()
            .area;
        // A new arrival must not change which notification the old hit region selects.
        app.notifications
            .push(NotificationLevel::Info, "Newest", "body", Instant::now());
        for (column, row) in [
            (area.x, area.y),
            (area.right() - 2, area.y + 2),
            (area.right() - 1, area.bottom() - 1),
        ] {
            let action = map_mouse(
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row,
                    modifiers: KeyModifiers::NONE,
                },
                &ui,
                &app,
            )
            .unwrap();
            assert_eq!(action, Action::OpenNotificationHistoryAt(id));
            app.update(action);
            let Some(Overlay::NotificationHistory(history)) = &app.overlay else {
                panic!("expected notification history")
            };
            assert_eq!(history.selected_id, Some(id));
            assert_eq!(history.selected, 2);
        }
    }

    #[test]
    fn width_and_text_helpers_use_display_cells() {
        assert_eq!("界".cell_width(), 2);
        assert_eq!(truncate_cells("界abc", 3), "界a");
        assert_eq!(wrapped_line_count("界界a", 4), 2);
    }

    #[test]
    fn card_width_is_clamped_for_small_and_large_terminals() {
        let notification = Notification {
            id: 1,
            level: NotificationLevel::Info,
            title: "title".into(),
            body: "body".into(),
            created_at: chrono::Local::now(),
            source: None,
        };
        assert_eq!(card_width(10, &[&notification]), 10);
        assert_eq!(card_width(100, &[&notification]), MIN_WIDTH);
    }

    #[test]
    fn history_popup_falls_back_to_full_width_on_narrow_terminals() {
        let popup = centered_history(Rect::new(0, 0, 60, 24), true);
        assert_eq!(popup.width, 58);
        assert!(popup.height <= 22);
    }

    #[test]
    fn empty_history_renders_an_empty_state_without_interaction_targets() {
        let app = App::new(Vec::new());
        let history = NotificationHistoryState::new();
        let mut ui = UiState::new();
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24))
            .expect("test terminal");

        terminal
            .draw(|frame| {
                render_history(
                    frame,
                    frame.area(),
                    &app,
                    &history,
                    Theme::default(),
                    &mut ui,
                );
            })
            .expect("render history");

        let buffer = terminal.backend().buffer();
        assert!(buffer.content().iter().any(|cell| cell.symbol() == "N"));
        assert!(ui.hit_regions.is_empty());
    }

    #[test]
    fn notification_detail_request_copies_complete_sanitized_body() {
        let body = "first\nsecond\u{1b}[31m";
        let request = notification_detail_request(&Notification {
            id: 1,
            level: NotificationLevel::Error,
            title: "Error".into(),
            body: body.into(),
            created_at: chrono::Local::now(),
            source: None,
        });

        assert_eq!(request.display_text, sanitize_terminal_text(body));
        assert_eq!(request.copy_text, sanitize_terminal_text(body));
        assert_eq!(request.source_session_id, uuid::Uuid::nil());
        assert_eq!(request.source_revision, 0);
    }

    #[test]
    fn narrow_history_registers_an_explicit_detail_target_for_selected_row() {
        let mut app = App::new(Vec::new());
        app.notifications.push(
            NotificationLevel::Info,
            "Title",
            "Complete body",
            Instant::now(),
        );
        let history = NotificationHistoryState::new();
        let mut ui = UiState::new();
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 20))
            .expect("test terminal");
        terminal
            .draw(|frame| {
                render_history(
                    frame,
                    frame.area(),
                    &app,
                    &history,
                    Theme::default(),
                    &mut ui,
                );
            })
            .expect("render history");

        assert!(
            ui.hit_regions
                .iter()
                .any(|region| { matches!(region.target, HitTarget::OpenTextDetail(_)) })
        );
    }
}
