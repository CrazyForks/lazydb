use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::identity::ConnectionIdentity;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum PrincipalKind {
    User,
    Role,
}

/// Render-only classification for the explorer tree, including the container group.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrincipalDisplayKind {
    Group,
    User,
    Role,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum PrincipalScope {
    Cluster,
    Server,
    Database(String),
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct PrincipalId {
    pub profile_id: Uuid,
    pub scope: PrincipalScope,
    pub native_id: String,
    /// MySQL/MariaDB accounts are identified by `user` + `host`; the host is
    /// kept as its own field so it is never re-parsed out of a display string.
    #[serde(default)]
    pub host: Option<String>,
}

impl PrincipalId {
    pub fn connection(&self, generation: u64) -> ConnectionIdentity {
        ConnectionIdentity {
            profile_id: self.profile_id,
            generation,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct PrincipalEntry {
    pub id: PrincipalId,
    pub kind: PrincipalKind,
    pub name: String,
    pub native_kind: String,
    pub system: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalPage {
    pub connection: ConnectionIdentity,
    pub entries: Vec<PrincipalEntry>,
    pub complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalDdl {
    pub principal: PrincipalEntry,
    pub sql: String,
}
