#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisLeaveDecision {
    Save,
    Discard,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisLeavePhase {
    Idle,
    Confirming,
    Saving,
    Discarding,
    Cancelled,
    Completed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisLeaveGuard {
    pub phase: RedisLeavePhase,
    pub dirty: bool,
}

impl RedisLeaveGuard {
    pub fn new(dirty: bool) -> Self {
        Self {
            phase: if dirty {
                RedisLeavePhase::Confirming
            } else {
                RedisLeavePhase::Idle
            },
            dirty,
        }
    }

    pub fn decide(&mut self, decision: RedisLeaveDecision) -> bool {
        if !self.dirty || self.phase != RedisLeavePhase::Confirming {
            return false;
        }
        self.phase = match decision {
            RedisLeaveDecision::Save => RedisLeavePhase::Saving,
            RedisLeaveDecision::Discard => RedisLeavePhase::Discarding,
            RedisLeaveDecision::Cancel => RedisLeavePhase::Cancelled,
        };
        true
    }

    pub fn complete(&mut self) {
        self.phase = RedisLeavePhase::Completed;
        self.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_values_leave_without_confirmation() {
        let mut guard = RedisLeaveGuard::new(false);
        assert!(!guard.decide(RedisLeaveDecision::Save));
        assert_eq!(guard.phase, RedisLeavePhase::Idle);
    }

    #[test]
    fn dirty_value_requires_a_decision_and_only_completion_clears_it() {
        let mut guard = RedisLeaveGuard::new(true);
        assert!(guard.decide(RedisLeaveDecision::Cancel));
        assert_eq!(guard.phase, RedisLeavePhase::Cancelled);
        assert!(guard.dirty);

        let mut guard = RedisLeaveGuard::new(true);
        assert!(guard.decide(RedisLeaveDecision::Save));
        assert_eq!(guard.phase, RedisLeavePhase::Saving);
        assert!(guard.dirty);
        guard.complete();
        assert_eq!(guard.phase, RedisLeavePhase::Completed);
        assert!(!guard.dirty);
    }

    #[test]
    fn multiple_dirty_drafts_are_resolved_independently() {
        let mut drafts = [RedisLeaveGuard::new(true), RedisLeaveGuard::new(true)];
        assert!(drafts[0].decide(RedisLeaveDecision::Save));
        assert!(drafts[1].decide(RedisLeaveDecision::Cancel));
        drafts[0].complete();
        assert!(!drafts[0].dirty);
        assert!(drafts[1].dirty);
        assert_eq!(drafts[1].phase, RedisLeavePhase::Cancelled);
    }
}
