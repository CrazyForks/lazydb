use uuid::Uuid;

use crate::db::principal::{
    PrincipalDdl, PrincipalDetails, PrincipalEntry, PrincipalPage, PrincipalReadTarget,
};
use crate::db::principal::{
    PrincipalMutationDraft, PrincipalMutationSection, PrincipalMutationTarget,
};
use crate::identity::ConnectionIdentity;
use crate::model::text_input::TextInput;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrincipalFindPhase {
    Editing,
    Confirmed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalAccessFind {
    pub section: PrincipalAccessSection,
    pub phase: PrincipalFindPhase,
    pub query: TextInput,
    pub matches: Vec<usize>,
    pub current: usize,
    pub original_selection: usize,
    pub original_offset: usize,
}

impl PrincipalAccessFind {
    pub fn position(&self) -> (usize, usize) {
        if self.matches.is_empty() {
            (0, 0)
        } else {
            (self.current + 1, self.matches.len())
        }
    }

    pub fn advance_from(&mut self, selection: usize, direction: isize) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let len = self.matches.len();
        self.current =
            if let Some(current) = self.matches.iter().position(|index| *index == selection) {
                if direction < 0 {
                    (current + len - 1) % len
                } else {
                    (current + 1) % len
                }
            } else if direction < 0 {
                self.matches
                    .iter()
                    .rposition(|index| *index < selection)
                    .unwrap_or(len - 1)
            } else {
                self.matches
                    .iter()
                    .position(|index| *index > selection)
                    .unwrap_or(0)
            };
        self.matches.get(self.current).copied()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PrincipalView {
    #[default]
    Overview,
    Ddl,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PrincipalAccessSection {
    #[default]
    Permissions,
    MemberOf,
    Members,
}

impl PrincipalAccessSection {
    pub const ALL: [Self; 3] = [Self::Permissions, Self::MemberOf, Self::Members];

    pub const fn next(self, delta: isize) -> Self {
        let index = match self {
            Self::Permissions => 0,
            Self::MemberOf => 1,
            Self::Members => 2,
        };
        Self::ALL[(index as isize + delta).rem_euclid(Self::ALL.len() as isize) as usize]
    }
}

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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PrincipalDetailsRequest {
    pub tab_id: Uuid,
    pub tab_generation: u64,
    pub request_id: u64,
    pub connection: ConnectionIdentity,
    pub entry: PrincipalEntry,
    pub target: PrincipalReadTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalMutationForm {
    pub draft: PrincipalMutationDraft,
    pub selected_field: PrincipalMutationField,
    pub tab_id: Option<Uuid>,
    pub tab_generation: Option<u64>,
    pub connection: Option<ConnectionIdentity>,
    pub principal: Option<PrincipalEntry>,
    pub database: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrincipalMutationField {
    Operation,
    Target,
    Privilege,
    GrantOption,
    Role,
    AdminOption,
}

impl PrincipalMutationForm {
    pub fn permission(target: PrincipalMutationTarget) -> Self {
        Self {
            draft: PrincipalMutationDraft::permission(target),
            selected_field: PrincipalMutationField::Operation,
            tab_id: None,
            tab_generation: None,
            connection: None,
            principal: None,
            database: None,
        }
    }
    pub fn membership(role: impl Into<String>) -> Self {
        Self {
            draft: PrincipalMutationDraft::membership(role),
            selected_field: PrincipalMutationField::Operation,
            tab_id: None,
            tab_generation: None,
            connection: None,
            principal: None,
            database: None,
        }
    }
    pub fn toggle_operation(&mut self) {
        self.draft.grant = !self.draft.grant;
        if !self.draft.grant {
            self.draft.grant_option = false;
        }
    }
    pub fn toggle_option(&mut self) {
        if self.draft.section == PrincipalMutationSection::Membership {
            self.draft.admin_option = !self.draft.admin_option;
        } else {
            self.draft.grant_option = !self.draft.grant_option;
        }
    }
    pub fn set_privilege(&mut self, privilege: impl Into<String>) {
        self.draft.set_privilege(privilege);
    }
    pub fn set_target(&mut self, target: PrincipalMutationTarget) {
        self.draft.set_target(target);
    }
    pub fn set_role(&mut self, role: impl Into<String>) {
        self.draft.set_role(role);
    }
    pub fn next_field(&mut self) {
        self.selected_field = match self.selected_field {
            PrincipalMutationField::Operation => PrincipalMutationField::Target,
            PrincipalMutationField::Target => PrincipalMutationField::Privilege,
            PrincipalMutationField::Privilege => PrincipalMutationField::GrantOption,
            PrincipalMutationField::GrantOption => PrincipalMutationField::Role,
            PrincipalMutationField::Role => PrincipalMutationField::AdminOption,
            PrincipalMutationField::AdminOption => PrincipalMutationField::Operation,
        };
    }

    pub fn next_permission_field(&mut self) {
        self.selected_field = match self.selected_field {
            PrincipalMutationField::Operation => PrincipalMutationField::GrantOption,
            _ => PrincipalMutationField::Operation,
        };
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrincipalDetailsLoad {
    Empty,
    Loading {
        request: PrincipalDetailsRequest,
        previous: Option<PrincipalDetails>,
    },
    Ready(PrincipalDetails),
    Failed {
        request: PrincipalDetailsRequest,
        message: String,
        previous: Option<PrincipalDetails>,
    },
}

impl PrincipalDetailsLoad {
    pub fn pending_request(&self) -> Option<&PrincipalDetailsRequest> {
        match self {
            Self::Loading { request, .. } => Some(request),
            _ => None,
        }
    }

    pub fn snapshot(&self) -> Option<&PrincipalDetails> {
        match self {
            Self::Ready(details) => Some(details),
            Self::Loading { previous, .. } | Self::Failed { previous, .. } => previous.as_ref(),
            Self::Empty => None,
        }
    }

    pub fn status(&self) -> Option<(String, bool)> {
        match self {
            Self::Empty => Some(("Loading permissions".to_owned(), false)),
            Self::Loading { .. } => Some(("Refreshing permissions".to_owned(), false)),
            Self::Failed { message, .. } => Some((message.clone(), true)),
            Self::Ready(_) => None,
        }
    }
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
    pub view: PrincipalView,
    pub details: PrincipalDetailsLoad,
    pub selected_permission: usize,
    pub access_section: PrincipalAccessSection,
    pub permission_offset: usize,
    pub member_of_selected: usize,
    pub member_of_offset: usize,
    pub members_selected: usize,
    pub members_offset: usize,
    pub access_viewport_rows: usize,
    pub access_find: Option<PrincipalAccessFind>,
    pub mutation_draft: Option<PrincipalMutationForm>,
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
            view: PrincipalView::Overview,
            details: PrincipalDetailsLoad::Empty,
            selected_permission: 0,
            access_section: PrincipalAccessSection::default(),
            permission_offset: 0,
            member_of_selected: 0,
            member_of_offset: 0,
            members_selected: 0,
            members_offset: 0,
            access_viewport_rows: 0,
            access_find: None,
            mutation_draft: None,
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

    pub fn access_selection(&self) -> usize {
        match self.access_section {
            PrincipalAccessSection::Permissions => self.selected_permission,
            PrincipalAccessSection::MemberOf => self.member_of_selected,
            PrincipalAccessSection::Members => self.members_selected,
        }
    }

    pub fn access_offset(&self) -> usize {
        match self.access_section {
            PrincipalAccessSection::Permissions => self.permission_offset,
            PrincipalAccessSection::MemberOf => self.member_of_offset,
            PrincipalAccessSection::Members => self.members_offset,
        }
    }

    pub fn set_access_selection(&mut self, value: usize) {
        match self.access_section {
            PrincipalAccessSection::Permissions => self.selected_permission = value,
            PrincipalAccessSection::MemberOf => self.member_of_selected = value,
            PrincipalAccessSection::Members => self.members_selected = value,
        }
    }

    pub fn set_access_offset(&mut self, value: usize) {
        match self.access_section {
            PrincipalAccessSection::Permissions => self.permission_offset = value,
            PrincipalAccessSection::MemberOf => self.member_of_offset = value,
            PrincipalAccessSection::Members => self.members_offset = value,
        }
    }

    pub fn normalize_access_state(&mut self, count: usize, visible_rows: usize) {
        if count == 0 {
            self.set_access_selection(0);
            self.set_access_offset(0);
            return;
        }

        let selected = self.access_selection().min(count - 1);
        let visible_rows = visible_rows.min(count);
        let max_offset = count.saturating_sub(visible_rows);
        let mut offset = self.access_offset().min(max_offset);
        if visible_rows > 0 {
            if selected < offset {
                offset = selected;
            } else if selected >= offset.saturating_add(visible_rows) {
                offset = selected.saturating_sub(visible_rows - 1).min(max_offset);
            }
        }
        self.set_access_selection(selected);
        self.set_access_offset(offset);
    }

    pub fn move_access_selection(&mut self, delta: isize, count: usize, visible_rows: usize) {
        if count == 0 {
            self.normalize_access_state(count, visible_rows);
            return;
        }
        let selected = self
            .access_selection()
            .min(count - 1)
            .saturating_add_signed(delta)
            .min(count - 1);
        self.set_access_selection(selected);
        self.normalize_access_state(count, visible_rows);
    }

    pub fn select_access_target(&mut self, last: bool, count: usize, visible_rows: usize) {
        let selected = if last { count.saturating_sub(1) } else { 0 };
        self.set_access_selection(selected);
        self.normalize_access_state(count, visible_rows);
    }

    pub fn scroll_access(&mut self, delta: isize, count: usize, visible_rows: usize) {
        self.normalize_access_state(count, visible_rows);
        if count == 0 || visible_rows == 0 {
            return;
        }
        let max_offset = count.saturating_sub(visible_rows);
        let offset = self
            .access_offset()
            .saturating_add_signed(delta)
            .min(max_offset);
        self.set_access_offset(offset);
        let selected = self
            .access_selection()
            .clamp(offset, offset + visible_rows - 1);
        self.set_access_selection(selected);
    }

    pub fn set_access_scroll_offset(&mut self, offset: usize, count: usize, visible_rows: usize) {
        if count == 0 {
            self.normalize_access_state(count, visible_rows);
            return;
        }
        let visible_rows = visible_rows.min(count);
        if visible_rows == 0 {
            self.normalize_access_state(count, visible_rows);
            return;
        }
        let offset = offset.min(count.saturating_sub(visible_rows));
        self.set_access_offset(offset);
        self.set_access_selection(
            self.access_selection()
                .clamp(offset, offset + visible_rows - 1),
        );
    }

    pub fn page_access(
        &mut self,
        direction: isize,
        half_page: bool,
        count: usize,
        visible_rows: usize,
    ) {
        if visible_rows == 0 || count == 0 || direction == 0 {
            return;
        }
        let page = if half_page {
            (visible_rows / 2).max(1)
        } else {
            visible_rows
        };
        let delta = if direction.is_negative() {
            -(page.min(isize::MAX as usize) as isize)
        } else {
            page.min(isize::MAX as usize) as isize
        };
        self.move_access_selection(delta, count, visible_rows);
    }

    pub fn clamp_access_state(&mut self, details: &PrincipalDetails) {
        self.selected_permission = self
            .selected_permission
            .min(details.permissions.len().saturating_sub(1));
        self.member_of_selected = self
            .member_of_selected
            .min(details.member_of.len().saturating_sub(1));
        self.members_selected = self
            .members_selected
            .min(details.members.len().saturating_sub(1));
        self.permission_offset = self
            .permission_offset
            .min(details.permissions.len().saturating_sub(1));
        self.member_of_offset = self
            .member_of_offset
            .min(details.member_of.len().saturating_sub(1));
        self.members_offset = self
            .members_offset
            .min(details.members.len().saturating_sub(1));
    }

    pub fn allocate_details_request(
        &mut self,
        connection: ConnectionIdentity,
        database: Option<String>,
    ) -> Option<PrincipalDetailsRequest> {
        let request_id = self.next_request_id.checked_add(1)?;
        self.next_request_id = request_id;
        Some(PrincipalDetailsRequest {
            tab_id: self.id,
            tab_generation: self.generation,
            request_id,
            connection,
            entry: self.entry.clone(),
            target: PrincipalReadTarget {
                principal: self.entry.id.clone(),
                database,
            },
        })
    }

    pub fn begin_details_load(&mut self, request: PrincipalDetailsRequest) {
        let previous = match std::mem::replace(&mut self.details, PrincipalDetailsLoad::Empty) {
            PrincipalDetailsLoad::Ready(details) => Some(details),
            PrincipalDetailsLoad::Loading { previous, .. }
            | PrincipalDetailsLoad::Failed { previous, .. } => previous,
            PrincipalDetailsLoad::Empty => None,
        };
        self.details = PrincipalDetailsLoad::Loading { request, previous };
    }

    pub fn apply_details_success(
        &mut self,
        request: &PrincipalDetailsRequest,
        details: PrincipalDetails,
    ) -> bool {
        if self.details.pending_request() != Some(request) {
            return false;
        }
        self.details = PrincipalDetailsLoad::Ready(details);
        true
    }

    pub fn apply_details_failure(
        &mut self,
        request: &PrincipalDetailsRequest,
        message: String,
    ) -> bool {
        if self.details.pending_request() != Some(request) {
            return false;
        }
        let previous = match std::mem::replace(&mut self.details, PrincipalDetailsLoad::Empty) {
            PrincipalDetailsLoad::Loading { previous, .. } => previous,
            other => {
                self.details = other;
                return false;
            }
        };
        self.details = PrincipalDetailsLoad::Failed {
            request: request.clone(),
            message,
            previous,
        };
        true
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
