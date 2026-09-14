//! Decoder dispatch lives here; concrete format implementations are added in
//! later tasks.  Keeping the dispatch API small lets the Redis UI remain
//! unaware of parser-specific details.

use super::{DecodeError, PreviewFormat};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodedValue {
    Bytes(Vec<u8>),
    Text(String),
}

pub fn decode(data: &[u8], format: PreviewFormat) -> Result<DecodedValue, DecodeError> {
    match format.encoding {
        super::ValueEncoding::Text | super::ValueEncoding::Unknown
            if matches!(format.view, super::ValueView::Raw | super::ValueView::Hex) =>
        {
            Ok(DecodedValue::Bytes(data.to_vec()))
        }
        _ => Err(DecodeError::new(
            super::DecodeStatus::Unsupported,
            "decoder is not implemented yet",
        )),
    }
}
