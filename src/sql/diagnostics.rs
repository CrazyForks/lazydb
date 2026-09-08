use sqlparser::{parser::Parser, tokenizer::Tokenizer};

use super::{SqlDialect, TextRange, analysis::LineIndex, dialect::parser_dialect};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqlDiagnostic {
    pub range: TextRange,
    pub message: String,
    pub code: &'static str,
}

pub fn diagnose_sql(text: &str, dialect: SqlDialect) -> Vec<SqlDiagnostic> {
    if let Err(error) = Tokenizer::new(parser_dialect(dialect), text).tokenize() {
        let index = LineIndex::new(text);
        let start = index.offset(text, error.location.line, error.location.column);
        return vec![SqlDiagnostic {
            range: TextRange::new(
                start,
                start + text[start..].chars().next().map_or(0, char::len_utf8),
            ),
            message: error.to_string(),
            code: "sql-tokenizer",
        }];
    }
    if let Err(error) = Parser::parse_sql(parser_dialect(dialect), text) {
        let message = error.to_string();
        // sqlparser exposes parser locations only as a trailing message suffix.
        let location = message.rsplit_once(" at Line: ").and_then(|(_, location)| {
            let (line, column) = location.split_once(", Column: ")?;
            Some((line.parse::<u64>().ok()?, column.parse::<u64>().ok()?))
        });
        let range = if let Some((line, column)) = location {
            let start = LineIndex::new(text).offset(text, line, column);
            TextRange::new(
                start,
                start + text[start..].chars().next().map_or(0, char::len_utf8),
            )
        } else if message.ends_with("found: EOF") {
            TextRange::new(text.trim_end().len(), text.trim_end().len())
        } else {
            TextRange::new(0, text.len())
        };
        return vec![SqlDiagnostic {
            range,
            message,
            code: "sql-parser",
        }];
    }
    Vec::new()
}
