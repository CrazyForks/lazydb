use std::io::Cursor;

use super::{DecodeError, DecodeStatus};

pub fn is_pickle_serialization(data: &[u8]) -> bool {
    data.len() >= 3 && data[0] == 0x80 && (2..=5).contains(&data[1]) && data.last() == Some(&b'.')
}

pub fn parse_pickle_to_json(data: &[u8]) -> Result<String, DecodeError> {
    if !is_pickle_serialization(data) {
        return Err(DecodeError::new(
            DecodeStatus::Invalid,
            "invalid Pickle protocol",
        ));
    }
    let value: serde_json::Value =
        serde_pickle::from_reader(Cursor::new(data), serde_pickle::DeOptions::new()).map_err(
            |error| {
                DecodeError::new(
                    DecodeStatus::Invalid,
                    format!("Pickle parse error: {error}"),
                )
            },
        )?;
    serde_json::to_string_pretty(&value)
        .map_err(|error| DecodeError::new(DecodeStatus::Invalid, error.to_string()))
}
