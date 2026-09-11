use uuid::Uuid;

use super::execution_target::ExecutionTarget;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseSelectorState {
    pub connection: super::workspace::ConnectionIdentity,
    pub candidates: Vec<ExecutionTarget>,
    pub selected: usize,
    pub current_database: String,
}

impl DatabaseSelectorState {
    pub fn new(
        connection: super::workspace::ConnectionIdentity,
        candidates: Vec<ExecutionTarget>,
        current_database: &str,
    ) -> Self {
        let selected = candidates
            .iter()
            .position(|candidate| candidate.database == current_database)
            .unwrap_or(0);
        Self {
            connection,
            candidates,
            selected,
            current_database: current_database.to_owned(),
        }
    }

    pub fn selected_target(&self) -> Option<&ExecutionTarget> {
        self.candidates.get(self.selected)
    }

    pub fn replace_candidates(&mut self, candidates: Vec<ExecutionTarget>, current_database: &str) {
        let selected_database = self
            .selected_target()
            .map(|target| target.database.clone())
            .unwrap_or_default();
        self.candidates = candidates;
        self.current_database = current_database.to_owned();
        self.selected = self
            .candidates
            .iter()
            .position(|candidate| candidate.database == selected_database)
            .or_else(|| {
                self.candidates
                    .iter()
                    .position(|candidate| candidate.database == current_database)
            })
            .unwrap_or(0);
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.candidates.is_empty() {
            return;
        }
        self.selected =
            (self.selected as isize + delta).rem_euclid(self.candidates.len() as isize) as usize;
    }

    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.candidates.len() {
            return false;
        }
        self.selected = index;
        true
    }

    pub fn is_current(&self, database: &str) -> bool {
        self.current_database == database
    }

    pub fn profile_id(&self) -> Uuid {
        self.connection.profile_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(database: &str) -> ExecutionTarget {
        ExecutionTarget {
            profile_id: Uuid::nil(),
            database: database.to_owned(),
            schema: None,
        }
    }

    fn state() -> DatabaseSelectorState {
        DatabaseSelectorState::new(
            super::super::workspace::ConnectionIdentity {
                profile_id: Uuid::nil(),
                generation: 1,
            },
            vec![target("moss_biz"), target("moss_log"), target("postgres")],
            "moss_biz",
        )
    }

    #[test]
    fn selection_stays_on_current_database() {
        let state = state();
        assert_eq!(state.selected, 0);
        assert!(state.is_current("moss_biz"));
    }

    #[test]
    fn selection_uses_source_identity() {
        let mut state = state();
        assert!(state.select(2));
        assert_eq!(
            state
                .selected_target()
                .map(|target| target.database.as_str()),
            Some("postgres")
        );
    }
}
