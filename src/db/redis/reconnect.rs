use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconnectPolicy {
    pub attempts: usize,
    pub initial_delay: Duration,
}

impl ReconnectPolicy {
    pub const fn default() -> Self {
        Self {
            attempts: 3,
            initial_delay: Duration::from_millis(250),
        }
    }

    pub const fn delay_for(&self, attempt: usize) -> Duration {
        let multiplier = if attempt > 10 { 1024 } else { 1u64 << attempt };
        self.initial_delay.saturating_mul(multiplier as u32)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionGeneration {
    pub profile_id: uuid::Uuid,
    pub generation: u64,
}

impl ConnectionGeneration {
    pub fn next(self) -> Option<Self> {
        Some(Self {
            profile_id: self.profile_id,
            generation: self.generation.checked_add(1)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_uses_bounded_exponential_delays() {
        let policy = ReconnectPolicy::default();
        assert_eq!(policy.delay_for(0), Duration::from_millis(250));
        assert_eq!(policy.delay_for(2), Duration::from_millis(1000));
        assert_eq!(policy.attempts, 3);
    }

    #[test]
    fn connection_generation_never_wraps() {
        let generation = ConnectionGeneration {
            profile_id: uuid::Uuid::nil(),
            generation: u64::MAX,
        };
        assert!(generation.next().is_none());
    }
}
