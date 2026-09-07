use crate::{
    model::data_query::{DataQueryCapability, DataQueryInput, DataQueryState},
    sql::{HighlightKind, HighlightSpan, SqlClauseKind, SqlDialect},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    HitRegion, HitTarget, UiState,
    icons::IconSet,
    register_data_query_input, text_input_horizontal_offset,
    theme::{self, Theme},
};

const FIELD_HEIGHT: u16 = 2;
const HORIZONTAL_MIN_WIDTH: u16 = 56;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct QueryBarHighlightCache {
    where_entry: Option<QueryBarHighlightEntry>,
    order_by_entry: Option<QueryBarHighlightEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct QueryBarHighlightEntry {
    text: String,
    dialect: SqlDialect,
    spans: Vec<HighlightSpan>,
}

impl QueryBarHighlightCache {
    fn highlights(
        &mut self,
        input: DataQueryInput,
        text: &str,
        dialect: SqlDialect,
    ) -> &[HighlightSpan] {
        let entry = match input {
            DataQueryInput::Where => &mut self.where_entry,
            DataQueryInput::OrderBy => &mut self.order_by_entry,
        };
        let clause = match input {
            DataQueryInput::Where => SqlClauseKind::Where,
            DataQueryInput::OrderBy => SqlClauseKind::OrderBy,
        };
        if entry
            .as_ref()
            .is_none_or(|cached| cached.text != text || cached.dialect != dialect)
        {
            *entry = Some(QueryBarHighlightEntry {
                text: text.to_owned(),
                dialect,
                spans: crate::sql::highlight_sql_clause(text, clause, dialect),
            });
        }
        &entry
            .as_ref()
            .expect("query bar cache entry is initialized")
            .spans
    }
}

fn fields_height(width: u16) -> u16 {
    if width >= HORIZONTAL_MIN_WIDTH {
        FIELD_HEIGHT
    } else {
        FIELD_HEIGHT * 2
    }
}

pub(crate) fn height(query: &DataQueryState, width: u16, _icons: IconSet) -> u16 {
    fields_height(width)
        + u16::from(
            query.error.is_some()
                || matches!(query.capability, DataQueryCapability::Unavailable(_)),
        )
}

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    query: &DataQueryState,
    theme: Theme,
    state: &mut UiState,
    icons: IconSet,
    dialect: SqlDialect,
) -> Option<Position> {
    if area.height == 0 {
        return None;
    }
    let horizontal = area.width >= HORIZONTAL_MIN_WIDTH;
    let fields_height = fields_height(area.width);
    let fields_area = Rect::new(area.x, area.y, area.width, area.height.min(fields_height));
    let chunks = if horizontal {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(fields_area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(FIELD_HEIGHT),
                Constraint::Length(FIELD_HEIGHT),
            ])
            .split(fields_area)
    };
    let enabled = matches!(
        query.capability,
        DataQueryCapability::Relation | DataQueryCapability::Sql
    );
    let fields = [
        (DataQueryInput::Where, "WHERE", query.where_input.value()),
        (
            DataQueryInput::OrderBy,
            "ORDER BY",
            query.order_by_input.value(),
        ),
    ];
    let mut cursor = None;
    for ((input, label, value), chunk) in fields.into_iter().zip(chunks.iter().copied()) {
        let active = enabled && query.focus == Some(input);
        let icon = match input {
            DataQueryInput::Where => icons.query_filter(),
            DataQueryInput::OrderBy => icons.query_sort(),
        };
        let field = Rect::new(chunk.x, chunk.y, chunk.width, 1);
        let underline = Rect::new(chunk.x, chunk.y.saturating_add(1), chunk.width, 1);
        let label = format!("{icon} {label}");
        let text_input = match input {
            DataQueryInput::Where => &query.where_input,
            DataQueryInput::OrderBy => &query.order_by_input,
        };
        let prefix = format!("{label}  ");
        let spans = state
            .query_bar_highlights
            .highlights(input, value, dialect)
            .to_vec();
        let offset = if active {
            text_input_horizontal_offset(field, &prefix, text_input)
        } else {
            0
        };
        if enabled {
            register_data_query_input(state, input, field, &prefix, text_input, offset);
        }
        cursor = render_query_field(
            frame, field, &label, text_input, &spans, enabled, active, offset, theme, state,
        )
        .or(cursor);
        frame.render_widget(
            Paragraph::new(icons.query_underline().repeat(usize::from(chunk.width)))
                .style(Style::new().fg(if active { theme.accent } else { theme.border })),
            underline,
        );
        if enabled {
            state.hit_regions.push(HitRegion {
                area: chunk,
                target: HitTarget::DataQueryInput(input),
            });
        }
    }
    let message = query.error.as_ref().or(match &query.capability {
        DataQueryCapability::Unavailable(reason) => Some(reason),
        DataQueryCapability::Relation
        | DataQueryCapability::Sql
        | DataQueryCapability::AwaitingResult => None,
    });
    if let Some(error) = message {
        let error_y = area.y.saturating_add(fields_height);
        if error_y >= area.bottom() {
            return cursor;
        }
        frame.render_widget(
            Paragraph::new(crate::security::sanitize_terminal_text(error))
                .style(Style::new().fg(theme.warning)),
            Rect::new(area.x, error_y, area.width, 1),
        );
    }
    cursor
}

