use crate::model::text_input::TextInput;
use uuid::Uuid;

use crate::db::redis::read::RedisValuePage;
use crate::db::redis::types::{RedisKeyId, RedisTarget};

use super::{keyspace::KeyspaceState, redis_key_tree::KeyTreeState};

use crate::value_preview::PreviewFormat;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedisPreviewState {
    Empty,
    Loading { key: RedisKeyId },
    Ready { key: RedisKeyId, content: String },
    Failed { key: RedisKeyId, message: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedisValuePageState {
    Empty,
    Loading { key: RedisKeyId },
    Ready(RedisValuePage),
    Failed { key: RedisKeyId, message: String },
}

/// Unified value state used by the Redis preview renderer. The legacy
/// `preview` field remains during migration so older runtime events can be
/// accepted without allowing them to replace a newer typed page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedisPreviewContentState {
    Empty,
    Loading {
        key: RedisKeyId,
    },
    Ready {
        key: RedisKeyId,
        page: RedisValuePage,
        format: PreviewFormat,
    },
    Failed {
        key: RedisKeyId,
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisBrowserFocus {
    Keys,
    Preview,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisBrowserPane {
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
    pub preview_editor_id: Uuid,
    pub target: RedisTarget,
    pub keyspace: KeyspaceState,
    pub tree: KeyTreeState,
    pub preview: RedisPreviewState,
    pub value_page: RedisValuePageState,
    pub content: RedisPreviewContentState,
    pub format: crate::model::redis_preview::RedisPreviewFormatState,
    pub preview_generation: u64,
    pub focus: RedisBrowserFocus,
    pub find: Option<RedisKeyFindState>,
    pub scroll: usize,
    pub viewport_rows: usize,
    pub preview_scroll: usize,
    pub preview_viewport_rows: usize,
    pub preview_content_rows: usize,
}

impl RedisBrowserTab {
    pub fn new(id: Uuid, target: RedisTarget) -> Self {
        Self {
            id,
            preview_editor_id: Uuid::new_v4(),
            keyspace: KeyspaceState::new(id, target.clone(), b"*".to_vec()),
            target,
            tree: KeyTreeState::default(),
            preview: RedisPreviewState::Empty,
            value_page: RedisValuePageState::Empty,
            content: RedisPreviewContentState::Empty,
            format: Default::default(),
            preview_generation: 0,
            focus: RedisBrowserFocus::Keys,
            find: None,
            scroll: 0,
            viewport_rows: 0,
            preview_scroll: 0,
            preview_viewport_rows: 0,
            preview_content_rows: 0,
        }
    }

    pub fn rebuild_tree(&mut self) {
        self.tree.rebuild(&self.keyspace.keys);
    }

    pub fn insert_tree_keys(&mut self) {
        self.tree.insert_keys(&self.keyspace.keys);
    }

    /// Select the first top-level node after the initial tree population.
    ///
    /// Incremental scan batches must not move an existing selection, so this
    /// is intentionally a no-op once a valid selection exists.
    pub fn select_first_root_if_empty(&mut self) -> Option<super::redis_key_tree::KeyTreeNodeId> {
        if self.tree.selected.is_some() {
            return None;
        }
        let first = self.tree.nodes.first()?.id.clone();
        self.tree.select(Some(first.clone()));
        Some(first)
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
        self.value_page = match self.tree.selected_key() {
            Some(key) => RedisValuePageState::Loading {
                key: RedisKeyId {
                    target: self.target.clone(),
                    key: key.to_vec(),
                },
            },
            None => RedisValuePageState::Empty,
        };
        self.content = match self.tree.selected_key() {
            Some(key) => RedisPreviewContentState::Loading {
                key: RedisKeyId {
                    target: self.target.clone(),
                    key: key.to_vec(),
                },
            },
            None => RedisPreviewContentState::Empty,
        };
        self.preview_scroll = 0;
    }

    pub fn preview_generation(&self) -> Option<u64> {
        (!matches!(self.preview, RedisPreviewState::Empty)).then_some(self.preview_generation)
    }

    pub fn scroll_pane(&mut self, pane: RedisBrowserPane, delta: isize, content_rows: usize) {
        let (scroll, viewport) = match pane {
            RedisBrowserPane::Keys => (&mut self.scroll, self.viewport_rows),
            RedisBrowserPane::Preview => (&mut self.preview_scroll, self.preview_viewport_rows),
        };
        let max = content_rows.saturating_sub(viewport.max(1));
        *scroll = scroll.saturating_add_signed(delta).min(max);
    }

    pub fn set_pane_viewport(
        &mut self,
        pane: RedisBrowserPane,
        viewport_rows: usize,
        content_rows: usize,
    ) {
        let (scroll, viewport) = match pane {
            RedisBrowserPane::Keys => (&mut self.scroll, &mut self.viewport_rows),
            RedisBrowserPane::Preview => {
                (&mut self.preview_scroll, &mut self.preview_viewport_rows)
            }
        };
        *viewport = viewport_rows;
        *scroll = (*scroll).min(content_rows.saturating_sub(viewport_rows.max(1)));
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
