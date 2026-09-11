use uuid::Uuid;

use super::{execution_target::ExecutionTarget, text_input::TextInput};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseSelectorState {
    pub connection: super::workspace::ConnectionIdentity,
    pub candidates: Vec<ExecutionTarget>,
    pub search: TextInput,
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
            search: TextInput::default(),
            selected,
            current_database: current_database.to_owned(),
        }
    }

    pub fn filtered_candidates(&self) -> Vec<(usize, &ExecutionTarget)> {
        let query = self.search.value().to_ascii_lowercase();
        self.candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                query.is_empty() || candidate.database.to_ascii_lowercase().contains(&query)
            })
            .collect()
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
        let candidates = self.filtered_candidates();
        if candidates.is_empty() {
            return;
        }
        let current = candidates
            .iter()
            .position(|(index, _)| *index == self.selected)
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(candidates.len() as isize) as usize;
        self.selected = candidates[next].0;
    }

    pub fn select_filtered(&mut self, index: usize) -> bool {
        let filtered = self.filtered_candidates();
        let Some((candidate, _)) = filtered.get(index) else {
            return false;
        };
        self.selected = *candidate;
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
    fn filters_database_names_without_changing_source_candidates() {
        let mut state = state();
        state.search.set("LOG");
        assert_eq!(
            state
                .filtered_candidates()
                .into_iter()
                .map(|(_, target)| target.database.as_str())
                .collect::<Vec<_>>(),
            vec!["moss_log"]
        );
        assert_eq!(state.candidates.len(), 3);
    }

    #[test]
    fn selection_stays_on_current_database() {
        let state = state();
        assert_eq!(state.selected, 0);
        assert!(state.is_current("moss_biz"));
    }

    #[test]
    fn filtered_selection_uses_source_identity() {
        let mut state = state();
        state.search.set("post");
        assert!(state.select_filtered(0));
        assert_eq!(
            state
                .selected_target()
                .map(|target| target.database.as_str()),
            Some("postgres")
        );
    }
}
