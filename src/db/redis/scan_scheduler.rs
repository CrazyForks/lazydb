use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanStorage {
    Memory,
    Indexed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanBudget {
    pub request_limit: usize,
    pub time_limit: Duration,
}

#[derive(Debug)]
pub struct ScanScheduler {
    budget: ScanBudget,
    requests: usize,
    started: Instant,
    storage: ScanStorage,
}

impl ScanScheduler {
    pub fn new(budget: ScanBudget, storage: ScanStorage) -> Self {
        Self {
            budget,
            requests: 0,
            started: Instant::now(),
            storage,
        }
    }

    pub fn storage(&self) -> ScanStorage {
        self.storage
    }
    pub fn requests(&self) -> usize {
        self.requests
    }

    pub fn should_yield(&self) -> bool {
        self.requests >= self.budget.request_limit
            || self.started.elapsed() >= self.budget.time_limit
    }

    pub fn record_request(&mut self) {
        self.requests = self.requests.saturating_add(1);
    }

    pub fn promote(&mut self) {
        self.storage = ScanStorage::Indexed;
    }
}

pub fn should_promote(
    key_count: usize,
    key_bytes: usize,
    key_limit: usize,
    byte_limit: usize,
) -> bool {
    key_count >= key_limit || key_bytes >= byte_limit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_budget_yields_after_limit() {
        let mut scheduler = ScanScheduler::new(
            ScanBudget {
                request_limit: 2,
                time_limit: Duration::from_secs(60),
            },
            ScanStorage::Memory,
        );
        assert!(!scheduler.should_yield());
        scheduler.record_request();
        scheduler.record_request();
        assert!(scheduler.should_yield());
    }

    #[test]
    fn key_or_byte_limit_promotes_to_index() {
        assert!(should_promote(10, 1, 10, 100));
        assert!(should_promote(1, 100, 10, 100));
        assert!(!should_promote(9, 99, 10, 100));
    }

    #[test]
    fn promotion_is_explicit() {
        let mut scheduler = ScanScheduler::new(
            ScanBudget {
                request_limit: 1,
                time_limit: Duration::from_secs(1),
            },
            ScanStorage::Memory,
        );
        scheduler.promote();
        assert_eq!(scheduler.storage(), ScanStorage::Indexed);
    }
}
