use std::time::{Duration, Instant};

use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewRequest<K> {
    pub tab_id: Uuid,
    pub generation: u64,
    pub key: K,
}

#[derive(Debug)]
pub struct PreviewScheduler<K> {
    debounce: Duration,
    pending: Option<(Instant, PreviewRequest<K>)>,
    in_flight: bool,
}

impl<K> PreviewScheduler<K> {
    pub fn new(debounce: Duration) -> Self {
        Self {
            debounce,
            pending: None,
            in_flight: false,
        }
    }

    pub fn select(&mut self, request: PreviewRequest<K>) {
        self.pending = Some((Instant::now(), request));
    }

    pub fn mark_in_flight(&mut self) {
        self.in_flight = true;
    }

    pub fn mark_complete(&mut self) {
        self.in_flight = false;
    }

    pub fn take_ready(&mut self) -> Option<PreviewRequest<K>> {
        if self.in_flight {
            return None;
        }
        let (started, _) = self.pending.as_ref()?;
        if started.elapsed() < self.debounce {
            return None;
        }
        self.pending.take().map(|(_, request)| request)
    }

    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn latest_selection_replaces_pending_request() {
        let mut scheduler = PreviewScheduler::new(Duration::from_millis(1));
        scheduler.select(PreviewRequest {
            tab_id: Uuid::nil(),
            generation: 1,
            key: "a",
        });
        scheduler.select(PreviewRequest {
            tab_id: Uuid::nil(),
            generation: 2,
            key: "b",
        });
        thread::sleep(Duration::from_millis(2));
        assert_eq!(scheduler.take_ready().unwrap().key, "b");
    }

    #[test]
    fn in_flight_request_blocks_new_dispatch() {
        let mut scheduler = PreviewScheduler::new(Duration::from_millis(0));
        scheduler.select(PreviewRequest {
            tab_id: Uuid::nil(),
            generation: 1,
            key: "a",
        });
        scheduler.mark_in_flight();
        assert!(scheduler.take_ready().is_none());
        scheduler.mark_complete();
        assert_eq!(scheduler.take_ready().unwrap().key, "a");
    }
}
