use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthChar;

use crate::{
    app::App,
    model::{
        transaction::{TransactionMode, TransactionState},
        workspace::ExecutionConfirmFocus,
    },
    security::sanitize_terminal_text,
    sql::{ExecutionDraft, HighlightKind, SqlDialect, SqlRisk},
};

use super::{HitRegion, HitTarget, UiState, centered, dialog, panel_block, theme, theme::Theme};

#[derive(Clone, Debug, Eq, PartialEq)]
struct Summary {
    title: String,
    scope: String,
    reason: String,
    risks: String,
    transaction: String,
    warning: Option<String>,
}

fn summary(draft: &ExecutionDraft, app: &App) -> Summary {
    let scope = match draft.scope {
        crate::sql::ScopeKind::VisualChar => "Selected text",
        crate::sql::ScopeKind::VisualLine => "Selected lines",
        crate::sql::ScopeKind::VisualBlock => "Block selection",
        crate::sql::ScopeKind::CurrentStatement => "Current statement",
        crate::sql::ScopeKind::FullBuffer => "Entire editor (all SQL)",
    };
    let has_unknown = draft.risks.contains(&SqlRisk::Unknown);
    let mut counts = Vec::new();
    for (risk, label) in [
        (SqlRisk::ReadOnly, "read-only"),
        (SqlRisk::Dml, "data-changing or lock-acquiring"),
        (SqlRisk::Ddl, "schema changes"),
        (SqlRisk::TransactionControl, "transaction control"),
    ] {
        let count = draft
            .risks
            .iter()
            .filter(|candidate| **candidate == risk)
            .count();
        if count > 0 {
            counts.push(format!("{count} {label}"));
        }
    }
    if has_unknown {
        counts.push("effects could not be fully determined".into());
    }
    let transaction = match (draft.transaction_mode, draft.transaction_state) {
        (TransactionMode::Auto, TransactionState::Idle) => {
            "Auto-commit; data changes are not protected by a pending manual transaction".into()
        }
        (TransactionMode::Manual, TransactionState::Idle) => {
            "Manual transaction; execution will start a transaction".into()
        }
        (TransactionMode::Manual, TransactionState::Active) => {
            "Existing transaction; execution joins the current transaction".into()
        }
        _ => format!(
            "Transaction state: {:?}/{:?}",
            draft.transaction_mode, draft.transaction_state
        ),
    };
    let warning = (draft.dialect == SqlDialect::MySql
        && draft.transaction_mode == TransactionMode::Manual
        && draft.risks.contains(&SqlRisk::Ddl))
    .then(|| "MySQL DDL may implicitly commit before and after execution".into());
    let connection = app
        .profiles
        .iter()
        .find(|profile| profile.id == draft.target.profile_id)
        .map(|profile| profile.name.clone())
        .unwrap_or_else(|| "unavailable connection".into());
    let target = if draft.target.schema.as_deref() == Some(draft.target.database.as_str()) {
        format!(
            "Connection: {connection}   Database: {}",
            draft.target.database
        )
    } else {
        format!(
            "Connection: {connection}   Database: {}   Default schema: {}",
            draft.target.database,
            draft.target.schema.as_deref().unwrap_or("none")
        )
    };
    let title = if has_unknown || draft.statement_count == 0 {
        "Execute selected SQL?".into()
    } else {
        format!(
            "Execute {} SQL statement{}?",
            draft.statement_count,
            if draft.statement_count == 1 { "" } else { "s" }
        )
    };
    Summary {
        title,
        scope: format!("{target}\nScope: {scope}"),
        reason: "Review the selected SQL before execution.".into(),
        risks: if counts.is_empty() {
            "Effects could not be fully determined".into()
        } else {
            counts.join("; ")
        },
        transaction,
        warning,
    }
}

fn preview_lines(sql: &str, dialect: SqlDialect, width: usize, theme: Theme) -> Vec<Line<'static>> {
    // Normalize the display projection before highlighting so byte ranges use
    // the same text that is eventually rendered.
    let text = sanitize_terminal_text(sql)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ");
    let highlights = crate::sql::highlight_sql(&text, dialect);
    let width = width.max(1);
    let mut output = Vec::new();
    let mut source_offset = 0;
    for (source_line, raw) in text.split('\n').enumerate() {
        let mut chunk = String::new();
        let mut used = 0;
        let mut chunk_offset = source_offset;
        for ch in raw.chars() {
            let cw = ch.width().unwrap_or(0);
            if used > 0 && used + cw > width {
                output.push(styled_chunk(
                    &chunk,
                    source_line,
                    &highlights,
                    chunk_offset,
                    theme,
                    chunk_offset == source_offset,
                ));
                chunk_offset += chunk.len();
                chunk.clear();
                used = 0;
            }
            chunk.push(ch);
            used += cw;
        }
        output.push(styled_chunk(
            &chunk,
            source_line,
            &highlights,
            chunk_offset,
            theme,
            chunk_offset == source_offset,
        ));
        source_offset += raw.len() + 1;
    }
    output
}

