use lazydb::{
    db::redis::types::{KeyScanBatch, RedisTarget, ScanPosition},
    identity::ConnectionIdentity,
    model::keyspace::{KeyspaceState, KeyspaceStatus},
};
use uuid::Uuid;

fn state() -> KeyspaceState {
    KeyspaceState::new(
        Uuid::from_u128(2),
        RedisTarget {
            profile_id: Uuid::from_u128(1),
            database: 0,
        },
        b"*".to_vec(),
    )
}

#[test]
fn empty_batches_advance_without_marking_scan_complete() {
    let mut state = state();
    let identity = state
        .start_scan(ConnectionIdentity {
            profile_id: Uuid::from_u128(1),
            generation: 1,
        })
        .unwrap();
    assert!(state.apply_batch(KeyScanBatch {
        identity: identity.clone(),
        keys: Vec::new(),
        next: ScanPosition::Continue(7),
    }));
    assert_eq!(state.status, KeyspaceStatus::Partial);
    assert_eq!(state.position, ScanPosition::Continue(7));
    assert!(
        state
            .start_scan(ConnectionIdentity {
                profile_id: Uuid::from_u128(1),
                generation: 1
            })
            .is_some()
    );
}

#[test]
fn duplicate_keys_are_removed_and_zero_cursor_completes() {
    let mut state = state();
    let identity = state
        .start_scan(ConnectionIdentity {
            profile_id: Uuid::from_u128(1),
            generation: 1,
        })
        .unwrap();
    assert!(state.apply_batch(KeyScanBatch {
        identity,
        keys: vec![b"a".to_vec(), b"a".to_vec(), Vec::new()],
        next: ScanPosition::Complete,
    }));
    assert_eq!(state.keys.len(), 2);
    assert_eq!(state.status, KeyspaceStatus::Complete);
}

#[test]
fn stale_identity_cannot_mutate_current_scan() {
    let mut state = state();
    let old = state
        .start_scan(ConnectionIdentity {
            profile_id: Uuid::from_u128(1),
            generation: 1,
        })
        .unwrap();
    state.refresh();
    assert!(!state.apply_batch(KeyScanBatch {
        identity: old,
        keys: vec![b"old".to_vec()],
        next: ScanPosition::Complete,
    }));
    assert!(state.keys.is_empty());
}

#[test]
fn scan_requests_are_single_flight() {
    let mut state = state();
    assert!(
        state
            .start_scan(ConnectionIdentity {
                profile_id: Uuid::from_u128(1),
                generation: 1
            })
            .is_some()
    );
    assert!(
        state
            .start_scan(ConnectionIdentity {
                profile_id: Uuid::from_u128(1),
                generation: 1
            })
            .is_none()
    );
    let identity = state.identity().unwrap();
    assert_eq!(
        identity.connection,
        ConnectionIdentity {
            profile_id: Uuid::from_u128(1),
            generation: 1
        }
    );
}

#[test]
fn refresh_keeps_old_snapshot_until_first_new_batch_succeeds() {
    let mut state = state();
    let connection = ConnectionIdentity {
        profile_id: Uuid::from_u128(1),
        generation: 1,
    };
    let initial = state.start_scan(connection).unwrap();
    state.apply_batch(KeyScanBatch {
        identity: initial,
        keys: vec![b"old".to_vec()],
        next: ScanPosition::Complete,
    });
    state.refresh();
    assert_eq!(state.keys[0].key, b"old");
    assert_eq!(state.status, KeyspaceStatus::Stale);
    let refresh = state
        .start_scan(ConnectionIdentity {
            profile_id: Uuid::from_u128(1),
            generation: 2,
        })
        .unwrap();
    state.fail(&refresh, "temporary failure");
    assert_eq!(state.keys[0].key, b"old");
}

#[test]
fn refreshing_replaces_old_snapshot_only_when_first_page_fits_budget() {
    let mut state = state();
    let initial = state
        .start_scan(ConnectionIdentity {
            profile_id: Uuid::from_u128(1),
            generation: 1,
        })
        .unwrap();
    state.apply_batch(KeyScanBatch {
        identity: initial,
        keys: vec![b"old".to_vec()],
        next: ScanPosition::Complete,
    });
    state.refresh();
    let request = state
        .start_scan(ConnectionIdentity {
            profile_id: Uuid::from_u128(1),
            generation: 2,
        })
        .unwrap();
    assert!(state.apply_batch(KeyScanBatch {
        identity: request,
        keys: vec![b"new".to_vec()],
        next: ScanPosition::Complete
    }));
    assert_eq!(
        state
            .keys
            .iter()
            .map(|key| key.key.as_slice())
            .collect::<Vec<_>>(),
        vec![b"new".as_slice()]
    );
}
