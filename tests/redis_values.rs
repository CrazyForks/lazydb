use lazydb::db::redis::{
    read::{RedisReadRequest, RedisType, TtlState},
    reply::{RedisReply, ReplyBudget},
};

#[test]
fn read_requests_enforce_preview_budgets_before_network_io() {
    let key = lazydb::db::redis::types::RedisKeyId {
        target: lazydb::db::redis::types::RedisTarget {
            profile_id: uuid::Uuid::nil(),
            database: 0,
        },
        key: b"key".to_vec(),
    };
    assert!(
        RedisReadRequest::StringRange {
            key: key.clone(),
            start: 0,
            end: 64 * 1024 - 1
        }
        .validate()
        .is_ok()
    );
    assert!(
        RedisReadRequest::StringRange {
            key: key.clone(),
            start: 0,
            end: 64 * 1024
        }
        .validate()
        .is_err()
    );
    assert!(
        RedisReadRequest::HashScan {
            key,
            cursor: 0,
            count: 201
        }
        .validate()
        .is_err()
    );
}

#[test]
fn reply_bounds_preserve_types_and_mark_truncation() {
    let reply = RedisReply::Array(vec![
        RedisReply::Bytes(b"ok".to_vec()),
        RedisReply::Integer(3),
    ]);
    let bounded = reply.bound(ReplyBudget {
        max_nodes: 10,
        max_bytes: 1,
        max_depth: 4,
    });
    assert!(bounded.truncated);
    assert_eq!(bounded.visited_nodes, 2);
    assert_eq!(
        bounded.reply,
        RedisReply::Array(vec![RedisReply::Status("<reply bytes truncated>".into())])
    );
}

#[test]
fn read_range_validation_rejects_integer_overflow() {
    let key = lazydb::db::redis::types::RedisKeyId {
        target: lazydb::db::redis::types::RedisTarget {
            profile_id: uuid::Uuid::nil(),
            database: 0,
        },
        key: b"key".to_vec(),
    };
    assert!(
        lazydb::db::redis::read::RedisReadRequest::StringRange {
            key: key.clone(),
            start: 0,
            end: u64::MAX,
        }
        .validate()
        .is_err()
    );
    assert!(
        lazydb::db::redis::read::RedisReadRequest::ListRange {
            key,
            start: 0,
            end: u64::MAX,
        }
        .validate()
        .is_err()
    );
}

#[test]
fn ttl_states_distinguish_missing_persistent_and_expiring_keys() {
    assert_ne!(TtlState::Missing, TtlState::Persistent);
    assert_eq!(RedisType::SortedSet, RedisType::SortedSet);
    assert_eq!(
        TtlState::ExpiresIn { millis: 1000 },
        TtlState::ExpiresIn { millis: 1000 }
    );
}

#[test]
fn metadata_size_metrics_are_distinct_from_page_bytes() {
    let metadata = lazydb::db::redis::read::RedisKeyMetadata {
        key: lazydb::db::redis::types::RedisKeyId {
            target: lazydb::db::redis::types::RedisTarget {
                profile_id: uuid::Uuid::nil(),
                database: 0,
            },
            key: b"key".to_vec(),
        },
        value_type: RedisType::String,
        ttl: TtlState::Persistent,
        memory_usage_bytes: Some(4096),
        value_size: Some(3),
    };
    assert_eq!(metadata.memory_usage_bytes, Some(4096));
    assert_eq!(metadata.value_size, Some(3));
    assert_ne!(metadata.memory_usage_bytes, metadata.value_size);
}
