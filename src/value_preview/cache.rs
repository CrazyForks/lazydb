use std::collections::{HashMap, VecDeque};

use super::{PreviewFormat, decode::DecodedValue};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CacheKey {
    pub source_revision: u64,
    pub format: PreviewFormat,
}

#[derive(Debug)]
pub struct PreviewCache {
    budget: usize,
    used: usize,
    values: HashMap<CacheKey, (DecodedValue, usize)>,
    order: VecDeque<CacheKey>,
}

impl PreviewCache {
    pub fn new(budget: usize) -> Self {
        Self {
            budget,
            used: 0,
            values: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn get(&mut self, key: &CacheKey) -> Option<DecodedValue> {
        let value = self.values.get(key)?.0.clone();
        self.touch(key);
        Some(value)
    }

    pub fn insert(&mut self, key: CacheKey, value: DecodedValue) {
        let size = match &value {
            DecodedValue::Bytes(value) => value.len(),
            DecodedValue::Text(value) => value.len(),
        };
        if size > self.budget {
            return;
        }
        if let Some((_, old_size)) = self.values.remove(&key) {
            self.used = self.used.saturating_sub(old_size);
        }
        self.order.retain(|existing| existing != &key);
        while self.used + size > self.budget {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some((_, old_size)) = self.values.remove(&oldest) {
                self.used = self.used.saturating_sub(old_size);
            }
        }
        self.used += size;
        self.order.push_back(key.clone());
        self.values.insert(key, (value, size));
    }

    pub fn used(&self) -> usize {
        self.used
    }
    pub fn len(&self) -> usize {
        self.values.len()
    }

    fn touch(&mut self, key: &CacheKey) {
        self.order.retain(|existing| existing != key);
        self.order.push_back(key.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_evicts_old_values_at_byte_budget() {
        let mut cache = PreviewCache::new(3);
        cache.insert(
            CacheKey {
                source_revision: 1,
                format: PreviewFormat::RAW,
            },
            DecodedValue::Text("ab".into()),
        );
        cache.insert(
            CacheKey {
                source_revision: 2,
                format: PreviewFormat::RAW,
            },
            DecodedValue::Text("cd".into()),
        );
        assert_eq!(cache.used(), 2);
        assert_eq!(cache.len(), 1);
        assert!(
            cache
                .get(&CacheKey {
                    source_revision: 1,
                    format: PreviewFormat::RAW
                })
                .is_none()
        );
    }
}
