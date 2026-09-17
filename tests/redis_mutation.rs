use lazydb::{
    db::redis::{
        mutation::{RedisKeyBaseline, RedisMutationMode, RedisMutationRequest, RedisValueDraft},
        read::RedisType,
        types::{RedisKeyId, RedisTarget},
    },
    identity::ConnectionIdentity,
};
use uuid::Uuid;

fn request(mode: RedisMutationMode, key: &[u8]) -> RedisMutationRequest {
    let profile_id = Uuid::from_u128(41);
    RedisMutationRequest {
        connection: ConnectionIdentity {
            profile_id,
            generation: 3,
        },
        request_id: 9,
        mode,
        key: RedisKeyId {
            target: RedisTarget {
                profile_id,
                database: 2,
            },
            key: key.to_vec(),
        },
    }
}

#[test]
fn redis_string_create_plan_is_binary_safe_and_persistent_by_default() {
    let plan = lazydb::db::redis::RedisAdapter::plan_mutation(
        request(RedisMutationMode::Create, b"binary\0key"),
        RedisValueDraft::String(vec![0, 1, 255]),
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        plan.commands().unwrap(),
        vec![
            lazydb::db::redis::mutation::RedisMutationCommand {
                name: "SET".into(),
                args: vec![b"binary\0key".to_vec(), vec![0, 1, 255]],
            },
            lazydb::db::redis::mutation::RedisMutationCommand {
                name: "PERSIST".into(),
                args: vec![b"binary\0key".to_vec()],
            }
        ]
    );
}

#[test]
fn redis_hash_edit_plan_replaces_the_value_and_sets_ttl_atomically() {
    let plan = lazydb::db::redis::RedisAdapter::plan_mutation(
        request(RedisMutationMode::Edit, b"hash"),
        RedisValueDraft::Hash(vec![(b"field".to_vec(), b"value".to_vec())]),
        Some(1_500),
        Some(RedisKeyBaseline {
            value_type: RedisType::Hash,
        }),
    )
    .unwrap();
    assert_eq!(
        plan.commands()
            .unwrap()
            .into_iter()
            .map(|command| command.name)
            .collect::<Vec<_>>(),
        vec!["DEL", "HSET", "PEXPIRE"]
    );
}

#[test]
fn redis_targeted_collection_edits_keep_other_members_and_require_expected_list_values() {
    let hash = lazydb::db::redis::RedisAdapter::plan_operation(
        request(RedisMutationMode::Edit, b"hash"),
        lazydb::db::redis::mutation::RedisMutationOperation::SetHashField {
            field: b"field".to_vec(),
            value: b"new".to_vec(),
            expected: Some(b"old".to_vec()),
        },
        lazydb::db::redis::mutation::RedisTtlMutation::Preserve,
        Some(RedisKeyBaseline {
            value_type: RedisType::Hash,
        }),
    )
    .unwrap();
    assert_eq!(hash.commands().unwrap()[0].name, "HSET");

    let list = lazydb::db::redis::RedisAdapter::plan_operation(
        request(RedisMutationMode::Edit, b"list"),
        lazydb::db::redis::mutation::RedisMutationOperation::SetListElement {
            index: 1,
            value: b"new".to_vec(),
            expected: b"old".to_vec(),
        },
        lazydb::db::redis::mutation::RedisTtlMutation::Preserve,
        Some(RedisKeyBaseline {
            value_type: RedisType::List,
        }),
    )
    .unwrap();
    assert_eq!(list.commands().unwrap()[0].name, "LSET");
}

#[test]
fn redis_string_delete_plan_uses_an_expected_value() {
    let plan = lazydb::db::redis::RedisAdapter::plan_operation(
        request(RedisMutationMode::Edit, b"string"),
        lazydb::db::redis::mutation::RedisMutationOperation::DeleteString {
            expected: b"old".to_vec(),
        },
        lazydb::db::redis::mutation::RedisTtlMutation::Preserve,
        Some(RedisKeyBaseline {
            value_type: RedisType::String,
        }),
    )
    .unwrap();
    assert_eq!(plan.commands().unwrap()[0].name, "DEL");
}

