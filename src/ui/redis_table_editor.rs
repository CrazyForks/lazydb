use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    model::redis_table_editor::{
        RedisTableDeleteConfirmation, RedisTableDeleteFocus, RedisTableEditorState,
    },
    ui::{Theme, UiState},
};

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    editor: &RedisTableEditorState,
    state: &mut UiState,
    theme: Theme,
) {
    let popup = super::centered(
        area,
        80.min(area.width),
        (editor.fields.len() as u16 + 5).min(area.height),
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" REDIS VALUE ROW ")
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().bg(theme.surface));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let mut constraints = vec![Constraint::Length(1); editor.fields.len()];
    constraints.extend([Constraint::Min(1), Constraint::Length(1)]);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);
    for (index, field) in editor.fields.iter().enumerate() {
        let label = format!("{index:>2} ");
        let line = Line::from(format!("{label}{}", field.value()));
        frame.render_widget(
            Paragraph::new(line).style(Style::new().fg(if index == editor.focused {
                theme.accent
            } else {
                theme.text
            })),
            rows[index],
        );
    }
    if let Some(error) = &editor.error {
        frame.render_widget(
            Paragraph::new(error.as_str()).style(Style::new().fg(theme.error)),
            rows[editor.fields.len()],
        );
    }
    let hints = [
        crate::ui::shortcut_hints::ShortcutHint::with_keys(
            "Tab",
            "next field",
            [KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)],
        ),
        crate::ui::shortcut_hints::ShortcutHint::with_keys(
            "Shift+Tab",
            "previous field",
            [KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE)],
        ),
        crate::ui::shortcut_hints::ShortcutHint::with_keys(
            "Enter",
            "apply",
            [KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)],
        ),
        crate::ui::shortcut_hints::ShortcutHint::with_keys(
            "Esc",
            "cancel",
            [KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)],
        ),
    ];
    crate::ui::shortcut_hints::render_interactive(
        frame,
        rows[editor.fields.len() + 1],
        &hints,
        theme,
        theme.surface,
        ratatui::layout::Alignment::Left,
        state,
    );
}

pub fn render_delete_confirm(
    frame: &mut Frame<'_>,
    area: Rect,
    confirm: &RedisTableDeleteConfirmation,
    state: &mut UiState,
    theme: Theme,
) {
    let popup = super::centered(area, 76.min(area.width), 8.min(area.height));
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" DELETE REDIS VALUE ROW ")
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.error))
        .style(Style::new().bg(theme.surface));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let selected = confirm.focus;
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(confirm.title()),
            Line::from("Delete the selected row?"),
            Line::from(if selected == RedisTableDeleteFocus::Cancel {
                "[ > Cancel ]   [ Delete ]"
            } else {
                "[ Cancel ]   [ > Delete ]"
            }),
        ])
        .style(Style::new().fg(theme.text)),
        inner,
    );
    crate::ui::shortcut_hints::render_interactive(
        frame,
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
        &[
            crate::ui::shortcut_hints::ShortcutHint::with_keys(
                "Tab",
                "switch",
                [KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)],
            ),
            crate::ui::shortcut_hints::ShortcutHint::with_keys(
                "Enter",
                "confirm",
                [KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)],
            ),
            crate::ui::shortcut_hints::ShortcutHint::with_keys(
                "Esc",
                "cancel",
                [KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)],
            ),
        ],
        theme,
        theme.surface,
        ratatui::layout::Alignment::Left,
        state,
    );
}
