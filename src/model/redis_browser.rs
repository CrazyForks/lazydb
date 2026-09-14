use crate::model::text_input::TextInput;
use uuid::Uuid;

use crate::db::redis::read::RedisValuePage;
use crate::db::redis::types::{RedisKeyId, RedisTarget};

use super::redis_key_tree::{KeyTreeNodeId, VisibleKeyTreeRow};
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
    pub rows: Vec<VisibleKeyTreeRow>,
    pub filtered_rows: Vec<VisibleKeyTreeRow>,
    pub matches: Vec<KeyTreeNodeId>,
    pub current: usize,
    pub original_selected: Option<super::redis_key_tree::KeyTreeNodeId>,
    pub original_scroll: usize,
    pub expanded: std::collections::HashSet<KeyTreeNodeId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisBrowserTab {
    pub id: Uuid,
    pub preview_editor_id: Uuid,
    pub preview_wrap: bool,
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
            preview_wrap: true,
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
        if self
            .find
            .as_ref()
            .is_some_and(|find| find.phase == RedisFindPhase::Confirmed)
        {
            self.refresh_find_rows();
        }
    }

    pub fn visible_rows(&self) -> Vec<VisibleKeyTreeRow> {
        self.find.as_ref().map_or_else(
            || self.tree.visible_rows(),
            |find| match find.phase {
                RedisFindPhase::Editing => self.tree.visible_rows(),
                RedisFindPhase::Confirmed => find.filtered_rows.clone(),
            },
        )
    }

    pub fn visible_ids(&self) -> Vec<KeyTreeNodeId> {
        self.visible_rows().into_iter().map(|row| row.id).collect()
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

    pub fn append_value_page(&mut self, next: RedisValuePage) {
        let RedisValuePageState::Ready(current) = &mut self.value_page else {
            self.value_page = RedisValuePageState::Ready(next);
            return;
        };
        if current.metadata.key != next.metadata.key {
            return;
        }
        match (&mut current.value, next.value) {
            (
                crate::db::redis::read::RedisPageValue::String(left),
                crate::db::redis::read::RedisPageValue::String(right),
            ) => left.extend(right),
            (
                crate::db::redis::read::RedisPageValue::Hash(left),
                crate::db::redis::read::RedisPageValue::Hash(right),
            ) => left.extend(right),
            (
                crate::db::redis::read::RedisPageValue::List(left),
                crate::db::redis::read::RedisPageValue::List(right),
            ) => left.extend(right),
            (
                crate::db::redis::read::RedisPageValue::Set(left),
                crate::db::redis::read::RedisPageValue::Set(right),
            ) => left.extend(right),
            (
                crate::db::redis::read::RedisPageValue::SortedSet(left),
                crate::db::redis::read::RedisPageValue::SortedSet(right),
            ) => left.extend(right),
            (
                crate::db::redis::read::RedisPageValue::Stream(left),
                crate::db::redis::read::RedisPageValue::Stream(right),
            ) => left.extend(right),
            _ => return,
        }
        current.position = next.position;
        current.complete = next.complete;
        current.truncated = next.truncated;
        current.raw_bytes = current.raw_bytes.saturating_add(next.raw_bytes);
        current.formatted_bytes = current.formatted_bytes.saturating_add(next.formatted_bytes);
        self.content = RedisPreviewContentState::Ready {
            key: current.metadata.key.clone(),
            page: current.clone(),
            format: self.format.selected,
        };
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
        if let Some(find) = self.find.as_mut() {
            if !find.expanded.insert(node.clone()) {
                find.expanded.remove(node);
            }
            self.rebuild_filtered_rows();
        } else if !self.tree.expanded.insert(node.clone()) {
            self.tree.expanded.remove(node);
        }
        true
    }

    pub fn open_find(&mut self) {
        if self.find.is_some() {
            return;
        }
        let rows = self.tree.visible_rows();
        self.find = Some(RedisKeyFindState {
            phase: RedisFindPhase::Editing,
            query: TextInput::default(),
            rows,
            filtered_rows: Vec::new(),
            matches: Vec::new(),
            current: 0,
            original_selected: self.tree.selected.clone(),
            original_scroll: self.scroll,
            expanded: self.tree.expanded.clone(),
        });
    }

    pub fn refresh_find_rows(&mut self) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        if find.phase != RedisFindPhase::Confirmed {
            return;
        }
        let mut projection = KeyTreeState::default();
        projection.rebuild(&self.keyspace.keys);
        for key in &self.keyspace.keys {
            let mut prefix = Vec::new();
            let parts = key.key.split(|byte| *byte == b':').collect::<Vec<_>>();
            for part in parts.iter().take(parts.len().saturating_sub(1)) {
                prefix.extend_from_slice(part);
                prefix.push(b':');
                projection
                    .expanded
                    .insert(KeyTreeNodeId::Prefix(prefix.clone()));
            }
        }
        if find.filtered_rows.is_empty() {
            find.expanded = projection.expanded.clone();
        }
        let rows = projection.visible_rows();
        find.rows = rows;
        self.update_find();
    }

    fn rebuild_filtered_rows(&mut self) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let included: std::collections::HashSet<KeyTreeNodeId> =
            if find.query.value().trim().is_empty() {
                find.rows.iter().map(|row| row.id.clone()).collect()
            } else {
                find.matches
                    .iter()
                    .filter_map(|id| find.rows.iter().find(|row| &row.id == id))
                    .flat_map(|row| {
                        std::iter::successors(Some(row), |current| {
                            current.parent.as_ref().and_then(|parent| {
                                find.rows.iter().find(|candidate| &candidate.id == parent)
                            })
                        })
                        .map(|row| row.id.clone())
                    })
                    .collect()
            };
        let mut visible = std::collections::HashSet::new();
        find.filtered_rows = find
            .rows
            .iter()
            .filter_map(|row| {
                if !included.contains(&row.id)
                    || row.parent.as_ref().is_some_and(|parent| {
                        !visible.contains(parent) || !find.expanded.contains(parent)
                    })
                {
                    return None;
                }
                let mut row = row.clone();
                row.expanded = find.expanded.contains(&row.id);
                visible.insert(row.id.clone());
                Some(row)
            })
            .collect();
    }

    pub fn update_find(&mut self) {
        let first_match = {
            let Some(find) = self.find.as_mut() else {
                return;
            };
            if find.phase != RedisFindPhase::Confirmed {
                return;
            }
            let query = find.query.value().trim();
            find.matches = if query.is_empty() {
                Vec::new()
            } else {
                find.rows
                    .iter()
                    .filter(|row| {
                        matches!(&row.id, KeyTreeNodeId::Key(_))
                            && crate::db::catalog::search_text_matches(
                                &match &row.id {
                                    KeyTreeNodeId::Key(bytes) => {
                                        crate::model::redis_key_text::display_bytes(bytes)
                                    }
                                    KeyTreeNodeId::Prefix(_) => String::new(),
                                },
                                query,
                            )
                    })
                    .map(|row| row.id.clone())
                    .collect()
            };
            find.matches.first().cloned()
        };
        self.rebuild_filtered_rows();
        if let Some(find) = self.find.as_mut() {
            find.current = 0;
        }
        if let Some(id) = first_match {
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
        if let Some(selected) = self.tree.selected.as_ref()
            && let Some(index) = self.visible_ids().iter().position(|id| id == selected)
        {
            self.scroll = index.saturating_sub(self.viewport_rows.saturating_sub(1));
        }
    }

    pub fn confirm_find(&mut self) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        if find.query.value().trim().is_empty() {
            return;
        }
        find.phase = RedisFindPhase::Confirmed;
        self.refresh_find_rows();
        self.update_find();
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
