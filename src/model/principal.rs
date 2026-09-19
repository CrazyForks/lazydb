use uuid::Uuid;

use crate::db::principal::{PrincipalDdl, PrincipalEntry, PrincipalPage};
use crate::identity::ConnectionIdentity;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalListRequest {
    pub profile_id: Uuid,
    pub generation: u64,
    pub request_id: u64,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PrincipalDdlRequest {
    pub tab_id: Uuid,
    pub tab_generation: u64,
    pub request_id: u64,
    pub connection: ConnectionIdentity,
    pub entry: PrincipalEntry,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrincipalDdlSnapshot {
    pub sql: String,
    pub connection: ConnectionIdentity,
}

impl PrincipalDdlSnapshot {
    pub fn new(ddl: PrincipalDdl, connection: ConnectionIdentity) -> Self {
        Self {
            sql: ddl.sql,
            connection,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrincipalDdlLoad {
    Empty,
    Loading {
        request: PrincipalDdlRequest,
        previous: Option<PrincipalDdlSnapshot>,
    },
    Ready(PrincipalDdlSnapshot),
    Failed {
        request: PrincipalDdlRequest,
        message: String,
        previous: Option<PrincipalDdlSnapshot>,
    },
    Cancelled {
        previous: Option<PrincipalDdlSnapshot>,
    },
}

impl PrincipalDdlLoad {
    pub fn pending_request(&self) -> Option<&PrincipalDdlRequest> {
        match self {
            Self::Loading { request, .. } => Some(request),
            _ => None,
        }
    }

    pub fn snapshot(&self) -> Option<&PrincipalDdlSnapshot> {
        match self {
            Self::Ready(snapshot) => Some(snapshot),
            Self::Loading { previous, .. }
            | Self::Failed { previous, .. }
            | Self::Cancelled { previous } => previous.as_ref(),
            Self::Empty => None,
        }
    }

    pub fn status(&self) -> Option<(String, bool)> {
        match self {
            Self::Empty => Some(("Loading DDL".to_owned(), false)),
            Self::Loading { .. } => Some(("Refreshing".to_owned(), false)),
            Self::Failed { message, .. } => Some((message.clone(), true)),
            Self::Cancelled { .. } => Some(("Cancelled".to_owned(), true)),
            Self::Ready(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrincipalDdlTab {
    pub id: Uuid,
    pub editor_id: Uuid,
    pub generation: u64,
    pub next_request_id: u64,
    pub entry: PrincipalEntry,
    pub load: PrincipalDdlLoad,
}

impl PrincipalDdlTab {
    pub fn new(entry: PrincipalEntry) -> Self {
        Self {
            id: Uuid::new_v4(),
            editor_id: Uuid::new_v4(),
            generation: 0,
            next_request_id: 0,
            entry,
            load: PrincipalDdlLoad::Empty,
        }
    }

    pub fn connection(&self, generation: u64) -> ConnectionIdentity {
        self.entry.id.connection(generation)
    }

    pub fn allocate_request(
        &mut self,
        connection: ConnectionIdentity,
    ) -> Option<PrincipalDdlRequest> {
        let request_id = self.next_request_id.checked_add(1)?;
        self.next_request_id = request_id;
        Some(PrincipalDdlRequest {
            tab_id: self.id,
            tab_generation: self.generation,
            request_id,
            connection,
            entry: self.entry.clone(),
        })
    }

    pub fn title(&self) -> &str {
        &self.entry.name
    }

    pub fn begin_load(&mut self, request: PrincipalDdlRequest) {
        let previous = match std::mem::replace(&mut self.load, PrincipalDdlLoad::Empty) {
            PrincipalDdlLoad::Ready(snapshot) => Some(snapshot),
            PrincipalDdlLoad::Loading { previous, .. }
            | PrincipalDdlLoad::Failed { previous, .. }
            | PrincipalDdlLoad::Cancelled { previous } => previous,
            PrincipalDdlLoad::Empty => None,
        };
        self.load = PrincipalDdlLoad::Loading { request, previous };
    }

    pub fn apply_success(
        &mut self,
        request: &PrincipalDdlRequest,
        ddl: PrincipalDdl,
    ) -> Option<String> {
        if self.load.pending_request() != Some(request) {
            return None;
        }
        let snapshot = PrincipalDdlSnapshot::new(ddl, request.connection);
        let sql = snapshot.sql.clone();
        self.load = PrincipalDdlLoad::Ready(snapshot);
        Some(sql)
    }

    pub fn apply_failure(&mut self, request: &PrincipalDdlRequest, message: String) -> bool {
        if self.load.pending_request() != Some(request) {
            return false;
        }
        let previous = match std::mem::replace(&mut self.load, PrincipalDdlLoad::Empty) {
            PrincipalDdlLoad::Loading { previous, .. } => previous,
            other => {
                self.load = other;
                return false;
            }
        };
        self.load = PrincipalDdlLoad::Failed {
            request: request.clone(),
            message,
            previous,
        };
        true
    }

    pub fn cancel(&mut self) -> Option<PrincipalDdlRequest> {
        let request = self.load.pending_request().cloned()?;
        let previous = match std::mem::replace(&mut self.load, PrincipalDdlLoad::Empty) {
            PrincipalDdlLoad::Loading { previous, .. } => previous,
            other => {
                self.load = other;
                return None;
            }
        };
        self.load = PrincipalDdlLoad::Cancelled { previous };
        Some(request)
    }

    pub fn invalidate(&mut self) {
        self.generation = self.generation.saturating_add(1);
        let previous = match std::mem::replace(&mut self.load, PrincipalDdlLoad::Empty) {
            PrincipalDdlLoad::Ready(snapshot) => Some(snapshot),
            PrincipalDdlLoad::Loading { previous, .. }
            | PrincipalDdlLoad::Failed { previous, .. }
            | PrincipalDdlLoad::Cancelled { previous } => previous,
            PrincipalDdlLoad::Empty => None,
        };
        self.load = PrincipalDdlLoad::Failed {
            request: PrincipalDdlRequest {
                tab_id: self.id,
                tab_generation: self.generation,
                request_id: self.next_request_id,
                connection: self.entry.id.connection(0),
                entry: self.entry.clone(),
            },
            message: "Catalog changed; refresh to reload".to_owned(),
            previous,
        };
    }

    /// Reset for a fresh connection.
    ///
    /// Bumping the generation makes any in-flight response for the previous
    /// connection stale, and clearing the load state lets the caller issue a
    /// brand-new request.
    pub fn invalidate_for_reconnect(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.load = PrincipalDdlLoad::Empty;
    }

    pub fn set_page(&mut self, page: &PrincipalPage) {
        if let Some(entry) = page.entries.iter().find(|entry| entry.id == self.entry.id) {
            self.entry = entry.clone();
        }
    }
}
