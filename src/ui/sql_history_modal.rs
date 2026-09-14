use chrono::{DateTime, Local};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::App,
    model::sql_history_view::SqlHistoryState,
    ui::{HitRegion, HitTarget, sql_preview, theme::Theme},
};

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    view: &SqlHistoryState,
    theme: Theme,
    state: &mut super::UiState,
) {
    let popup = super::centered(
        area,
        area.width.saturating_sub(4).min(150),
        area.height.saturating_sub(4).max(10),
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().bg(theme.surface_raised))
        .title(" SQL HISTORY ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width < 12 || inner.height < 5 {
        frame.render_widget(
            Paragraph::new("Terminal too small for SQL History")
                .style(Style::new().fg(theme.warning)),
            inner,
        );
        return;
    }

    let header_height = 1;
    let footer_height = 1;
    let content = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Min(1),
        Constraint::Length(footer_height),
    ])
    .split(inner);
    render_header(frame, content[0], view, theme);
    render_content(frame, content[1], app, view, theme, state);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ↑↓ ", Style::new().fg(theme.action)),
            Span::raw("select  "),
            Span::styled("Enter ", Style::new().fg(theme.action)),
            Span::raw("SQL  "),
            Span::styled("y ", Style::new().fg(theme.action)),
            Span::raw("copy  "),
            Span::styled("Esc ", Style::new().fg(theme.action)),
            Span::raw("close"),
        ]))
        .style(Style::new().fg(theme.muted)),
        content[2],
    );
}

fn render_header(frame: &mut Frame<'_>, area: Rect, view: &SqlHistoryState, theme: Theme) {
    let query = if view.search.value().is_empty() {
        "Search: all SQL"
    } else {
        view.search.value()
    };
    let status = view
        .status_filter
        .map_or("all", |value| format_status(value));
    let transaction = view
        .transaction_filter
        .map_or("all", |value| format_transaction(value));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {query} "),
                Style::new().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  status: {status}  transaction: {transaction}"),
                Style::new().fg(theme.muted),
            ),
        ])),
        area,
    );
}

fn render_content(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    view: &SqlHistoryState,
    theme: Theme,
    state: &mut super::UiState,
) {
    let split = if area.width >= 92 {
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).split(area)
    } else {
        Layout::vertical([Constraint::Percentage(48), Constraint::Percentage(52)]).split(area)
    };
    render_list(frame, split[0], view, theme, state);
    render_detail(frame, split[1], app, view, theme, state);
}

fn render_list(
    frame: &mut Frame<'_>,
    area: Rect,
    view: &SqlHistoryState,
    theme: Theme,
    state: &mut super::UiState,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.border))
        .title(" SQL ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut y = inner.y;
    for (index, item) in view.items.iter().enumerate().skip(view.list_offset) {
        if y >= inner.bottom() {
            break;
        }
        let selected = view.selected_execution == Some(item.execution_id);
        let preview_height = (inner.bottom().saturating_sub(y)).min(3);
        let lines = sql_preview::lines(
            &item.sql,
            crate::sql::SqlDialect::Generic,
            inner.width.saturating_sub(2) as usize,
            theme,
        );
        let shown = lines
            .into_iter()
            .take(preview_height as usize)
            .collect::<Vec<_>>();
        let metadata = Line::from(Span::styled(
            format!(
                "  {} · {} · {}",
                format_timestamp(item.requested_at),
                item.database.as_deref().unwrap_or("—"),
                format_status(item.status)
            ),
            Style::new().fg(theme.muted),
        ));
        let row_height = shown.len().max(1) as u16 + 1;
        if y.saturating_add(row_height) > inner.bottom() {
            break;
        }
        let row_area = Rect::new(inner.x, y, inner.width, row_height);
        state.hit_regions.push(HitRegion {
            area: row_area,
            target: HitTarget::SqlHistoryRow(index),
        });
        for (line_index, line) in shown.into_iter().enumerate() {
            frame.render_widget(
                Paragraph::new(line).style(Style::new().bg(if selected {
                    theme.selection
                } else {
                    theme.surface_raised
                })),
                Rect::new(inner.x, y.saturating_add(line_index as u16), inner.width, 1),
            );
        }
        frame.render_widget(
            Paragraph::new(metadata).style(Style::new().bg(if selected {
                theme.selection
            } else {
                theme.surface_raised
            })),
            Rect::new(inner.x, y.saturating_add(row_height - 1), inner.width, 1),
        );
        y = y.saturating_add(row_height);
    }
    if view.loading && view.items.is_empty() {
        frame.render_widget(Paragraph::new(" Loading…"), inner);
    } else if view.items.is_empty() && view.error.is_none() {
        frame.render_widget(Paragraph::new(" No SQL execution history"), inner);
    }
}

fn render_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    _app: &App,
    view: &SqlHistoryState,
    theme: Theme,
    _state: &mut super::UiState,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.border))
        .title(" DETAILS ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(item) = view.selected_item() else {
        frame.render_widget(Paragraph::new(" Select a SQL record"), inner);
        return;
    };
    let info_height = inner.height.min(7);
    let sections =
        Layout::vertical([Constraint::Length(info_height), Constraint::Min(1)]).split(inner);
    let info = vec![
        Line::from(format!(
            "Time       {}",
            format_timestamp(item.requested_at)
        )),
        Line::from(format!(
            "Database   {}",
            item.database.as_deref().unwrap_or("—")
        )),
        Line::from(format!(
            "Schema     {}",
            item.schema.as_deref().unwrap_or("—")
        )),
        Line::from(format!("Status     {}", format_status(item.status))),
        Line::from(format!(
            "Elapsed    {}",
            item.elapsed_millis
                .map_or("—".into(), |v| format!("{v} ms"))
        )),
        Line::from(format!(
            "Rows       {}",
            item.returned_rows.map_or("—".into(), |v| v.to_string())
        )),
        Line::from(format!(
            "Affected   {}",
            item.affected_rows.map_or("—".into(), |v| v.to_string())
        )),
    ];
    frame.render_widget(
        Paragraph::new(info).style(Style::new().fg(theme.text)),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "SQL is shown on Enter",
            Style::new().fg(theme.muted),
        )))
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::new().fg(theme.border)),
        ),
        sections[1],
    );
}

fn format_timestamp(millis: i64) -> String {
    DateTime::from_timestamp_millis(millis)
        .map(|value| {
            value
                .with_timezone(&Local)
                .format("%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "unknown time".into())
}

fn format_status(status: crate::model::sql_history::HistoryExecutionStatus) -> &'static str {
    use crate::model::sql_history::HistoryExecutionStatus::*;
    match status {
        Queued => "queued",
        Running => "running",
        Succeeded => "success",
        Failed => "failed",
        TimedOut => "timeout",
        Cancelled => "cancelled",
        Interrupted => "interrupted",
        NotExecuted => "not executed",
    }
}

fn format_transaction(
    outcome: crate::model::sql_history::HistoryTransactionOutcome,
) -> &'static str {
    use crate::model::sql_history::HistoryTransactionOutcome::*;
    match outcome {
        NotApplicable => "—",
        Pending => "pending",
        AutoCommitted => "auto-commit",
        Committed => "committed",
        RolledBack => "rolled back",
        RolledBackToSavepoint => "savepoint",
        Unknown => "unknown",
    }
}
