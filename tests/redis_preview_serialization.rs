use lazydb::db::redis::read::{
    RedisKeyMetadata, RedisPagePosition, RedisPageValue, RedisType, RedisValuePage, TtlState,
};
use lazydb::db::redis::types::{RedisKeyId, RedisTarget};
use lazydb::ui::redis_value::{format_bytes_value, format_page};
use lazydb::value_preview::{PreviewFormat, ValueEncoding, ValueView};
use uuid::Uuid;

fn string_page(value: Vec<u8>) -> RedisValuePage {
    RedisValuePage {
        metadata: RedisKeyMetadata {
            key: RedisKeyId {
                target: RedisTarget {
                    profile_id: Uuid::nil(),
                    database: 0,
                },
                key: b"serialized".to_vec(),
            },
            value_type: RedisType::String,
            ttl: TtlState::Persistent,
            memory_usage_bytes: None,
            value_size: None,
        },
        position: RedisPagePosition::Complete,
        value: RedisPageValue::String(value),
        truncated: false,
        complete: true,
        raw_bytes: 0,
        formatted_bytes: 0,
    }
}

#[test]
fn main_page_and_cell_detail_share_serialization_decoder() {
    let value = b"a:1:{i:0;s:5:\"hello\";}".to_vec();
    let format = PreviewFormat::preset(ValueEncoding::Php);
    let page = string_page(value.clone());
    let page_text = format_page(&page, format).unwrap();
    let cell_text = format_bytes_value(&value, format).unwrap();
    assert_eq!(page_text, cell_text);
    assert!(page_text.contains("hello"));
}

#[test]
fn raw_and_hex_bypass_serialization_decoder() {
    let page = string_page(vec![0xac, 0xed, 0x00, 0x05]);
    assert_eq!(
        format_page(
            &page,
            PreviewFormat {
                encoding: ValueEncoding::Java,
                view: ValueView::Hex,
            },
        )
        .unwrap(),
        "aced0005"
    );
}
