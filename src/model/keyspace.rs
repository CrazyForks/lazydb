use std::collections::HashSet;

use uuid::Uuid;

use crate::db::redis::types::{
    KeyScanBatch, RedisKeyId, RedisRequestIdentity, RedisTarget, ScanPosition,
};
use crate::identity::ConnectionIdentity;

pub const DEFAULT_SCAN_COUNT: u32 = 200;
pub const MAX_KEYS: usize = 10_000;
pub const MAX_KEY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeyspaceStatus {
    NotLoaded,
    Idle,
    Loading,
    Partial,
    Complete,
    CompleteEmpty,
    Paused { loaded: usize },
    Stale,
    Failed(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyspaceState {
    pub owner_id: Uuid,
    pub target: RedisTarget,
    pub pattern: Vec<u8>,
    pub generation: u64,
    pub connection: Option<ConnectionIdentity>,
    pub request_id: u64,
    pub position: ScanPosition,
    pub keys: Vec<RedisKeyId>,
    pub status: KeyspaceStatus,
    key_set: HashSet<Vec<u8>>,
    key_bytes: usize,
    in_flight: bool,
    refreshing_snapshot: bool,
    staged_keys: Vec<RedisKeyId>,
    staged_set: HashSet<Vec<u8>>,
    staged_bytes: usize,
}

impl KeyspaceState {
    pub fn new(owner_id: Uuid, target: RedisTarget, pattern: Vec<u8>) -> Self {
        Self {
            owner_id,
            target,
            pattern,
            generation: 0,
            connection: None,
            request_id: 0,
            position: ScanPosition::Start,
            keys: Vec::new(),
            status: KeyspaceStatus::NotLoaded,
            key_set: HashSet::new(),
            key_bytes: 0,
            in_flight: false,
            refreshing_snapshot: false,
            staged_keys: Vec::new(),
            staged_set: HashSet::new(),
            staged_bytes: 0,
        }
    }

    pub fn start_scan(&mut self, connection: ConnectionIdentity) -> Option<RedisRequestIdentity> {
        if self.in_flight
            || matches!(self.position, ScanPosition::Complete)
            || matches!(self.status, KeyspaceStatus::Paused { .. })
        {
            return None;
        }
        self.request_id = self.request_id.checked_add(1)?;
        self.in_flight = true;
        self.connection = Some(connection);
        self.status = KeyspaceStatus::Loading;
        self.identity()
    }

    pub fn identity(&self) -> Option<RedisRequestIdentity> {
        Some(RedisRequestIdentity {
            connection: self.connection?,
            target: self.target.clone(),
            owner_id: self.owner_id,
            generation: self.generation,
            request_id: self.request_id,
        })
    }

    pub fn apply_batch(&mut self, batch: KeyScanBatch) -> bool {
        if self.identity().as_ref() != Some(&batch.identity) || !self.in_flight {
            return false;
        }
        self.in_flight = false;
        let (base_keys, base_set, base_bytes) = if self.refreshing_snapshot {
            (&self.staged_keys, &self.staged_set, self.staged_bytes)
        } else {
            (&self.keys, &self.key_set, self.key_bytes)
        };
        let mut next_keys = base_keys.clone();
        let mut next_set = base_set.clone();
        let mut next_bytes = base_bytes;
        for key in batch.keys {
            if next_set.contains(&key) {
                continue;
            }
            if next_keys.len() >= MAX_KEYS || next_bytes.saturating_add(key.len()) > MAX_KEY_BYTES {
                self.status = KeyspaceStatus::Paused {
                    loaded: self.keys.len(),
                };
                return true;
            }
            next_bytes += key.len();
            next_set.insert(key.clone());
            next_keys.push(RedisKeyId {
                target: self.target.clone(),
                key,
            });
        }
        self.keys = next_keys;
        self.key_set = next_set;
        self.key_bytes = next_bytes;
        self.refreshing_snapshot = false;
        self.staged_keys.clear();
        self.staged_set.clear();
        self.staged_bytes = 0;
        self.position = batch.next;
        self.status = if matches!(self.position, ScanPosition::Complete) {
            if self.keys.is_empty() {
                KeyspaceStatus::CompleteEmpty
            } else {
                KeyspaceStatus::Complete
            }
        } else {
            KeyspaceStatus::Partial
        };
        true
    }

    pub fn fail(&mut self, identity: &RedisRequestIdentity, message: impl Into<String>) -> bool {
        if self.identity().as_ref() != Some(identity) || !self.in_flight {
            return false;
        }
        self.in_flight = false;
        self.status = KeyspaceStatus::Failed(message.into());
        true
    }

    pub fn refresh(&mut self) {
        let Some(generation) = self.generation.checked_add(1) else {
            self.status = KeyspaceStatus::Failed("scan generation exhausted".into());
            self.in_flight = false;
            return;
        };
        self.generation = generation;
        self.request_id = 0;
        self.position = ScanPosition::Start;
        self.refreshing_snapshot = !self.keys.is_empty()
            || matches!(
                self.status,
                KeyspaceStatus::Complete
                    | KeyspaceStatus::CompleteEmpty
                    | KeyspaceStatus::Partial
                    | KeyspaceStatus::Failed(_)
            );
        self.staged_keys.clear();
        self.staged_set.clear();
        self.staged_bytes = 0;
        self.in_flight = false;
        self.status = if self.refreshing_snapshot {
            KeyspaceStatus::Stale
        } else {
            KeyspaceStatus::NotLoaded
        };
    }
}
