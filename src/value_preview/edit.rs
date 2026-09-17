use super::{PreviewFormat, ValueView};
use crate::db::redis::mutation::RedisValueDraft;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditValidation {
    Valid,
    Warning(String),
}

pub fn encode_string(text: &str, format: PreviewFormat) -> Result<Vec<u8>, String> {
    match format.view {
        ValueView::Raw | ValueView::Table => Ok(text.as_bytes().to_vec()),
        ValueView::Hex => decode_hex(text),
        // JSON/YAML are validation views over a Redis string.  Saving keeps
        // the user's bytes rather than silently re-serializing or formatting
        // them.  The caller uses `validate_string` to decide whether an
        // invalid document needs an explicit “save anyway” confirmation.
        ValueView::Json | ValueView::Yaml => Ok(text.as_bytes().to_vec()),
    }
}

pub fn validate_string(text: &str, format: PreviewFormat) -> EditValidation {
    match format.view {
        ValueView::Json => serde_json::from_str::<serde_json::Value>(text)
            .map(|_| EditValidation::Valid)
            .unwrap_or_else(|error| EditValidation::Warning(format!("JSON parse error: {error}"))),
        ValueView::Yaml => serde_yaml::from_str::<serde_yaml::Value>(text)
            .map(|_| EditValidation::Valid)
            .unwrap_or_else(|error| EditValidation::Warning(format!("YAML parse error: {error}"))),
        _ => EditValidation::Valid,
    }
}

pub fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    let digits = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    if digits.len() % 2 != 0 {
        return Err("hex value must contain an even number of digits".into());
    }
    digits
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_digit(pair[0])?;
            let low = hex_digit(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_digit(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(format!("invalid hex digit: {byte:?}")),
    }
}

pub fn draft_from_string(text: &str, format: PreviewFormat) -> Result<RedisValueDraft, String> {
    Ok(RedisValueDraft::String(encode_string(text, format)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_text_is_lossless_and_hex_is_unambiguous() {
        assert_eq!(
            encode_string(r"\\x41", PreviewFormat::RAW).unwrap(),
            br"\\x41"
        );
        assert_eq!(decode_hex("00 ff\n41").unwrap(), vec![0, 255, 65]);
        assert!(decode_hex("f").is_err());
        assert!(decode_hex("gg").is_err());
    }

    #[test]
    fn json_and_yaml_validation_is_separate_from_raw_encoding() {
        assert!(matches!(
            validate_string("{", PreviewFormat::JSON),
            EditValidation::Warning(_)
        ));
        assert!(matches!(
            validate_string("{", PreviewFormat::RAW),
            EditValidation::Valid
        ));
        assert_eq!(
            encode_string("缓存😀", PreviewFormat::RAW).unwrap(),
            "缓存😀".as_bytes()
        );
    }
}
