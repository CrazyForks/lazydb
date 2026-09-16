use crate::db::redis::read::RedisPageValue;

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
    use super::RedisTable;

    #[test]
    fn keyword_filter_is_case_insensitive_and_preserves_row_identity() {
        let table = RedisTable {
            columns: vec!["Field".into(), "Value".into()],
            rows: vec![
                super::RedisTableRow {
                    cells: vec!["first".into(), "ordinary".into()],
                    identity: vec![b"first".to_vec(), b"ordinary".to_vec()],
                    row_key: b"first".to_vec(),
                },
                super::RedisTableRow {
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
                super::RedisTableRow {
                    cells: vec!["a.*b".into()],
                    identity: vec![b"a.*b".to_vec()],
                    row_key: b"a.*b".to_vec(),
                },
                super::RedisTableRow {
                    cells: vec!["axb".into()],
                    identity: vec![b"axb".to_vec()],
                    row_key: b"axb".to_vec(),
                },
            ],
        };

        assert_eq!(table.clone().filtered("").rows.len(), 2);
        assert_eq!(table.filtered(".*").rows.len(), 1);
    }
}
