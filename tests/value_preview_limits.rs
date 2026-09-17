use lazydb::value_preview::{
    DecodeStatus, MAX_PREVIEW_INPUT_BYTES, PreviewFormat, cache::CacheKey, cache::PreviewCache,
    decode::DecodedValue,
};
use std::sync::Arc;
use uuid::Uuid;

#[test]
fn decoded_preview_cache_isolated_by_source_and_format() {
    let mut cache = PreviewCache::new(128);
    let source = Uuid::from_u128(1);
    let raw = CacheKey {
        source_id: source,
        source_revision: 1,
        format: PreviewFormat::RAW,
    };
    let json = CacheKey {
        source_id: source,
        source_revision: 1,
        format: PreviewFormat::JSON,
    };
    cache.insert(raw.clone(), DecodedValue::Bytes(vec![0xff]));
    cache.insert(json.clone(), DecodedValue::Text("{}".into()));
    assert!(matches!(&*cache.get(&raw).unwrap(), DecodedValue::Bytes(_)));
    assert!(matches!(&*cache.get(&json).unwrap(), DecodedValue::Text(_)));
    assert!(!Arc::ptr_eq(
        &cache.get(&raw).unwrap(),
        &cache.get(&json).unwrap()
    ));
}

#[test]
fn serialization_decoders_reject_values_over_input_budget() {
    let value = vec![b'x'; MAX_PREVIEW_INPUT_BYTES + 1];
    assert_eq!(
        lazydb::value_preview::php::parse_php_to_json(&value)
            .unwrap_err()
            .status,
        DecodeStatus::Unsupported
    );
    assert_eq!(
        lazydb::value_preview::pickle::parse_pickle_to_json(&value)
            .unwrap_err()
            .status,
        DecodeStatus::Unsupported
    );
}

#[test]
fn yaml_detection_respects_input_budget() {
    let value = format!(
        "items:\n{}",
        (0..MAX_PREVIEW_INPUT_BYTES)
            .map(|index| format!("  - {index}\n"))
            .collect::<String>()
    );
    assert!(value.len() > MAX_PREVIEW_INPUT_BYTES);
    assert!(
        !lazydb::value_preview::detect::detect(value.as_bytes())
            .iter()
            .any(|candidate| candidate.format == PreviewFormat::YAML)
    );
}
