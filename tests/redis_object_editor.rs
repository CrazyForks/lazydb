use lazydb::{
    db::redis::{
        mutation::{RedisMutationMode, RedisTtlMutation},
        read::{RedisType, TtlState},
        types::{RedisKeyId, RedisTarget},
    },
    identity::ConnectionIdentity,
    model::redis_object_editor::{
        RedisEditorTtlMode, RedisObjectEditorFocus, RedisObjectEditorState, parse_bytes,
    },
};
use uuid::Uuid;

fn target() -> RedisTarget {
    RedisTarget {
        profile_id: Uuid::from_u128(91),
        database: 4,
    }
}

fn connection() -> ConnectionIdentity {
    ConnectionIdentity {
        profile_id: target().profile_id,
        generation: 2,
    }
}

#[test]
fn redis_editor_keeps_binary_input_explicit() {
    assert_eq!(parse_bytes("hex:0001ff").unwrap(), vec![0, 1, 255]);
    assert_eq!(parse_bytes("0x4142").unwrap(), b"AB");
    assert_eq!(
        parse_bytes(&lazydb::model::redis_object_editor::display_bytes(b"0xAB")).unwrap(),
        b"0xAB"
    );
    assert!(parse_bytes("hex:abc").is_err());
}

#[test]
fn redis_create_editor_builds_a_typed_hash_and_ttl() {
    let mut editor = RedisObjectEditorState::create(Uuid::from_u128(92), connection(), target());
    editor.key.set("cache:hash");
    editor.value_type = RedisType::Hash;
    editor.value.set("name\tAda\ncount\t1");
    editor.ttl_mode = RedisEditorTtlMode::Expires;
    editor.ttl.set("5000");
    let draft = editor.value_draft().unwrap();
    assert_eq!(draft.value_type(), RedisType::Hash);
    assert_eq!(
        editor.ttl_mutation().unwrap(),
        RedisTtlMutation::SetMillis(5000)
    );
    let request = {
        editor.request_id = 1;
        editor.request().unwrap()
    };
    assert_eq!(request.mode, RedisMutationMode::Create);
    assert_eq!(request.key.key, b"cache:hash");
}

#[test]
fn redis_edit_editor_preserves_type_and_cycles_focus_without_editing_the_key() {
    let key = RedisKeyId {
        target: target(),
        key: b"binary\0key".to_vec(),
    };
    let mut editor = RedisObjectEditorState::edit(
        Uuid::from_u128(93),
        connection(),
        key,
        RedisType::String,
        TtlState::Persistent,
    );
    assert_eq!(editor.focus, RedisObjectEditorFocus::Value);
    editor.move_focus(1);
    assert_eq!(editor.focus, RedisObjectEditorFocus::Ttl);
    editor.cycle_type(1);
    assert_eq!(editor.value_type, RedisType::String);
    assert!(editor.request().is_ok());
}
