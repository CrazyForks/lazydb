use crate::sql::{SqlDiagnostic, SqlDialect, diagnose_sql};
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

use super::{document::Document, position::PositionIndex};

pub fn diagnostics_for_document(document: &Document, dialect: SqlDialect) -> Vec<Diagnostic> {
    if document.language_id == "xml" {
        let positions = PositionIndex::new(document.text.clone());
        return crate::sql::embedded::mybatis::extract_units_for_dialect(&document.text, dialect)
            .into_iter()
            .filter(|unit| unit.trusted_diagnostics)
            .flat_map(|unit| {
                let positions = &positions;
                diagnose_sql(&unit.sql, dialect)
                    .into_iter()
                    .filter_map(move |mut diagnostic| {
                        diagnostic.range = unit.source_diagnostic(diagnostic.range)?;
                        if let Some((message, _)) = diagnostic.message.rsplit_once(" at Line: ") {
                            diagnostic.message = message.to_owned();
                        }
                        Some(to_lsp_diagnostic(positions, diagnostic))
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
