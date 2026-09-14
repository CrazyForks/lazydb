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
