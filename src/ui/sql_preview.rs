use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthChar;

use crate::{
    security::sanitize_terminal_text,
    sql::{HighlightKind, HighlightSpan, SqlDialect},
};

use super::theme::{self, Theme};

pub(crate) fn lines(
    sql: &str,
    dialect: SqlDialect,
    width: usize,
    theme: Theme,
) -> Vec<Line<'static>> {
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
            let char_width = ch.width().unwrap_or(0);
            if used > 0 && used + char_width > width {
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
            used += char_width;
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
    highlights: &[HighlightSpan],
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
        spans.push(Span::styled(
            ch.to_string(),
            Style::new().fg(theme.syntax_color(syntax_color(kind))),
        ));
        offset = end;
    }
    Line::from(spans)
}

fn syntax_color(kind: HighlightKind) -> theme::SyntaxColor {
    match kind {
        HighlightKind::Keyword => theme::SyntaxColor::Keyword,
        HighlightKind::Identifier => theme::SyntaxColor::Identifier,
        HighlightKind::Relation => theme::SyntaxColor::Relation,
        HighlightKind::RelationAlias => theme::SyntaxColor::RelationAlias,
        HighlightKind::Column => theme::SyntaxColor::Column,
        HighlightKind::Type => theme::SyntaxColor::Type,
        HighlightKind::Function => theme::SyntaxColor::Function,
        HighlightKind::String => theme::SyntaxColor::String,
        HighlightKind::Number => theme::SyntaxColor::Number,
        HighlightKind::Comment => theme::SyntaxColor::Comment,
        HighlightKind::Operator => theme::SyntaxColor::Operator,
        HighlightKind::Punctuation => theme::SyntaxColor::Punctuation,
        HighlightKind::Parameter => theme::SyntaxColor::Parameter,
        HighlightKind::Plain => theme::SyntaxColor::Plain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_and_highlights_multibyte_sql() {
        let lines = lines(
            "SELECT '中文'\r\nFROM users",
            SqlDialect::Sqlite,
            32,
            Theme::deep_space(),
        );
        assert_eq!(lines.len(), 2);
        assert!(lines[0].spans.len() > 1);
    }
}
