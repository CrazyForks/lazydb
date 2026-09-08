use crate::sql::{
    CompletionContext, CompletionIndex, CompletionInsertionMode, CompletionKind, SqlDialect,
    complete_with_mode, completion_dependencies, qualifier_segments_at,
};
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind as LspKind, CompletionList, CompletionResponse,
    CompletionTextEdit, Position, TextEdit,
};

use super::catalog::CatalogTargetService;
use super::{document::Document, position::PositionIndex};

pub fn complete_document_with_embedded_sql(
    document: &Document,
    position: Position,
    dialect: SqlDialect,
    index: &CompletionIndex,
    is_incomplete: bool,
    completion_context: CompletionContext<'_>,
) -> CompletionResponse {
    if document.language_id != "xml" {
        return complete_document_with_context(
            document,
            position,
            dialect,
            index,
            is_incomplete,
            completion_context,
        );
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
            return CompletionResponse::List(CompletionList {
                is_incomplete: true,
                items: Vec::new(),
            });
        };
        let mut virtual_document = document.clone();
        virtual_document.text = unit.sql.clone();
        let generated = complete_document_with_context(
            &virtual_document,
            PositionIndex::new(virtual_document.text.clone()).position(generated_cursor),
            dialect,
            index,
            is_incomplete,
            completion_context,
        );
        return map_embedded_completion(generated, &source_positions, &unit);
    }
    CompletionResponse::List(CompletionList {
        is_incomplete: false,
        items: Vec::new(),
    })
}

pub async fn complete_document_with_catalog(
    document: &Document,
    position: Position,
    dialect: SqlDialect,
    service: &std::sync::Arc<CatalogTargetService>,
    completion_context: CompletionContext<'_>,
) -> CompletionResponse {
    if document.language_id != "xml" {
        let positions = PositionIndex::new(document.text.clone());
        let cursor = positions.offset(position);
        return complete_text_with_catalog(
            &document.text,
            cursor,
            dialect,
            service,
            completion_context,
            &positions,
        )
        .await;
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
            return CompletionResponse::List(CompletionList {
                is_incomplete: true,
                items: Vec::new(),
            });
        };
        let generated = complete_text_with_catalog(
            &unit.sql,
            generated_cursor,
            dialect,
            service,
            completion_context,
            &PositionIndex::new(unit.sql.clone()),
        )
        .await;
        return map_embedded_completion(generated, &source_positions, &unit);
    }
    CompletionResponse::List(CompletionList {
        is_incomplete: false,
        items: Vec::new(),
    })
}

async fn complete_text_with_catalog(
    text: &str,
    cursor: usize,
    dialect: SqlDialect,
    service: &std::sync::Arc<CatalogTargetService>,
    completion_context: CompletionContext<'_>,
    positions: &PositionIndex,
) -> CompletionResponse {
    let qualifiers = qualifier_segments_at(text, cursor, dialect);
    let (base_entries, base_incomplete) = service.warm(completion_context, &qualifiers).await;
    let base_index = CompletionIndex::new(&base_entries);
    let (children_entries, children_incomplete) = if base_incomplete {
        (Vec::new(), true)
    } else {
        let relation_children =
            completion_dependencies(text, cursor, dialect, &base_index, completion_context)
                .relation_children;
        service.load_children(&relation_children).await
    };
    let mut merged = base_entries;
    merged.extend(children_entries);
    let index = CompletionIndex::new(&merged);
    let incomplete = base_incomplete || children_incomplete;
    CompletionResponse::List(CompletionList {
        is_incomplete: incomplete,
        items: map_candidates(
            complete_with_mode(
                text,
                cursor,
                dialect,
                &index,
                completion_context,
                CompletionInsertionMode::CurrentSegment,
            ),
            positions,
        ),
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
        let Some(CompletionTextEdit::Edit(edit)) = item.text_edit.as_mut() else {
            continue;
        };
        let generated = PositionIndex::new(unit.sql.clone());
        let source_range = unit.source_edit(crate::sql::TextRange::new(
            generated.offset(edit.range.start),
            generated.offset(edit.range.end),
        ));
        let Some(source_range) = source_range else {
            continue;
        };
        edit.range = positions.range(source_range.start, source_range.end);
    }
    list.items.retain(|item| item.text_edit.is_some());
    CompletionResponse::List(list)
}

pub fn complete_document(
    document: &Document,
    position: Position,
    dialect: SqlDialect,
    index: &CompletionIndex,
    is_incomplete: bool,
) -> CompletionResponse {
    complete_document_with_context(
        document,
        position,
        dialect,
        index,
        is_incomplete,
        CompletionContext::default(),
    )
}

pub fn complete_document_with_context(
    document: &Document,
    position: Position,
    dialect: SqlDialect,
    index: &CompletionIndex,
    is_incomplete: bool,
    completion_context: CompletionContext<'_>,
) -> CompletionResponse {
    let positions = PositionIndex::new(document.text.clone());
    let cursor = positions.offset(position);
    CompletionResponse::List(CompletionList {
        is_incomplete,
        items: map_candidates(
            complete_with_mode(
                &document.text,
                cursor,
                dialect,
                index,
                completion_context,
                CompletionInsertionMode::CurrentSegment,
            ),
            &positions,
        ),
    })
}

fn map_candidates(
    candidates: Vec<crate::sql::CompletionCandidate>,
    positions: &PositionIndex,
) -> Vec<CompletionItem> {
    candidates
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
        .collect()
}

fn lsp_kind(kind: CompletionKind) -> LspKind {
    match kind {
        CompletionKind::Keyword | CompletionKind::BuiltinExpression => LspKind::KEYWORD,
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
