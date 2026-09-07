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
            range: TextRange::new(start, (start + 1).min(text.len())),
            message: error.to_string(),
            code: "sql-tokenizer",
        }];
    }
    if let Err(error) = Parser::parse_sql(parser_dialect(dialect), text) {
        return vec![SqlDiagnostic {
            range: TextRange::new(0, text.len()),
            message: error.to_string(),
            code: "sql-parser",
        }];
    }
    Vec::new()
}
