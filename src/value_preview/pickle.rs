use std::io::Cursor;

use super::{DecodeError, DecodeStatus};

pub fn is_pickle_serialization(data: &[u8]) -> bool {
    if data.len() < 2 {
        return false;
    }
    if data[0] == 0x80 {
        return (0..=5).contains(&data[1]) && data.last() == Some(&b'.');
    }
    matches!(
        data[0],
        b'(' | b'c' | b']' | b'}' | b'I' | b'J' | b'K' | b'M' | b'N' | b'V'
    ) && data.last() == Some(&b'.')
}

pub fn parse_pickle_to_json(data: &[u8]) -> Result<String, DecodeError> {
    if data.len() > super::MAX_PREVIEW_INPUT_BYTES {
        return Err(DecodeError::new(
            DecodeStatus::Unsupported,
            "Pickle value exceeds preview input budget",
        ));
    }
    let value = serde_pickle::value_from_reader(Cursor::new(data), serde_pickle::DeOptions::new())
        .map_err(|error| {
            DecodeError::new(
                DecodeStatus::Invalid,
                format!("Pickle parse error: {error}"),
            )
        })?;
    let value = to_json(value);
    let output = serde_json::to_string_pretty(&value)
        .map_err(|error| DecodeError::new(DecodeStatus::Invalid, error.to_string()))?;
    if output.len() > super::MAX_PREVIEW_OUTPUT_BYTES {
        return Err(DecodeError::new(
            DecodeStatus::Unsupported,
            "Pickle preview exceeds output budget",
        ));
    }
    Ok(output)
}

fn to_json(value: serde_pickle::value::Value) -> serde_json::Value {
    use serde_pickle::value::Value;
    match value {
        Value::None => serde_json::Value::Null,
        Value::Bool(value) => serde_json::Value::Bool(value),
        Value::I64(value) => value.into(),
        Value::Int(value) => serde_json::json!({
            "__pickle_type": "integer",
            "value": value.to_string(),
        }),
        Value::F64(value) => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .unwrap_or_else(
                || serde_json::json!({ "__pickle_type": "float", "value": value.to_string() }),
            ),
        Value::Bytes(value) => serde_json::json!({
            "__pickle_type": "bytes",
            "length": value.len(),
            "hex": value.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        }),
        Value::String(value) => value.into(),
        Value::List(values) => values.into_iter().map(to_json).collect(),
        Value::Tuple(values) => serde_json::json!({
            "__pickle_type": "tuple",
            "items": values.into_iter().map(to_json).collect::<Vec<_>>(),
        }),
        Value::Set(values) => serde_json::json!({
            "__pickle_type": "set",
            "items": values.into_iter().map(hashable_to_json).collect::<Vec<_>>(),
        }),
        Value::FrozenSet(values) => serde_json::json!({
            "__pickle_type": "frozenset",
            "items": values.into_iter().map(hashable_to_json).collect::<Vec<_>>(),
        }),
        Value::Dict(values) => serde_json::json!({
            "__pickle_type": "dict",
            "entries": values.into_iter().map(|(key, value)| serde_json::json!({
                "key": hashable_to_json(key),
                "value": to_json(value),
            })).collect::<Vec<_>>(),
        }),
    }
}

fn hashable_to_json(value: serde_pickle::value::HashableValue) -> serde_json::Value {
    use serde_pickle::value::HashableValue;
    match value {
        HashableValue::None => serde_json::Value::Null,
        HashableValue::Bool(value) => value.into(),
        HashableValue::I64(value) => value.into(),
        HashableValue::Int(value) => serde_json::json!({
            "__pickle_type": "integer",
            "value": value.to_string(),
        }),
        HashableValue::F64(value) => serde_json::json!({
            "__pickle_type": "float",
            "value": value.to_string(),
        }),
        HashableValue::Bytes(value) => serde_json::json!({
            "__pickle_type": "bytes",
            "length": value.len(),
            "hex": value.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        }),
        HashableValue::String(value) => value.into(),
        HashableValue::Tuple(values) => serde_json::json!({
            "__pickle_type": "tuple",
            "items": values.into_iter().map(hashable_to_json).collect::<Vec<_>>(),
        }),
        HashableValue::FrozenSet(values) => serde_json::json!({
            "__pickle_type": "frozenset",
            "items": values.into_iter().map(hashable_to_json).collect::<Vec<_>>(),
        }),
    }
}
