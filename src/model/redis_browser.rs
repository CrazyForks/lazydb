use crate::model::text_input::TextInput;
use uuid::Uuid;

use crate::db::redis::types::{RedisKeyId, RedisTarget};

use super::{keyspace::KeyspaceState, redis_key_tree::KeyTreeState};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedisPreviewState {
    Empty,
    Loading { key: RedisKeyId },
    Ready { key: RedisKeyId, content: String },
    Failed { key: RedisKeyId, message: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisBrowserFocus {
    Keys,
    Preview,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisFindPhase {
    Editing,
    Confirmed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisKeyFindState {
    pub phase: RedisFindPhase,
    pub query: TextInput,
    pub rows: Vec<(super::redis_key_tree::KeyTreeNodeId, String)>,
    pub matches: Vec<super::redis_key_tree::KeyTreeNodeId>,
    pub current: usize,
    pub original_selected: Option<super::redis_key_tree::KeyTreeNodeId>,
    pub original_scroll: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisBrowserTab {
    pub id: Uuid,
    pub target: RedisTarget,
    pub keyspace: KeyspaceState,
    pub tree: KeyTreeState,
    pub preview: RedisPreviewState,
    pub preview_generation: u64,
    pub focus: RedisBrowserFocus,
    pub find: Option<RedisKeyFindState>,
    pub scroll: usize,
    pub viewport_rows: usize,
}

impl RedisBrowserTab {
    pub fn new(id: Uuid, target: RedisTarget) -> Self {
        Self {
            id,
            keyspace: KeyspaceState::new(id, target.clone(), b"*".to_vec()),
            target,
            tree: KeyTreeState::default(),
            preview: RedisPreviewState::Empty,
            preview_generation: 0,
            focus: RedisBrowserFocus::Keys,
            find: None,
            scroll: 0,
            viewport_rows: 0,
        }
    }

    pub fn rebuild_tree(&mut self) {
        self.tree.rebuild(&self.keyspace.keys);
    }

    pub fn select(&mut self, node: Option<super::redis_key_tree::KeyTreeNodeId>) {
        self.preview_generation = self.preview_generation.saturating_add(1);
        self.tree.select(node);
        self.preview = match self.tree.selected_key() {
            Some(key) => RedisPreviewState::Loading {
                key: RedisKeyId {
                    target: self.target.clone(),
                    key: key.to_vec(),
                },
            },
            None => RedisPreviewState::Empty,
        };
    }

    pub fn preview_generation(&self) -> Option<u64> {
        (!matches!(self.preview, RedisPreviewState::Empty)).then_some(self.preview_generation)
    }

    pub fn toggle_prefix(&mut self, node: &super::redis_key_tree::KeyTreeNodeId) -> bool {
        if !matches!(node, super::redis_key_tree::KeyTreeNodeId::Prefix(_))
            || !self.tree.contains(node)
        {
            return false;
        }
        if !self.tree.expanded.insert(node.clone()) {
            self.tree.expanded.remove(node);
        }
        true
    }

    pub fn open_find(&mut self) {
        if self.find.is_some() {
            return;
        }
        let rows = self
            .tree
            .visible_ids()
            .into_iter()
            .map(|id| {
                let label = match &id {
                    super::redis_key_tree::KeyTreeNodeId::Prefix(bytes)
                    | super::redis_key_tree::KeyTreeNodeId::Key(bytes) => {
                        String::from_utf8_lossy(bytes).into_owned()
                    }
                };
                (id, label)
            })
            .collect::<Vec<_>>();
        self.find = Some(RedisKeyFindState {
            phase: RedisFindPhase::Editing,
            query: TextInput::default(),
            rows,
            matches: Vec::new(),
            current: 0,
            original_selected: self.tree.selected.clone(),
            original_scroll: self.scroll,
        });
    }

    pub fn update_find(&mut self) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let query = find.query.value().trim().to_lowercase();
        find.matches = if query.is_empty() {
            Vec::new()
        } else {
            find.rows
                .iter()
                .filter(|(_, label)| label.to_lowercase().contains(&query))
                .map(|(id, _)| id.clone())
                .collect()
        };
        find.current = 0;
        if let Some(id) = find.matches.first().cloned() {
            self.tree.select(Some(id));
        }
    }

    pub fn move_find(&mut self, delta: isize) {
        let Some(find) = self
            .find
            .as_mut()
            .filter(|find| find.phase == RedisFindPhase::Confirmed)
        else {
            return;
        };
        if find.matches.is_empty() {
            return;
        }
        find.current =
            (find.current as isize + delta).rem_euclid(find.matches.len() as isize) as usize;
        self.tree.select(find.matches.get(find.current).cloned());
    }

    pub fn confirm_find(&mut self) {
        if let Some(find) = self.find.as_mut() {
            find.phase = RedisFindPhase::Confirmed;
        }
    }

    pub fn close_find(&mut self, cancel: bool) {
        if let Some(find) = self.find.take()
            && cancel
            && find.phase == RedisFindPhase::Editing
        {
            self.tree.select(find.original_selected);
            self.scroll = find.original_scroll;
        }
    }
}