#[test]
fn redis_set_member_replace_is_atomic_and_rejects_existing_target() {
    let plan = lazydb::db::redis::RedisAdapter::plan_operation(
        request(RedisMutationMode::Edit, b"set"),
        lazydb::db::redis::mutation::RedisMutationOperation::ReplaceSetMember {
            member: b"old".to_vec(),
            replacement: b"new".to_vec(),
        },
        lazydb::db::redis::mutation::RedisTtlMutation::Preserve,
        Some(RedisKeyBaseline {
            value_type: RedisType::Set,
        }),
    )
    .unwrap();
    assert_eq!(plan.commands().unwrap()[0].name, "EVAL");
}

#[test]
fn redis_add_hash_field_and_sorted_set_member_use_conflict_safe_plans() {
    let hash = lazydb::db::redis::RedisAdapter::plan_operation(
        request(RedisMutationMode::Edit, b"hash"),
        lazydb::db::redis::mutation::RedisMutationOperation::AddHashField {
            field: b"new-field".to_vec(),
            value: b"value".to_vec(),
        },
        lazydb::db::redis::mutation::RedisTtlMutation::Preserve,
        Some(RedisKeyBaseline {
            value_type: RedisType::Hash,
        }),
    )
    .unwrap();
    assert_eq!(hash.commands().unwrap()[0].name, "HSETNX");

    let sorted_set = lazydb::db::redis::RedisAdapter::plan_operation(
        request(RedisMutationMode::Edit, b"zset"),
        lazydb::db::redis::mutation::RedisMutationOperation::AddSortedSetMember {
            member: b"new-member".to_vec(),
            score: "1.5".into(),
        },
        lazydb::db::redis::mutation::RedisTtlMutation::Preserve,
        Some(RedisKeyBaseline {
            value_type: RedisType::SortedSet,
        }),
    )
    .unwrap();
    assert_eq!(sorted_set.commands().unwrap()[0].name, "ZADD NX");
}

#[test]
fn redis_add_list_element_appends_without_replacing_existing_items() {
    let plan = lazydb::db::redis::RedisAdapter::plan_operation(
        request(RedisMutationMode::Edit, b"list"),
        lazydb::db::redis::mutation::RedisMutationOperation::AppendListElement {
            value: b"new".to_vec(),
        },
        lazydb::db::redis::mutation::RedisTtlMutation::Preserve,
        Some(RedisKeyBaseline {
            value_type: RedisType::List,
        }),
    )
    .unwrap();
    assert_eq!(plan.commands().unwrap()[0].name, "RPUSH");
}

#[test]
fn redis_edit_defaults_to_preserving_ttl_but_create_defaults_to_persistent() {
    let edit = lazydb::db::redis::RedisAdapter::plan_mutation(
        request(RedisMutationMode::Edit, b"key"),
        RedisValueDraft::String(b"new".to_vec()),
        None,
        Some(RedisKeyBaseline {
            value_type: RedisType::String,
        }),
    )
    .unwrap();
    assert!(
        edit.commands()
            .unwrap()
            .iter()
            .all(|command| command.name != "PERSIST")
    );

    let create = lazydb::db::redis::RedisAdapter::plan_mutation(
        request(RedisMutationMode::Create, b"key"),
        RedisValueDraft::String(b"new".to_vec()),
        None,
        None,
    )
    .unwrap();
    assert_eq!(create.commands().unwrap().last().unwrap().name, "PERSIST");
}

