use crate::sql::{SqlDiagnostic, SqlDialect, diagnose_sql};
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

use super::{document::Document, position::PositionIndex};

pub fn diagnostics_for_document(document: &Document, dialect: SqlDialect) -> Vec<Diagnostic> {
    if document.language_id == "xml" {
        return crate::sql::embedded::mybatis::extract_units(&document.text)
            .into_iter()
            .filter(|unit| unit.trusted_diagnostics)
            .flat_map(|unit| {
                let mut virtual_document = document.clone();
                virtual_document.language_id = "sql".into();
                virtual_document.text = unit.sql.clone();
                diagnostics_for_document(&virtual_document, dialect)
                    .into_iter()
                    .filter_map(move |mut diagnostic| {
                        let positions = PositionIndex::new(unit.sql.clone());
                        let start = positions.offset(diagnostic.range.start);
                        let end = positions.offset(diagnostic.range.end);
                        let source_range =
                            unit.source_edit(crate::sql::TextRange::new(start, end))?;
                        let source_positions = PositionIndex::new(document.text.clone());
                        diagnostic.range =
                            source_positions.range(source_range.start, source_range.end);
                        Some(diagnostic)
                    })
            })
            .collect();
    }
    let positions = PositionIndex::new(document.text.clone());
    diagnose_sql(&document.text, dialect)
        .into_iter()
        .map(|diagnostic| to_lsp_diagnostic(&positions, diagnostic))
        .collect()
}

fn to_lsp_diagnostic(positions: &PositionIndex, diagnostic: SqlDiagnostic) -> Diagnostic {
    Diagnostic {
        range: positions.range(diagnostic.range.start, diagnostic.range.end),
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(diagnostic.code.into())),
        source: Some("lazydb".into()),
        message: diagnostic.message,
        ..Default::default()
    }
}
