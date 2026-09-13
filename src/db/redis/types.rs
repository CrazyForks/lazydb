use uuid::Uuid;

use crate::identity::ConnectionIdentity;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RedisTarget {
    pub profile_id: Uuid,
    pub database: u32,
}

impl RedisTarget {
    pub fn new(profile_id: Uuid, database: &str) -> Option<Self> {
        Some(Self {
            profile_id,
            database: database.parse().ok()?,
        })
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RedisKeyId {
    pub target: RedisTarget,
    pub key: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisRequestIdentity {
    pub connection: ConnectionIdentity,
    pub target: RedisTarget,
    pub owner_id: Uuid,
    pub generation: u64,
    pub request_id: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScanPosition {
    Start,
    Continue(u64),
    Complete,
}

impl ScanPosition {
    pub fn next(cursor: u64) -> Self {
        if cursor == 0 {
            Self::Complete
        } else {
            Self::Continue(cursor)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyScanRequest {
    pub identity: RedisRequestIdentity,
    pub position: ScanPosition,
    pub pattern: Vec<u8>,
    pub count_hint: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyScanBatch {
    pub identity: RedisRequestIdentity,
    pub keys: Vec<Vec<u8>>,
    pub next: ScanPosition,
}
