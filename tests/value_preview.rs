use lazydb::model::redis_preview::RedisPreviewFormatState;
use lazydb::value_preview::{DecodeStatus, PreviewFormat, ValueEncoding, ValueView};

#[test]
fn manual_selection_is_not_automatic() {
    let mut state = RedisPreviewFormatState::default();
    assert!(state.automatic);
    state.select(PreviewFormat {
        encoding: ValueEncoding::Java,
        view: ValueView::Yaml,
    });
    assert!(!state.automatic);
    assert_eq!(state.encoding(), ValueEncoding::Java);
    assert_eq!(state.view(), ValueView::Yaml);
}

#[test]
fn reset_returns_to_raw_auto_mode() {
    let mut state = RedisPreviewFormatState::default();
    state.select(PreviewFormat::HEX);
    state.reset_auto();
    assert_eq!(state.selected, PreviewFormat::RAW);
    assert!(state.automatic);
}

#[test]
fn raw_and_hex_are_byte_views() {
    assert_eq!(
        lazydb::value_preview::decode::decode(b"\xff", PreviewFormat::HEX).unwrap(),
        lazydb::value_preview::decode::DecodedValue::Bytes(vec![0xff])
    );
    assert_eq!(DecodeStatus::NeedsMoreData, DecodeStatus::NeedsMoreData);
}

#[test]
fn json_and_yaml_views_format_valid_structured_text() {
    use lazydb::db::redis::read::{RedisKeyMetadata, RedisPagePosition, RedisType, TtlState};
    use lazydb::db::redis::types::{RedisKeyId, RedisTarget};
    let json = br#"{"name":"\u5f20\u4e09","enabled":true}"#.to_vec();
    let page = lazydb::db::redis::read::RedisValuePage {
        metadata: RedisKeyMetadata {
            key: RedisKeyId {
                target: RedisTarget {
                    profile_id: uuid::Uuid::nil(),
                    database: 0,
                },
                key: b"key".to_vec(),
            },
            value_type: RedisType::String,
            ttl: TtlState::Persistent,
            memory_usage_bytes: None,
            value_size: None,
        },
        position: RedisPagePosition::Complete,
        value: lazydb::db::redis::read::RedisPageValue::String(json),
        truncated: false,
        complete: true,
        raw_bytes: 0,
        formatted_bytes: 0,
    };
    let formatted = lazydb::ui::redis_value::format_page(&page, ValueView::Json).unwrap();
    assert!(formatted.contains("\n") && formatted.contains("name"));
    let yaml = lazydb::ui::redis_value::format_page(&page, ValueView::Yaml).unwrap();
    assert!(yaml.contains("name:"));
}

#[test]
fn redis_table_keeps_collection_columns_and_raw_identity() {
    let value =
        lazydb::db::redis::read::RedisPageValue::Hash(vec![(b"field".to_vec(), b"value".to_vec())]);
    let table = lazydb::value_preview::table::from_page(&value);
    assert_eq!(table.columns, vec!["Field", "Value"]);
    assert_eq!(table.rows[0].cells, vec!["field", "value"]);
    assert_eq!(table.rows[0].identity[0], b"field");
}