#[allow(clippy::too_many_arguments)]
fn render_query_field(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    input: &crate::model::text_input::TextInput,
    highlights: &[HighlightSpan],
    enabled: bool,
    active: bool,
    offset: usize,
    theme: Theme,
    state: &mut UiState,
) -> Option<Position> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let projection = crate::security::project_editor_line(input.value());
    let prefix = format!("{label}  ");
    let prefix_width = prefix.width();
    let available = usize::from(area.width).saturating_sub(prefix_width);
    let cursor_cells = projection
        .source_to_display_cells
        .get(input.cursor())
        .copied()
        .unwrap_or_else(|| projection.text.width());
    let mut spans = Vec::new();
    let keyword_start = label.find(char::is_alphabetic).unwrap_or(0);
    let (icon, keyword) = label.split_at(keyword_start);
    spans.push(Span::styled(
        icon.to_owned(),
        Style::new()
            .fg(if !enabled {
                theme.muted
            } else if active {
                theme.accent
            } else {
                theme.muted
            })
            .bg(theme.surface),
    ));
    spans.push(Span::styled(
        keyword.to_owned(),
        Style::new()
            .fg(if enabled {
                theme.syntax_color(theme::SyntaxColor::Keyword)
            } else {
                theme.muted
            })
            .bg(theme.surface),
    ));
    spans.push(Span::styled("  ", Style::new().bg(theme.surface)));

    let mut display_cell = 0usize;
    let mut source_index = 0usize;
    for character in projection.text.chars() {
        let width = character.width().unwrap_or(0);
        let end = display_cell.saturating_add(width);
        while source_index + 1 < projection.source_to_display_cells.len()
            && projection.source_to_display_cells[source_index + 1] <= display_cell
        {
            source_index += 1;
        }
        let source_start = input
            .value()
            .char_indices()
            .nth(source_index)
            .map_or(0, |(start, _)| start);
        let source_end = input
            .value()
            .char_indices()
            .nth(source_index + 1)
            .map_or(input.value().len(), |(start, _)| start);
        let kind = highlights
            .iter()
            .find(|span| span.range.start <= source_start && span.range.end >= source_end)
            .map(|span| span.kind)
            .unwrap_or(HighlightKind::Plain);
        if end > offset && display_cell < offset.saturating_add(available) {
            let style = Style::new()
                .fg(if enabled {
                    theme.syntax_color(theme::syntax_color_for_highlight(kind))
                } else {
                    theme.muted
                })
                .bg(theme.surface);
            let style = if input
                .selection_range()
                .is_some_and(|range| range.contains(&source_index))
            {
                style.add_modifier(ratatui::style::Modifier::REVERSED)
            } else {
                style
            };
            if let Some(previous) = spans.last_mut()
                && previous.style == style
            {
                previous.content.to_mut().push(character);
            } else {
                spans.push(Span::styled(character.to_string(), style));
            }
        }
        display_cell = end;
        if display_cell >= offset.saturating_add(available) {
            break;
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(theme.surface)),
        area,
    );
    if !active {
        return None;
    }
    let cursor_x = area
        .x
        .saturating_add(prefix_width as u16)
        .saturating_add(cursor_cells.saturating_sub(offset) as u16)
        .min(area.right().saturating_sub(1));
    let cursor = Position::new(cursor_x, area.y);
    state.cursor = Some(super::CursorSpec {
        position: cursor,
        style: super::CursorStyle::Bar,
    });
    Some(cursor)
}

#[cfg(test)]
mod tests {
    use super::{
        FIELD_HEIGHT, QueryBarHighlightCache, fields_height, render_query_field, theme::Theme,
    };
    use crate::model::data_query::DataQueryInput;
    use crate::model::text_input::TextInput;
    use crate::sql::{HighlightKind, HighlightSpan, SqlDialect, TextRange};
    use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Modifier};

    #[test]
    fn switches_to_horizontal_layout_at_the_minimum_usable_width() {
        assert_eq!(fields_height(55), FIELD_HEIGHT * 2);
        assert_eq!(fields_height(56), FIELD_HEIGHT);
    }

    #[test]
    fn query_bar_cache_invalidates_text_and_dialect() {
        let mut cache = QueryBarHighlightCache::default();
        let first = cache
            .highlights(DataQueryInput::Where, "name = 1", SqlDialect::Sqlite)
            .to_vec();
        assert!(first.iter().any(|span| span.kind == HighlightKind::Number));

        let same = cache
            .highlights(DataQueryInput::Where, "name = 1", SqlDialect::Sqlite)
            .to_vec();
        assert_eq!(same, first);

        let changed = cache
            .highlights(DataQueryInput::Where, "name = 'Ada'", SqlDialect::Sqlite)
            .to_vec();
        assert!(
            changed
                .iter()
                .any(|span| span.kind == HighlightKind::String)
        );

        let order = cache
            .highlights(DataQueryInput::OrderBy, "name DESC", SqlDialect::Sqlite)
            .to_vec();
        assert!(order.iter().any(|span| span.kind == HighlightKind::Keyword));
    }

    #[test]
    fn query_bar_selection_reverses_cells_without_replacing_syntax_color() {
        let mut input = TextInput::from("name = 1");
        input.begin_selection(7);
        input.extend_selection(8);
        let mut state = super::super::UiState::new();
        let mut terminal = Terminal::new(TestBackend::new(24, 1)).unwrap();

        terminal
            .draw(|frame| {
                render_query_field(
                    frame,
                    Rect::new(0, 0, 24, 1),
                    "WHERE",
                    &input,
                    &[HighlightSpan {
                        range: TextRange::new(7, 8),
                        kind: HighlightKind::Number,
                    }],
                    true,
                    true,
                    0,
                    Theme::default(),
                    &mut state,
                );
            })
            .unwrap();

        let cell = terminal.backend().buffer().cell((14, 0)).unwrap();
        assert!(cell.modifier.contains(Modifier::REVERSED));
        assert_eq!(
            cell.fg,
            Theme::default().syntax_color(super::theme::SyntaxColor::Number)
        );
    }
}
