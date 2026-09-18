use crate::{
    db::{
        mutation::{InputValue, MetadataFingerprint, RelationColumnMapping},
        value::CellValue,
    },
    model::relation_edit::{EditableRow, EditableRowState},
};

#[derive(Clone, Debug, PartialEq)]
pub struct PlannedInsert {
    pub columns: Vec<usize>,
    pub values: Vec<InputValue>,
}

pub fn plan_insert(
    row: &EditableRow,
    metadata: &MetadataFingerprint,
    mapping: &RelationColumnMapping,
) -> Option<PlannedInsert> {
    if !matches!(row.state, EditableRowState::InsertDraft) {
        return None;
    }
    let mut columns = Vec::with_capacity(row.supplied_columns.len());
    let mut values = Vec::with_capacity(row.supplied_columns.len());
    for result_column in &row.supplied_columns {
        let metadata_column = mapping.metadata_column_for_result(*result_column)?;
        if metadata_column >= metadata.columns.len() {
            return None;
        }
        columns.push(metadata_column);
        values.push(input_value(row.current.get(*result_column)?));
    }
    Some(PlannedInsert { columns, values })
}

fn input_value(value: &CellValue) -> InputValue {
    match value {
        CellValue::Null => InputValue::Null,
        value => InputValue::Value(value.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::plan_insert;
    use crate::{
        db::mutation::{MetadataFingerprint, relation_column_mapping},
        db::value::CellValue,
        model::relation_edit::RelationEditSession,
    };

    #[test]
    fn plan_insert_maps_reordered_result_columns_to_metadata_columns() {
        let metadata = MetadataFingerprint {
            relation: "items".into(),
            columns: vec![
                ("name".into(), "text".into(), true),
                ("id".into(), "text".into(), true),
            ],
            primary_key: Vec::new(),
        };
        let mapping = relation_column_mapping(&metadata, &["id".into(), "name".into()]).unwrap();
        let mut edit = RelationEditSession::default();
        edit.insert_row(
            0,
            vec![CellValue::Text("two".into()), CellValue::Text("one".into())],
        );
        edit.update_cell(0, 0, CellValue::Text("two".into()));
        edit.update_cell(0, 1, CellValue::Text("one".into()));
        let planned = plan_insert(&edit.rows[0], &metadata, &mapping).unwrap();
        assert_eq!(planned.columns, vec![1, 0]);
        assert_eq!(
            planned.values,
            vec![
                crate::db::mutation::InputValue::Value(CellValue::Text("two".into())),
                crate::db::mutation::InputValue::Value(CellValue::Text("one".into())),
            ]
        );
    }
}
