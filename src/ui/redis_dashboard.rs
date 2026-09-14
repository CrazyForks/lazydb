use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use super::{HitRegion, HitTarget, Theme, UiState, panel_block};
use crate::{app::App, model::tab::WorkspaceTab};

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
    state: &mut UiState,
) {
    let Some(WorkspaceTab::Dashboard(tab)) = app.tabs.get(app.active_tab) else {
        return;
    };
    let block = panel_block(" REDIS DASHBOARD ", true, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() {
        return;
    }
    let details = tab.redis_details.as_ref();
    if tab.page == crate::model::dashboard::DashboardPage::Info {
        render_info(frame, inner, theme, details, app.redis_info_scroll);
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Min(1),
        ])
        .split(inner);
    let identity = details
        .map(|details| {
            format!(
                "Redis {} · {} · {}",
                details.redis_version.as_deref().unwrap_or("--"),
                details.redis_mode.as_deref().unwrap_or("--"),
                details
                    .uptime_in_seconds
                    .map(format_uptime)
                    .unwrap_or_else(|| "uptime --".into())
            )
        })
        .unwrap_or_else(|| "Redis · waiting for INFO".into());
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(identity, theme.title(true)),
            Line::from(format!(
                "Instance scope · context db{}",
                tab.redis_database.map_or(0, |database| database)
            )),
        ]),
        rows[0],
    );
    let cards = [
        (
            "OPS/s",
            value(tab, crate::model::dashboard::MetricKey::RedisCommands),
            theme.accent,
        ),
        (
            "Clients",
            value(tab, crate::model::dashboard::MetricKey::Connections),
            theme.action,
        ),
        (
            "Memory",
            format_bytes(value_number(
                tab,
                crate::model::dashboard::MetricKey::RedisMemory,
            )),
            theme.warning,
        ),
        (
            "Keys",
            value(tab, crate::model::dashboard::MetricKey::RedisKeys),
            theme.success,
        ),
    ];
    render_cards(frame, rows[1], theme, &cards);
    let status = details
        .map(|details| {
            format!(
                "Port {} · PID {} · fragmentation {} · AOF {}",
                details
                    .tcp_port
                    .map_or_else(|| "--".into(), |value| value.to_string()),
                details
                    .process_id
                    .map_or_else(|| "--".into(), |value| value.to_string()),
                details
                    .mem_fragmentation_ratio
                    .map_or_else(|| "--".into(), |value| format!("{value:.2}")),
                if details.aof_enabled == Some(true) {
                    "enabled"
                } else {
                    "disabled"
                }
            )
        })
        .unwrap_or_else(|| "Server information unavailable".into());
    frame.render_widget(
        Paragraph::new(status).style(Style::new().fg(theme.muted)),
        rows[2],
    );
    super::dashboard::render_metric_chart(
        frame,
        rows[3],
        theme,
        tab,
        " Commands/s · last 10 minutes ",
        &[(
            crate::model::dashboard::MetricKey::RedisCommands,
            "ops/s",
            theme.accent,
        )],
    );
    let info = details
        .map(|details| {
            details
                .fields
                .iter()
                .flat_map(|(section, fields)| {
                    fields
                        .iter()
                        .map(move |(key, value)| format!("{section}.{key}: {value}"))
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| "Redis INFO is not available".into());
    state.hit_regions.push(HitRegion {
        area: rows[3],
        target: HitTarget::OpenTextDetail(super::readonly_detail_request("Redis INFO", info)),
    });
}

fn render_info(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: Theme,
    details: Option<&crate::db::redis::monitor::RedisMonitorDetails>,
    scroll: u16,
) {
    let Some(details) = details else {
        frame.render_widget(
            Paragraph::new("Waiting for Redis INFO…").style(Style::new().fg(theme.muted)),
            area,
        );
        return;
    };
    let mut lines = Vec::new();
    for (section, fields) in &details.fields {
        lines.push(Line::styled(format!("[{section}]"), theme.title(true)));
        for (key, value) in fields {
            lines.push(Line::from(vec![
                Span::styled(format!("{key:<34}"), Style::new().fg(theme.muted)),
                Span::raw(value.clone()),
            ]));
        }
        lines.push(Line::raw(""));
    }
    let max_scroll = lines.len().saturating_sub(area.height as usize) as u16;
    let scroll = scroll.min(max_scroll);
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), area);
}

fn render_cards(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: Theme,
    cards: &[(&str, String, ratatui::style::Color)],
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25); 4])
        .split(area);
    for (index, (label, value, color)) in cards.iter().enumerate() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(*label, Style::new().fg(theme.muted))),
                Line::raw(""),
                Line::styled(value, Style::new().fg(*color)),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::new().fg(theme.border)),
            ),
            columns[index],
        );
    }
}

fn value(
    tab: &crate::model::dashboard::DashboardTab,
    key: crate::model::dashboard::MetricKey,
) -> String {
    value_number(tab, key).map_or_else(|| "--".into(), format_number)
}
fn value_number(
    tab: &crate::model::dashboard::DashboardTab,
    key: crate::model::dashboard::MetricKey,
) -> Option<f64> {
    tab.latest
        .as_ref()
        .and_then(|sample| sample.values.get(&key))
        .copied()
}
fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        (value as u64).to_string()
    } else {
        format!("{value:.1}")
    }
}
fn format_bytes(value: Option<f64>) -> String {
    value.map_or_else(
        || "--".into(),
        |value| {
            let mut value = value;
            let units = ["B", "KiB", "MiB", "GiB"];
            let mut index = 0;
            while value >= 1024.0 && index < units.len() - 1 {
                value /= 1024.0;
                index += 1;
            }
            format!("{value:.1} {}", units[index])
        },
    )
}
fn format_uptime(seconds: u64) -> String {
    format!(
        "uptime {}d {:02}:{:02}:{:02}",
        seconds / 86400,
        (seconds / 3600) % 24,
        (seconds / 60) % 60,
        seconds % 60
    )
}
