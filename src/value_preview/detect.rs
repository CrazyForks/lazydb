use super::{DecodeStatus, FormatCandidate, PreviewFormat, ValueEncoding};

pub fn detect(data: &[u8]) -> Vec<FormatCandidate> {
    let mut candidates = Vec::new();
    if serde_json::from_slice::<serde_json::Value>(data).is_ok() {
        candidates.push(FormatCandidate {
            format: PreviewFormat::JSON,
            confidence: 100,
            reason: "valid JSON",
            status: DecodeStatus::Complete,
        });
    }
    if super::java::is_java_serialization(data) {
        candidates.push(candidate(
            ValueEncoding::Java,
            100,
            "Java stream header",
            || super::java::parse_java_to_json(data).map(|_| ()),
        ));
    }
    if super::php::is_php_serialization(data) {
        candidates.push(candidate(
            ValueEncoding::Php,
            95,
            "PHP serialization prefix",
            || super::php::parse_php_to_json(data).map(|_| ()),
        ));
    }
    if super::pickle::is_pickle_serialization(data) {
        candidates.push(candidate(
            ValueEncoding::Pickle,
            95,
            "Pickle protocol",
            || super::pickle::parse_pickle_to_json(data).map(|_| ()),
        ));
    }
    if super::protobuf::is_protobuf_data(data) {
        candidates.push(candidate(
            ValueEncoding::Protobuf,
            40,
            "valid protobuf wire fields",
            || super::protobuf::parse_protobuf_to_json(data).map(|_| ()),
        ));
    }
    if data.len() <= super::MAX_PREVIEW_INPUT_BYTES
        && std::str::from_utf8(data).is_ok()
        && let Ok(value) = serde_yaml::from_slice::<serde_yaml::Value>(data)
        && matches!(
            value,
            serde_yaml::Value::Mapping(_) | serde_yaml::Value::Sequence(_)
        )
    {
        candidates.push(FormatCandidate {
            format: PreviewFormat::YAML,
            confidence: 90,
            reason: "valid structured YAML",
            status: DecodeStatus::Complete,
        });
    }
    if candidates.is_empty() && std::str::from_utf8(data).is_ok() {
        candidates.push(FormatCandidate {
            format: PreviewFormat::RAW,
            confidence: 60,
            reason: "valid UTF-8 text",
            status: DecodeStatus::Complete,
        });
    }
    candidates
}

pub fn default_format(data: &[u8], collection: bool) -> PreviewFormat {
    if collection {
        return PreviewFormat::TABLE;
    }
    let candidates = detect(data);
    candidates
        .into_iter()
        .filter(|candidate| candidate.status == DecodeStatus::Complete)
        .max_by_key(|candidate| candidate.confidence)
        .map(|candidate| candidate.format)
        .unwrap_or_else(|| {
            if std::str::from_utf8(data).is_ok() {
                PreviewFormat::RAW
            } else {
                PreviewFormat::HEX
            }
        })
}

fn candidate(
    encoding: ValueEncoding,
    confidence: u8,
    reason: &'static str,
    parse: impl FnOnce() -> Result<(), super::DecodeError>,
) -> FormatCandidate {
    let status = match parse() {
        Ok(()) => DecodeStatus::Complete,
        Err(error) => error.status,
    };
    FormatCandidate {
        format: PreviewFormat::preset(encoding),
        confidence: if status == DecodeStatus::Complete {
            confidence
        } else {
            confidence / 2
        },
        reason,
        status,
    }
}
