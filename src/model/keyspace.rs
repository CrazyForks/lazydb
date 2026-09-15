use std::collections::HashSet;

use uuid::Uuid;

use crate::db::redis::key_store::{KeyStore, MemoryKeyStore};
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
    pending_keys: Vec<Vec<u8>>,
    pending_next: Option<ScanPosition>,
    store: MemoryKeyStore,
    staged_store: MemoryKeyStore,
    deleted_in_generation: HashSet<Vec<u8>>,
}

impl KeyspaceState {
    pub fn new(owner_id: Uuid, target: RedisTarget, pattern: Vec<u8>) -> Self {
        Self {
            owner_id,
            target: target.clone(),
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
            pending_keys: Vec::new(),
            pending_next: None,
            store: MemoryKeyStore::new(target.clone()),
            staged_store: MemoryKeyStore::new(target.clone()),
            deleted_in_generation: HashSet::new(),
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
        let mut incoming = self.pending_keys.drain(..).collect::<Vec<_>>();
        incoming.extend(batch.keys);
        incoming.retain(|key| !self.deleted_in_generation.contains(key));
        self.pending_next = None;
        let mut pending = Vec::new();
        let mut index = 0;
        if self.refreshing_snapshot {
            while index < incoming.len() {
                let key = &incoming[index];
                if self.staged_set.contains(key) {
                    index += 1;
                    continue;
                }
                if self.staged_keys.len() >= MAX_KEYS
                    || self.staged_bytes.saturating_add(key.len()) > MAX_KEY_BYTES
                {
                    pending.extend(incoming.into_iter().skip(index));
                    break;
                }
                self.staged_bytes += key.len();
                self.staged_set.insert(key.clone());
                self.staged_store.insert_batch(std::slice::from_ref(key));
                self.staged_keys.push(RedisKeyId {
                    target: self.target.clone(),
                    key: key.clone(),
                });
                index += 1;
            }
        } else {
            while index < incoming.len() {
                let key = &incoming[index];
                if self.key_set.contains(key) {
                    index += 1;
                    continue;
                }
                if self.keys.len() >= MAX_KEYS
                    || self.key_bytes.saturating_add(key.len()) > MAX_KEY_BYTES
                {
                    pending.extend(incoming.into_iter().skip(index));
                    break;
                }
                self.key_bytes += key.len();
                self.key_set.insert(key.clone());
                self.store.insert_batch(std::slice::from_ref(key));
                self.keys.push(RedisKeyId {
                    target: self.target.clone(),
                    key: key.clone(),
                });
                index += 1;
            }
        }
        if !pending.is_empty() {
            self.pending_keys = pending;
            self.position = batch.next.clone();
            self.pending_next = Some(batch.next);
            self.status = KeyspaceStatus::Paused {
                loaded: self.keys.len(),
            };
            return true;
        }
        if self.refreshing_snapshot {
            self.keys = std::mem::take(&mut self.staged_keys);
            self.key_set = std::mem::take(&mut self.staged_set);
            self.key_bytes = self.staged_bytes;
            self.store = std::mem::replace(
                &mut self.staged_store,
                MemoryKeyStore::new(self.target.clone()),
            );
        }
        self.refreshing_snapshot = false;
        self.staged_keys.clear();
        self.staged_set.clear();
        self.staged_bytes = 0;
        self.staged_store = MemoryKeyStore::new(self.target.clone());
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

    pub fn pending_keys(&self) -> &[Vec<u8>] {
        &self.pending_keys
    }

    pub fn pending_position(&self) -> Option<&ScanPosition> {
        self.pending_next.as_ref()
    }

    pub fn stored_count(&self) -> usize {
        self.store.count()
    }

    pub fn stored_bytes(&self) -> usize {
        self.store.bytes()
    }

    pub fn is_refreshing_snapshot(&self) -> bool {
        self.refreshing_snapshot
    }

    pub fn remove_key(&mut self, key: &[u8]) -> bool {
        self.deleted_in_generation.insert(key.to_vec());
        let Some(index) = self.keys.iter().position(|item| item.key == key) else {
            self.staged_keys.retain(|item| item.key != key);
            self.staged_set.remove(key);
            self.staged_store.remove(key);
            self.pending_keys.retain(|item| item.as_slice() != key);
            return false;
        };
        self.keys.remove(index);
        self.key_set.remove(key);
        self.key_bytes = self.key_bytes.saturating_sub(key.len());
        self.store.remove(key);
        self.staged_keys.retain(|item| item.key != key);
        self.staged_set.remove(key);
        self.staged_bytes = self.staged_bytes.saturating_sub(key.len());
        self.staged_store.remove(key);
        self.pending_keys.retain(|item| item.as_slice() != key);
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
        self.deleted_in_generation.clear();
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
        self.pending_keys.clear();
        self.pending_next = None;
        self.store = MemoryKeyStore::new(self.target.clone());
        self.staged_store = MemoryKeyStore::new(self.target.clone());
        self.in_flight = false;
        self.status = if self.refreshing_snapshot {
            KeyspaceStatus::Stale
        } else {
            KeyspaceStatus::NotLoaded
        };
    }
}
