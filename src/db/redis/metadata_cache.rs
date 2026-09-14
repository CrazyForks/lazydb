use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::{db::redis::read::RedisKeyMetadata, identity::ConnectionIdentity};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MetadataCacheKey {
    pub connection: ConnectionIdentity,
    pub key: Vec<u8>,
}

#[derive(Debug)]
pub struct MetadataCache {
    capacity: usize,
    ttl: Duration,
    entries: HashMap<MetadataCacheKey, (Instant, RedisKeyMetadata)>,
}

impl MetadataCache {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            capacity,
            ttl,
            entries: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &MetadataCacheKey) -> Option<RedisKeyMetadata> {
        let (inserted, value) = self.entries.get(key)?;
        if inserted.elapsed() >= self.ttl {
            self.entries.remove(key);
            return None;
        }
        Some(value.clone())
    }

    pub fn insert(&mut self, key: MetadataCacheKey, value: RedisKeyMetadata) {
        if self.capacity == 0 {
            return;
        }
        if self.entries.len() >= self.capacity
            && !self.entries.contains_key(&key)
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(key, _)| key.clone())
        {
            self.entries.remove(&oldest);
        }
        self.entries.insert(key, (Instant::now(), value));
    }

    pub fn invalidate_connection(&mut self, connection: ConnectionIdentity) {
        self.entries.retain(|key, _| key.connection != connection);
    }

    pub fn invalidate_key(&mut self, connection: ConnectionIdentity, key: &[u8]) {
        self.entries.remove(&MetadataCacheKey {
            connection,
            key: key.to_vec(),
        });
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::redis::read::{RedisKeyMetadata, RedisType, TtlState};
    use crate::db::redis::types::{RedisKeyId, RedisTarget};
    use uuid::Uuid;

    fn metadata(key: &[u8]) -> RedisKeyMetadata {
        RedisKeyMetadata {
            key: RedisKeyId {
                target: RedisTarget {
                    profile_id: Uuid::nil(),
                    database: 0,
                },
                key: key.to_vec(),
            },
            value_type: RedisType::String,
            ttl: TtlState::Persistent,
            memory_usage_bytes: None,
            value_size: None,
        }
    }

    fn cache_key(key: &[u8]) -> MetadataCacheKey {
        MetadataCacheKey {
            connection: ConnectionIdentity {
                profile_id: Uuid::nil(),
                generation: 1,
            },
            key: key.to_vec(),
        }
    }

    #[test]
    fn cache_is_bounded_and_invalidated_by_connection() {
        let connection = cache_key(b"a").connection;
        let mut cache = MetadataCache::new(1, Duration::from_secs(5));
        cache.insert(cache_key(b"a"), metadata(b"a"));
        cache.insert(cache_key(b"b"), metadata(b"b"));
        assert_eq!(cache.len(), 1);
        cache.insert(cache_key(b"b"), metadata(b"b"));
        cache.invalidate_key(connection, b"b");
        assert!(cache.is_empty());
        cache.insert(cache_key(b"a"), metadata(b"a"));
        cache.invalidate_connection(connection);
        assert_eq!(cache.len(), 0);
    }
}
