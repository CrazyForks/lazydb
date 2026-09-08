use super::{EmbeddedSqlUnit, SourceSegment, SourceSegmentKind};
use crate::sql::{SqlDialect, TextRange};

const STATEMENT_TAGS: [&str; 4] = ["select", "insert", "update", "delete"];
const DYNAMIC_TAGS: [&str; 10] = [
    "if",
    "choose",
    "when",
    "otherwise",
    "foreach",
    "where",
    "set",
    "trim",
    "include",
    "sql",
];

pub fn extract_units(source: &str) -> Vec<EmbeddedSqlUnit> {
    extract_units_for_dialect(source, SqlDialect::Generic)
}

pub fn extract_units_for_dialect(source: &str, dialect: SqlDialect) -> Vec<EmbeddedSqlUnit> {
    let mut units = Vec::new();
    let mut cursor = 0;
    while let Some((open_start, open_end, name)) = next_statement_tag(source, cursor) {
        let close = format!("</{name}");
        let Some(close_start) = find_case_insensitive(source, &close, open_end) else {
            break;
        };
        let Some(close_end) = source[close_start..]
            .find('>')
            .map(|end| close_start + end + 1)
        else {
            break;
        };
        let content = &source[open_end..close_start];
        let (sql, segments, trusted) = normalize_content(content, open_end, dialect);
        units.push(EmbeddedSqlUnit {
            source: TextRange::new(open_start, close_end),
            sql,
            segments,
            trusted_diagnostics: trusted,
        });
        cursor = close_end;
    }
    units
}

fn next_statement_tag(source: &str, from: usize) -> Option<(usize, usize, String)> {
    let mut cursor = from;
    while let Some(relative) = source[cursor..].find('<') {
        let start = cursor + relative;
        let end = source[start..].find('>')? + start + 1;
        let inside = source[start + 1..end - 1].trim_start();
        if inside.starts_with('/') || inside.starts_with('!') || inside.starts_with('?') {
            cursor = end;
            continue;
        }
        let name = inside
            .split_ascii_whitespace()
            .next()?
            .trim_end_matches('/');
        if STATEMENT_TAGS.contains(&name.to_ascii_lowercase().as_str()) {
            return Some((start, end, name.to_ascii_lowercase()));
        }
        cursor = end;
    }
    None
}

fn normalize_content(
    content: &str,
    source_offset: usize,
    dialect: SqlDialect,
) -> (String, Vec<SourceSegment>, bool) {
    let mut sql = String::new();
    let mut segments = Vec::new();
    let mut trusted = true;
    let mut parameter_count = 0;
    let mut cursor = 0;
    while cursor < content.len() {
        if content[cursor..].starts_with("<![CDATA[") {
            let body_start = cursor + "<![CDATA[".len();
            let body_end = content[body_start..]
                .find("]]>")
                .map(|value| body_start + value)
                .unwrap_or(content.len());
            append_text(
                &content[body_start..body_end],
                source_offset + body_start,
                &mut sql,
                &mut segments,
                &mut trusted,
                dialect,
                &mut parameter_count,
            );
            cursor = (body_end + 3).min(content.len());
            continue;
        }
        if content[cursor..].starts_with("<!--") {
            let end = content[cursor + 4..]
                .find("-->")
                .map(|value| cursor + 4 + value + 3)
                .unwrap_or(content.len());
            cursor = end;
            continue;
        }
        if content[cursor..].starts_with('<') {
            let end = content[cursor..].find('>').map(|value| cursor + value + 1);
            let Some(end) = end else {
                trusted = false;
                break;
            };
            let name = content[cursor + 1..end - 1]
                .trim_start_matches('/')
                .split_ascii_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if DYNAMIC_TAGS.contains(&name.as_str()) {
                trusted = false;
            }
            cursor = end;
            continue;
        }
        let next = content[cursor..]
            .find('<')
            .map(|value| cursor + value)
            .unwrap_or(content.len());
        let text = &content[cursor..next];
        append_text(
            text,
            source_offset + cursor,
            &mut sql,
            &mut segments,
            &mut trusted,
            dialect,
            &mut parameter_count,
        );
        cursor = next;
    }
    (sql, segments, trusted)
}

fn append_text(
    text: &str,
    source_start: usize,
    sql: &mut String,
    segments: &mut Vec<SourceSegment>,
    trusted: &mut bool,
    dialect: SqlDialect,
    parameter_count: &mut usize,
) {
    let mut cursor = 0;
    while cursor < text.len() {
        let next_special = ["#{", "${", "&lt;", "&gt;", "&amp;", "&quot;", "&apos;"]
            .into_iter()
            .filter_map(|needle| {
                text[cursor..]
                    .find(needle)
                    .map(|value| (cursor + value, needle))
            })
            .min_by_key(|(offset, _)| *offset);
        let Some((special, needle)) = next_special else {
            push_segment(
                text,
                cursor,
                text.len(),
                source_start,
                sql,
                segments,
                SourceSegmentKind::Original,
            );
            break;
        };
        if special > cursor {
            push_segment(
                text,
                cursor,
                special,
                source_start,
                sql,
                segments,
                SourceSegmentKind::Original,
            );
        }
        if needle == "#{" || needle == "${" {
            let end = text[special + 2..]
                .find('}')
                .map(|value| special + value + 3);
            let Some(end) = end else {
                *trusted = false;
                break;
            };
            let generated_start = sql.len();
            *parameter_count += 1;
            match dialect {
                SqlDialect::Postgres => sql.push_str(&format!("${parameter_count}")),
                SqlDialect::SqlServer => sql.push_str(&format!("@p{parameter_count}")),
                _ => sql.push('?'),
            }
            segments.push(SourceSegment {
                source: TextRange::new(source_start + special, source_start + end),
                generated: TextRange::new(generated_start, sql.len()),
                kind: if needle == "#{" {
                    SourceSegmentKind::Parameter
                } else {
                    *trusted = false;
                    SourceSegmentKind::Unknown
                },
            });
            cursor = end;
        } else {
            let (decoded, end) = match needle {
                "&lt;" => ('<', special + 4),
                "&gt;" => ('>', special + 4),
                "&amp;" => ('&', special + 5),
                "&quot;" => ('"', special + 6),
                _ => ('\'', special + 6),
            };
            let generated_start = sql.len();
            sql.push(decoded);
            segments.push(SourceSegment {
                source: TextRange::new(source_start + special, source_start + end),
                generated: TextRange::new(generated_start, generated_start + 1),
                kind: SourceSegmentKind::EntityDecoded,
            });
            cursor = end;
        }
    }
}

fn push_segment(
    text: &str,
    start: usize,
    end: usize,
    source_start: usize,
    sql: &mut String,
    segments: &mut Vec<SourceSegment>,
    kind: SourceSegmentKind,
) {
    if start == end {
        return;
    }
    let generated_start = sql.len();
    sql.push_str(&text[start..end]);
    segments.push(SourceSegment {
        source: TextRange::new(source_start + start, source_start + end),
        generated: TextRange::new(generated_start, sql.len()),
        kind,
    });
}

fn find_case_insensitive(source: &str, needle: &str, from: usize) -> Option<usize> {
    source[from..].char_indices().find_map(|(offset, _)| {
        source[from + offset..]
            .get(..needle.len())
            .filter(|value| value.eq_ignore_ascii_case(needle))
            .map(|_| from + offset)
    })
}