fn styled_chunk(
    chunk: &str,
    source_line: usize,
    highlights: &[crate::sql::HighlightSpan],
    start: usize,
    theme: Theme,
    first_chunk: bool,
) -> Line<'static> {
    let mut spans = Vec::new();
    let mut offset = start;
    if first_chunk {
        spans.push(Span::styled(
            format!("{:>3} ", source_line + 1),
            Style::new().fg(theme.muted),
        ));
    } else {
        spans.push(Span::raw("    "));
    }
    for ch in chunk.chars() {
        let end = offset + ch.len_utf8();
        let kind = highlights
            .iter()
            .find(|span| span.range.start <= offset && span.range.end >= end)
            .map(|span| span.kind)
            .unwrap_or(HighlightKind::Plain);
        let color = match kind {
            HighlightKind::Keyword => theme::SyntaxColor::Keyword,
            HighlightKind::Identifier => theme::SyntaxColor::Identifier,
            HighlightKind::Relation => theme::SyntaxColor::Relation,
            HighlightKind::RelationAlias => theme::SyntaxColor::RelationAlias,
            HighlightKind::Column => theme::SyntaxColor::Column,
            HighlightKind::Function => theme::SyntaxColor::Function,
            HighlightKind::String => theme::SyntaxColor::String,
            HighlightKind::Number => theme::SyntaxColor::Number,
            HighlightKind::Comment => theme::SyntaxColor::Comment,
            HighlightKind::Operator => theme::SyntaxColor::Operator,
            HighlightKind::Punctuation => theme::SyntaxColor::Punctuation,
            HighlightKind::Parameter => theme::SyntaxColor::Parameter,
            HighlightKind::Plain => theme::SyntaxColor::Plain,
        };
        spans.push(Span::styled(
            ch.to_string(),
            Style::new().fg(theme.syntax_color(color)),
        ));
        offset = end;
    }
    Line::from(spans)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    draft: &ExecutionDraft,
    focus: ExecutionConfirmFocus,
    offset: usize,
    app: &App,
    state: &mut UiState,
    theme: Theme,
) {
    let popup = centered(
        area,
        92.min(area.width.saturating_sub(4)),
        area.height.saturating_sub(2).clamp(8, 32),
    );
    frame.render_widget(Clear, popup);
    let inner_width = popup.width.saturating_sub(4) as usize;
    let info = summary(draft, app);
    let mut summary_lines = vec![
        Line::from(Span::styled(info.title, theme.title(true))),
        Line::raw(info.reason),
    ];
    summary_lines.extend(info.scope.lines().map(Line::raw));
    summary_lines.push(Line::from(Span::styled(
        format!("Risk: {}", info.risks),
        Style::new().fg(theme.warning),
    )));
    summary_lines.push(Line::raw(format!("Transaction: {}", info.transaction)));
    if let Some(warning) = info.warning {
        summary_lines.push(Line::from(Span::styled(
            warning,
            Style::new().fg(theme.warning).add_modifier(Modifier::BOLD),
        )));
    }
    let preview = preview_lines(
        &draft.sql,
        draft.dialect,
        inner_width.saturating_sub(7),
        theme,
    );
    let inner = popup.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    let summary_height = (summary_lines.len() as u16).min(inner.height.saturating_sub(4));
    let sections = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(summary_height),
            Constraint::Min(1),
            Constraint::Length(2),
        ],
    )
    .split(inner);
    let available = sections[1].height as usize;
    let start = offset.min(preview.len().saturating_sub(available));
    let end = (start + available).min(preview.len());
    let shown = preview[start..end].to_vec();
    frame.render_widget(
        Paragraph::new(summary_lines).wrap(Wrap { trim: false }),
        sections[0],
    );
    let mut preview_lines = vec![Line::from(Span::styled(
        format!(
            "SQL preview  Lines {}-{} of {}",
            start + 1,
            end.max(start + 1),
            preview.len()
        ),
        Style::new().fg(theme.muted),
    ))];
    preview_lines.extend(shown);
    frame.render_widget(
        Paragraph::new(preview_lines).wrap(Wrap { trim: false }),
        sections[1],
    );
    let actions = dialog::render_actions(
        frame,
        Rect::new(sections[2].x, sections[2].y, sections[2].width, 1),
        &[
            dialog::DialogButton {
                label: "Cancel",
                tone: dialog::DialogTone::Normal,
                enabled: true,
            },
            dialog::DialogButton {
                label: "Execute",
                tone: dialog::DialogTone::Danger,
                enabled: true,
            },
        ],
        usize::from(focus == ExecutionConfirmFocus::Execute),
        theme,
    );
    for action in actions {
        state.hit_regions.push(HitRegion {
            area: action.area,
            target: if action.index == 0 {
                HitTarget::ExecutionCancel
            } else {
                HitTarget::ExecutionConfirm
            },
        });
    }
    dialog::render_hint(
        frame,
        Rect::new(
            sections[2].x,
            sections[2].y.saturating_add(1),
            sections[2].width,
            1,
        ),
        "Tab / Left / Right switch   Enter activate   Up / Down preview   Esc cancel",
        theme,
    );
    frame.render_widget(panel_block(" EXECUTION CONFIRMATION ", true, theme), popup);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_sanitizes_and_keeps_unicode_boundaries() {
        let lines = preview_lines(
            "SELECT '中文';\r\n-- \u{1b}[31m",
            SqlDialect::Sqlite,
            8,
            Theme::deep_space(),
        );
        assert!(!lines.is_empty());
    }
}
