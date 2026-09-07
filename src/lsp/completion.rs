use crate::sql::{CompletionContext, CompletionIndex, CompletionKind, SqlDialect, complete};
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind as LspKind, CompletionList, CompletionResponse,
    CompletionTextEdit, Position, TextEdit,
};

use super::{document::Document, position::PositionIndex};

pub fn complete_document_with_embedded_sql(
    document: &Document,
    position: Position,
    dialect: SqlDialect,
    index: &CompletionIndex,
    is_incomplete: bool,
) -> CompletionResponse {
    if document.language_id != "xml" {
        return complete_document(document, position, dialect, index, is_incomplete);
    }
    let source_positions = PositionIndex::new(document.text.clone());
    let source_cursor = source_positions.offset(position);
    for unit in crate::sql::embedded::mybatis::extract_units(&document.text) {
        if source_cursor < unit.source.start || source_cursor > unit.source.end {
            continue;
        }
        let Some(generated_cursor) = unit
            .segments
            .iter()
            .find(|segment| {
                segment.source.start <= source_cursor && source_cursor <= segment.source.end
            })
            .map(|segment| segment.generated.start + (source_cursor - segment.source.start))
        else {
            return CompletionResponse::List(tower_lsp_server::ls_types::CompletionList {
                is_incomplete: true,
                items: Vec::new(),
            });
        };
        let mut virtual_document = document.clone();
        virtual_document.text = unit.sql.clone();
        let generated = complete_document(
            &virtual_document,
            PositionIndex::new(virtual_document.text.clone()).position(generated_cursor),
            dialect,
            index,
            is_incomplete,
        );
        return map_embedded_completion(generated, &source_positions, &unit);
    }
    CompletionResponse::List(tower_lsp_server::ls_types::CompletionList {
        is_incomplete: false,
        items: Vec::new(),
    })
}

fn map_embedded_completion(
    response: CompletionResponse,
    positions: &PositionIndex,
    unit: &crate::sql::embedded::EmbeddedSqlUnit,
) -> CompletionResponse {
    let CompletionResponse::List(mut list) = response else {
        return response;
    };
    for item in &mut list.items {
        let Some(tower_lsp_server::ls_types::CompletionTextEdit::Edit(edit)) =
            item.text_edit.as_mut()
        else {
            continue;
        };
        let generated = PositionIndex::new(unit.sql.clone());
        let source_range = unit.source_edit(crate::sql::TextRange::new(
            generated.offset(edit.range.start),
            generated.offset(edit.range.end),
        ));
        let Some(source_range) = source_range else {
            item.text_edit = None;
            continue;
        };
        edit.range = positions.range(source_range.start, source_range.end);
    }
    CompletionResponse::List(list)
}

pub fn complete_document(
    document: &Document,
    position: Position,
    dialect: SqlDialect,
    index: &CompletionIndex,
    is_incomplete: bool,
) -> CompletionResponse {
    let positions = PositionIndex::new(document.text.clone());
    let cursor = positions.offset(position);
    let candidates = complete(
        &document.text,
        cursor,
        dialect,
        index,
        CompletionContext::default(),
    );
    CompletionResponse::List(CompletionList {
        is_incomplete,
        items: candidates
            .into_iter()
            .map(|candidate| {
                let range = positions.range(candidate.replace.start, candidate.replace.end);
                CompletionItem {
                    label: candidate.label,
                    kind: Some(lsp_kind(candidate.kind)),
                    detail: candidate.detail,
                    sort_text: Some(format!(
                        "{}{}{}",
                        9 - candidate.score.context,
                        9 - candidate.score.name_match,
                        9 - candidate.score.schema
                    )),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                        range,
                        candidate.insert_text,
                    ))),
                    ..Default::default()
                }
            })
            .collect(),
    })
}

fn lsp_kind(kind: CompletionKind) -> LspKind {
    match kind {
        CompletionKind::Keyword => LspKind::KEYWORD,
        CompletionKind::DataType | CompletionKind::Type => LspKind::TYPE_PARAMETER,
        CompletionKind::Database | CompletionKind::Schema => LspKind::MODULE,
        CompletionKind::Table | CompletionKind::View => LspKind::CLASS,
        CompletionKind::Column => LspKind::FIELD,
        CompletionKind::Function | CompletionKind::Procedure => LspKind::FUNCTION,
        CompletionKind::Index
        | CompletionKind::Constraint
        | CompletionKind::Trigger
        | CompletionKind::Sequence => LspKind::REFERENCE,
    }
}
