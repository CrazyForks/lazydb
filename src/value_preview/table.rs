use crate::db::redis::read::RedisPageValue;
use crate::db::redis::{mutation::RedisMutationOperation, read::RedisType};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisTable {
    pub columns: Vec<String>,
    pub rows: Vec<RedisTableRow>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisTableRow {
    pub cells: Vec<String>,
    /// Source bytes are retained independently of their display text.
    pub identity: Vec<Vec<u8>>,
    /// Stable identity for selecting the same logical row after a refresh.
    pub row_key: Vec<u8>,
}

impl RedisTable {
    /// Return the rows whose complete, display-safe cell text contains the
    /// keyword.  Filtering is deliberately local to the already loaded page.
    pub fn filtered(mut self, keyword: &str) -> Self {
        if keyword.is_empty() {
            return self;
        }
        let keyword = keyword.to_lowercase();
        self.rows.retain(|row| {
            row.cells
                .iter()
                .any(|cell| cell.to_lowercase().contains(&keyword))
        });
        self
    }
}

pub fn from_page(value: &RedisPageValue) -> RedisTable {
    match value {
        RedisPageValue::String(value) => RedisTable {
            columns: vec!["Value".into()],
            rows: vec![RedisTableRow {
                cells: vec![display(value)],
                identity: vec![value.clone()],
                row_key: Vec::new(),
            }],
        },
        RedisPageValue::Hash(values) => RedisTable {
            columns: vec!["Field".into(), "Value".into()],
            rows: values
                .iter()
                .map(|(field, value)| RedisTableRow {
                    cells: vec![display(field), display(value)],
                    identity: vec![field.clone(), value.clone()],
                    row_key: field.clone(),
                })
                .collect(),
        },
        RedisPageValue::List(values) => RedisTable {
            columns: vec!["Index".into(), "Value".into()],
            rows: values
                .iter()
                .map(|(index, value)| RedisTableRow {
                    cells: vec![index.to_string(), display(value)],
                    identity: vec![index.to_string().into_bytes(), value.clone()],
                    row_key: index.to_string().into_bytes(),
                })
                .collect(),
        },
        RedisPageValue::Set(values) => RedisTable {
            columns: vec!["Member".into()],
            rows: values
                .iter()
                .map(|value| RedisTableRow {
                    cells: vec![display(value)],
                    identity: vec![value.clone()],
                    row_key: value.clone(),
                })
                .collect(),
        },
        RedisPageValue::SortedSet(values) => RedisTable {
            columns: vec!["Member".into(), "Score".into()],
            rows: values
                .iter()
                .map(|(member, score)| RedisTableRow {
                    cells: vec![display(member), display(score)],
                    identity: vec![member.clone(), score.clone()],
                    row_key: member.clone(),
                })
                .collect(),
        },
        RedisPageValue::Stream(values) => RedisTable {
            columns: vec!["ID".into(), "Fields".into()],
            rows: values
                .iter()
                .map(|(id, fields)| RedisTableRow {
                    cells: vec![display(id), fields.len().to_string()],
                    identity: vec![id.clone()],
                    row_key: id.clone(),
                })
                .collect(),
        },
    }
}

impl RedisTableRow {
    pub fn source_cell(&self, column: usize) -> Option<&[u8]> {
        self.identity.get(column).map(Vec::as_slice)
    }

    /// Whether this displayed column represents a value that can be edited
    /// without inventing data from the presentation layer.
    pub fn is_editable_column(
        &self,
        value_type: crate::db::redis::read::RedisType,
        column: usize,
    ) -> bool {
        match value_type {
            crate::db::redis::read::RedisType::String => column == 0,
            crate::db::redis::read::RedisType::Hash => matches!(column, 0 | 1),
            crate::db::redis::read::RedisType::List => column == 1,
            crate::db::redis::read::RedisType::Set => column == 0,
            crate::db::redis::read::RedisType::SortedSet => matches!(column, 0 | 1),
            // Fields is a derived count, and an existing stream ID is not
            // safely editable in place.
            crate::db::redis::read::RedisType::Stream => false,
            crate::db::redis::read::RedisType::Module
            | crate::db::redis::read::RedisType::Missing
            | crate::db::redis::read::RedisType::Unknown => false,
        }
    }

    /// Build a compare-and-set operation for an editable table cell. Display
    /// strings are never used here; `identity` retains the original bytes.
    pub fn edit_operation(
        &self,
        value_type: RedisType,
        column: usize,
        value: Vec<u8>,
    ) -> Result<RedisMutationOperation, String> {
        match (value_type, column) {
            (RedisType::String, 0) => {
                let expected = self.source_cell(0).ok_or("missing string value")?;
                Ok(RedisMutationOperation::SetString {
                    value,
                    expected: Some(expected.to_vec()),
                })
            }
            (RedisType::Hash, 1) => {
                let field = self.source_cell(0).ok_or("missing hash field")?;
                let expected = self.source_cell(1).ok_or("missing hash value")?;
                Ok(RedisMutationOperation::SetHashField {
                    field: field.to_vec(),
                    value,
                    expected: Some(expected.to_vec()),
                })
            }
            (RedisType::List, 1) => {
                let index = self.source_cell(0).ok_or("missing list index")?;
                let index = std::str::from_utf8(index)
                    .map_err(|_| "list index is not valid UTF-8")?
                    .parse::<i64>()
                    .map_err(|_| "list index is invalid")?;
                let expected = self.source_cell(1).ok_or("missing list value")?;
                Ok(RedisMutationOperation::SetListElement {
                    index,
                    value,
                    expected: expected.to_vec(),
                })
            }
            (RedisType::Set, 0) => Ok(RedisMutationOperation::ReplaceSetMember {
                member: self.source_cell(0).ok_or("missing set member")?.to_vec(),
                replacement: value,
            }),
            (RedisType::SortedSet, 1) => {
                let member = self.source_cell(0).ok_or("missing sorted-set member")?;
                let score = std::str::from_utf8(&value)
                    .map_err(|_| "sorted-set score is not valid UTF-8")?
                    .parse::<f64>()
                    .map_err(|_| "sorted-set score is invalid")?;
                if !score.is_finite() {
                    return Err("sorted-set score must be finite".into());
                }
                let expected = self
                    .source_cell(1)
                    .and_then(|bytes| std::str::from_utf8(bytes).ok().map(str::to_owned));
                Ok(RedisMutationOperation::SetSortedSetMember {
                    member: member.to_vec(),
                    score: String::from_utf8(value).map_err(|_| "score is not UTF-8")?,
                    expected_score: expected,
                })
            }
            _ => Err("this table column is not editable in place".into()),
        }
    }

    /// Build a compare-and-set deletion for the selected table row.
    pub fn delete_operation(
        &self,
        value_type: RedisType,
    ) -> Result<RedisMutationOperation, String> {
        match value_type {
            RedisType::String => Ok(RedisMutationOperation::DeleteString {
                expected: self.source_cell(0).ok_or("missing string value")?.to_vec(),
            }),
            RedisType::Hash => Ok(RedisMutationOperation::DeleteHashField {
                field: self.source_cell(0).ok_or("missing hash field")?.to_vec(),
                expected: Some(self.source_cell(1).ok_or("missing hash value")?.to_vec()),
            }),
            RedisType::List => {
                let index = std::str::from_utf8(self.source_cell(0).ok_or("missing list index")?)
                    .map_err(|_| "list index is not valid UTF-8")?
                    .parse::<i64>()
                    .map_err(|_| "list index is invalid")?;
                Ok(RedisMutationOperation::DeleteListElement {
                    index,
                    expected: self.source_cell(1).ok_or("missing list value")?.to_vec(),
                })
            }
            RedisType::Set => Ok(RedisMutationOperation::RemoveSetMember {
                member: self.source_cell(0).ok_or("missing set member")?.to_vec(),
            }),
            RedisType::SortedSet => Ok(RedisMutationOperation::RemoveSortedSetMember {
                member: self
                    .source_cell(0)
                    .ok_or("missing sorted-set member")?
                    .to_vec(),
                expected_score: Some(
                    std::str::from_utf8(self.source_cell(1).ok_or("missing sorted-set score")?)
                        .map_err(|_| "sorted-set score is not valid UTF-8")?
                        .to_owned(),
                ),
            }),
            RedisType::Stream | RedisType::Module | RedisType::Missing | RedisType::Unknown => {
                Err("this Redis table row is not deletable through this operation".into())
            }
        }
    }
}

fn display(value: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(value)
        && text
            .chars()
            .all(|character| !character.is_control() || character.is_ascii_whitespace())
    {
        text.to_owned()
    } else {
        value.iter().map(|byte| format!("\\x{byte:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::redis::read::RedisType;

    #[test]
    fn keyword_filter_is_case_insensitive_and_preserves_row_identity() {
        let table = RedisTable {
            columns: vec!["Field".into(), "Value".into()],
            rows: vec![
                RedisTableRow {
                    cells: vec!["first".into(), "ordinary".into()],
                    identity: vec![b"first".to_vec(), b"ordinary".to_vec()],
                    row_key: b"first".to_vec(),
                },
                RedisTableRow {
                    cells: vec!["second".into(), "Needle value".into()],
                    identity: vec![b"second".to_vec(), b"Needle value".to_vec()],
                    row_key: b"second".to_vec(),
                },
            ],
        };

        let filtered = table.filtered("needle");
        assert_eq!(filtered.columns, vec!["Field", "Value"]);
        assert_eq!(filtered.rows.len(), 1);
        assert_eq!(filtered.rows[0].row_key, b"second");
        assert_eq!(filtered.rows[0].identity[1], b"Needle value");
    }

    #[test]
    fn empty_keyword_filter_returns_all_rows_and_literal_text_is_not_regex() {
        let table = RedisTable {
            columns: vec!["Value".into()],
            rows: vec![
                RedisTableRow {
                    cells: vec!["a.*b".into()],
                    identity: vec![b"a.*b".to_vec()],
                    row_key: b"a.*b".to_vec(),
                },
                RedisTableRow {
                    cells: vec!["axb".into()],
                    identity: vec![b"axb".to_vec()],
                    row_key: b"axb".to_vec(),
                },
            ],
        };

        assert_eq!(table.clone().filtered("").rows.len(), 2);
        assert_eq!(table.filtered(".*").rows.len(), 1);
    }

    #[test]
    fn derived_list_index_and_stream_fields_are_not_editable() {
        let row = RedisTableRow {
            cells: vec!["0".into(), "value".into()],
            identity: vec![b"0".to_vec(), b"value".to_vec()],
            row_key: b"0".to_vec(),
        };
        assert!(!row.is_editable_column(RedisType::List, 0));
        assert!(row.is_editable_column(RedisType::List, 1));
        assert!(!row.is_editable_column(RedisType::Stream, 0));
        assert!(!row.is_editable_column(RedisType::Stream, 1));
    }

    #[test]
    fn hash_value_edit_and_delete_keep_original_bytes_as_expectations() {
        let row = RedisTableRow {
            cells: vec!["field".into(), "shown".into()],
            identity: vec![b"field".to_vec(), vec![0, 255]],
            row_key: b"field".to_vec(),
        };
        assert_eq!(
            row.edit_operation(RedisType::Hash, 1, b"new".to_vec())
                .unwrap(),
            RedisMutationOperation::SetHashField {
                field: b"field".to_vec(),
                value: b"new".to_vec(),
                expected: Some(vec![0, 255]),
            }
        );
        assert_eq!(
            row.delete_operation(RedisType::Hash).unwrap(),
            RedisMutationOperation::DeleteHashField {
                field: b"field".to_vec(),
                expected: Some(vec![0, 255]),
            }
        );
    }

    #[test]
    fn derived_columns_and_invalid_scores_are_rejected() {
        let row = RedisTableRow {
            cells: vec!["2".into(), "member".into()],
            identity: vec![b"2".to_vec(), b"member".to_vec()],
            row_key: b"2".to_vec(),
        };
        assert!(
            row.edit_operation(RedisType::List, 0, b"3".to_vec())
                .is_err()
        );
        assert!(
            row.edit_operation(RedisType::SortedSet, 1, b"NaN".to_vec())
                .is_err()
        );
    }
}
