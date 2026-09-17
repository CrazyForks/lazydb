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
    _state: &mut UiState,
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
    frame.render_widget(
        Paragraph::new("Tab/↑↓ focus · Enter apply · Esc cancel")
            .style(Style::new().fg(theme.muted)),
        rows[editor.fields.len() + 1],
    );
}

pub fn render_delete_confirm(
    frame: &mut Frame<'_>,
    area: Rect,
    confirm: &RedisTableDeleteConfirmation,
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
    let selected = match confirm.focus {
        RedisTableDeleteFocus::Cancel => "Cancel",
        RedisTableDeleteFocus::Delete => "Delete",
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(confirm.title()),
            Line::from("Delete the selected row?"),
            Line::from(format!("{selected} · Enter confirm · Esc cancel")),
        ])
        .style(Style::new().fg(theme.text)),
        inner,
    );
}