#[tokio::test]
#[ignore = "requires an isolated Redis server"]
async fn redis_mutations_round_trip_binary_values_targeted_edits_and_ttl() {
    let url = std::env::var("LAZYDB_TEST_REDIS_URL").expect("LAZYDB_TEST_REDIS_URL");
    let imported =
        lazydb::profile::import_connection_url(&url, Some("redis-mutation-test")).unwrap();
    let adapter = lazydb::db::redis::RedisAdapter::connect(&imported.profile, None)
        .await
        .unwrap();
    let database = adapter.database();
    let target = RedisTarget {
        profile_id: imported.profile.id,
        database,
    };
    let suffix = Uuid::new_v4().to_string().into_bytes();
    let string_key = [b"lazydb:mutation:string:".as_slice(), suffix.as_slice()].concat();
    let hash_key = [b"lazydb:mutation:hash:".as_slice(), suffix.as_slice()].concat();
    let cleanup = redis::Client::open(url.clone()).unwrap();
    let mut cleanup_connection = cleanup.get_multiplexed_async_connection().await.unwrap();
    let string_id = RedisKeyId {
        target: target.clone(),
        key: string_key.clone(),
    };
    let create = lazydb::db::redis::RedisAdapter::plan_mutation(
        request_for(&string_id, 1, RedisMutationMode::Create),
        RedisValueDraft::String(vec![0, 1, 255]),
        Some(60_000),
        None,
    )
    .unwrap();
    adapter.execute_mutation(&create).await.unwrap();
    let metadata = adapter.key_metadata(&string_id).await.unwrap();
    assert!(
        matches!(metadata.ttl, lazydb::db::redis::read::TtlState::ExpiresIn { millis } if millis > 0)
    );

    let edit = lazydb::db::redis::RedisAdapter::plan_operation(
        request_for(&string_id, 2, RedisMutationMode::Edit),
        lazydb::db::redis::mutation::RedisMutationOperation::SetString {
            value: b"changed".to_vec(),
            expected: Some(vec![0, 1, 255]),
        },
        lazydb::db::redis::mutation::RedisTtlMutation::Preserve,
        Some(RedisKeyBaseline {
            value_type: RedisType::String,
        }),
    )
    .unwrap();
    adapter.execute_mutation(&edit).await.unwrap();
    let value: Vec<u8> = redis::cmd("GET")
        .arg(&string_key)
        .query_async(&mut cleanup_connection)
        .await
        .unwrap();
    assert_eq!(value, b"changed");
    let metadata = adapter.key_metadata(&string_id).await.unwrap();
    assert!(
        matches!(metadata.ttl, lazydb::db::redis::read::TtlState::ExpiresIn { millis } if millis > 0)
    );

    let hash_id = RedisKeyId {
        target,
        key: hash_key.clone(),
    };
    let create_hash = lazydb::db::redis::RedisAdapter::plan_mutation(
        request_for(&hash_id, 3, RedisMutationMode::Create),
        RedisValueDraft::Hash(vec![
            (b"keep".to_vec(), b"yes".to_vec()),
            (b"edit".to_vec(), b"old".to_vec()),
        ]),
        None,
        None,
    )
    .unwrap();
    adapter.execute_mutation(&create_hash).await.unwrap();
    let edit_hash = lazydb::db::redis::RedisAdapter::plan_operation(
        request_for(&hash_id, 4, RedisMutationMode::Edit),
        lazydb::db::redis::mutation::RedisMutationOperation::SetHashField {
            field: b"edit".to_vec(),
            value: b"new".to_vec(),
            expected: Some(b"old".to_vec()),
        },
        lazydb::db::redis::mutation::RedisTtlMutation::Preserve,
        Some(RedisKeyBaseline {
            value_type: RedisType::Hash,
        }),
    )
    .unwrap();
    adapter.execute_mutation(&edit_hash).await.unwrap();
    let keep: Vec<u8> = redis::cmd("HGET")
        .arg(&hash_key)
        .arg(b"keep")
        .query_async(&mut cleanup_connection)
        .await
        .unwrap();
    assert_eq!(keep, b"yes");
    let edited: Vec<u8> = redis::cmd("HGET")
        .arg(&hash_key)
        .arg(b"edit")
        .query_async(&mut cleanup_connection)
        .await
        .unwrap();
    assert_eq!(edited, b"new");
    let _: i64 = redis::cmd("DEL")
        .arg(&string_key)
        .arg(&hash_key)
        .query_async(&mut cleanup_connection)
        .await
        .unwrap();
}

fn request_for(key: &RedisKeyId, request_id: u64, mode: RedisMutationMode) -> RedisMutationRequest {
    RedisMutationRequest {
        connection: ConnectionIdentity {
            profile_id: key.target.profile_id,
            generation: 1,
        },
        request_id,
        mode,
        key: key.clone(),
    }
}

#[test]
fn redis_edit_rejects_a_changed_native_value_type() {
    let error = lazydb::db::redis::RedisAdapter::plan_mutation(
        request(RedisMutationMode::Edit, b"key"),
        RedisValueDraft::List(vec![b"value".to_vec()]),
        None,
        Some(RedisKeyBaseline {
            value_type: RedisType::String,
        }),
    )
    .unwrap_err();
    assert!(error.to_string().contains("does not match"));
}

#[test]
fn redis_sorted_set_scores_are_validated_before_execution() {
    let error = lazydb::db::redis::RedisAdapter::plan_mutation(
        request(RedisMutationMode::Create, b"scores"),
        RedisValueDraft::SortedSet(vec![("not-a-number".into(), b"member".to_vec())]),
        None,
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("scores"));
}
