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
}

pub fn from_page(value: &RedisPageValue) -> RedisTable {
    match value {
        RedisPageValue::String(value) => RedisTable {
            columns: vec!["Value".into()],
            rows: vec![RedisTableRow {
                cells: vec![display(value)],
                identity: vec![value.clone()],
            }],
        },
        RedisPageValue::Hash(values) => RedisTable {
            columns: vec!["Field".into(), "Value".into()],
            rows: values
                .iter()
                .map(|(field, value)| RedisTableRow {
                    cells: vec![display(field), display(value)],
                    identity: vec![field.clone(), value.clone()],
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
                })
                .collect(),
        },
    }
}

fn display(value: &[u8]) -> String {
    if value
        .iter()
        .all(|byte| byte.is_ascii_graphic() || byte.is_ascii_whitespace())
    {
        String::from_utf8_lossy(value).into_owned()
    } else {
        value.iter().map(|byte| format!("\\x{byte:02x}")).collect()
    }
}
