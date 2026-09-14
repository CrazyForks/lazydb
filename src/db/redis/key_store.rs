use std::collections::BTreeSet;

use super::types::{RedisKeyId, RedisTarget};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyPage {
    pub keys: Vec<RedisKeyId>,
    pub next_after: Option<Vec<u8>>,
    pub complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreePageEntry {
    pub parent: Vec<u8>,
    pub name: Vec<u8>,
    pub path: Vec<u8>,
    pub is_leaf: bool,
    pub total_keys: usize,
}

pub trait KeyStore {
    fn insert_batch(&mut self, keys: &[Vec<u8>]) -> usize;
    fn remove(&mut self, key: &[u8]) -> bool;
    fn page_after(&self, after: Option<&[u8]>, limit: usize) -> KeyPage;
    fn search_page(&self, contains: &[u8], after: Option<&[u8]>, limit: usize) -> KeyPage;
    fn tree_page(
        &self,
        parent: &[u8],
        after_name: Option<&[u8]>,
        limit: usize,
    ) -> Vec<TreePageEntry>;
    fn count(&self) -> usize;
    fn bytes(&self) -> usize;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryKeyStore {
    target: RedisTarget,
    keys: BTreeSet<Vec<u8>>,
    bytes: usize,
}

impl MemoryKeyStore {
    pub fn new(target: RedisTarget) -> Self {
        Self {
            target,
            keys: BTreeSet::new(),
            bytes: 0,
        }
    }
}

impl KeyStore for MemoryKeyStore {
    fn insert_batch(&mut self, keys: &[Vec<u8>]) -> usize {
        let mut inserted = 0;
        for key in keys {
            if self.keys.insert(key.clone()) {
                self.bytes += key.len();
                inserted += 1;
            }
        }
        inserted
    }

    fn remove(&mut self, key: &[u8]) -> bool {
        if self.keys.remove(key) {
            self.bytes = self.bytes.saturating_sub(key.len());
            true
        } else {
            false
        }
    }

    fn page_after(&self, after: Option<&[u8]>, limit: usize) -> KeyPage {
        let keys = self
            .keys
            .iter()
            .filter(|key| after.is_none_or(|value| key.as_slice() > value))
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_after = keys.last().cloned();
        let complete = next_after.as_ref().is_none_or(|last| {
            self.keys
                .iter()
                .skip_while(|key| *key != last)
                .nth(1)
                .is_none()
        });
        KeyPage {
            keys: keys
                .into_iter()
                .map(|key| RedisKeyId {
                    target: self.target.clone(),
                    key,
                })
                .collect(),
            next_after,
            complete,
        }
    }

    fn search_page(&self, contains: &[u8], after: Option<&[u8]>, limit: usize) -> KeyPage {
        if contains.is_empty() {
            return self.page_after(after, limit);
        }
        let mut page = self.page_after(after, usize::MAX);
        page.keys.retain(|key| {
            key.key
                .windows(contains.len())
                .any(|window| window == contains)
        });
        page.keys.truncate(limit);
        page.next_after = page.keys.last().map(|key| key.key.clone());
        page.complete = page.next_after.is_none() || page.complete;
        page
    }

    fn tree_page(
        &self,
        parent: &[u8],
        after_name: Option<&[u8]>,
        limit: usize,
    ) -> Vec<TreePageEntry> {
        let mut entries = BTreeSet::new();
        for key in &self.keys {
            if !key.starts_with(parent) {
                continue;
            }
            let rest = &key[parent.len()..];
            let Some(separator) = rest.iter().position(|byte| *byte == b':') else {
                entries.insert((rest.to_vec(), key.clone(), true));
                continue;
            };
            let name = &rest[..separator];
            let path = [parent, &rest[..=separator]].concat();
            entries.insert((name.to_vec(), path, false));
        }
        entries
            .into_iter()
            .filter(|(name, _, _)| after_name.is_none_or(|after| name.as_slice() > after))
            .take(limit)
            .map(|(name, path, is_leaf)| TreePageEntry {
                parent: parent.to_vec(),
                name,
                path,
                is_leaf,
                total_keys: 0,
            })
            .collect()
    }

    fn count(&self) -> usize {
        self.keys.len()
    }
    fn bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MemoryKeyStore {
        MemoryKeyStore::new(RedisTarget {
            profile_id: uuid::Uuid::nil(),
            database: 0,
        })
    }

    #[test]
    fn insertion_is_binary_safe_and_deduplicated() {
        let mut store = store();
        assert_eq!(
            store.insert_batch(&[b"b".to_vec(), vec![0, 1], b"b".to_vec()]),
            2
        );
        assert_eq!(store.count(), 2);
        assert_eq!(store.bytes(), 3);
        assert_eq!(store.page_after(None, 2).keys[0].key, vec![0, 1]);
    }

    #[test]
    fn keyset_pages_continue_after_last_key() {
        let mut store = store();
        store.insert_batch(&[b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
        let first = store.page_after(None, 2);
        let second = store.page_after(first.next_after.as_deref(), 2);
        assert_eq!(
            second
                .keys
                .iter()
                .map(|key| key.key.as_slice())
                .collect::<Vec<_>>(),
            vec![b"c".as_slice()]
        );
        assert!(second.complete);
    }
}
