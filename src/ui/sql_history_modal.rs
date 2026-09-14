use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{app::App, model::sql_history_view::SqlHistoryState, ui::theme::Theme};

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    _app: &App,
    view: &SqlHistoryState,
    theme: Theme,
    _state: &mut super::UiState,
) {
    let popup = super::centered(
        area,
        area.width.saturating_sub(4).min(120),
        area.height.saturating_sub(4).max(8),
    );
    frame.render_widget(Clear, popup);
    let body = if view.loading {
        "Loading SQL history…"
    } else if let Some(error) = &view.error {
        error.as_str()
    } else if view.items.is_empty() {
        "No SQL execution history"
    } else {
        "SQL history loaded"
    };
    frame.render_widget(
        Paragraph::new(body)
            .style(Style::new().fg(theme.text).bg(theme.surface_raised))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::new().fg(theme.accent))
                    .title(" SQL History "),
            ),
        popup,
    );
}
