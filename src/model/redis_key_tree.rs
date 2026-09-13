use std::collections::{BTreeMap, HashSet};

use crate::db::redis::types::RedisKeyId;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum KeyTreeNodeId {
    Prefix(Vec<u8>),
    Key(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyTreeNode {
    pub id: KeyTreeNodeId,
    pub key_id: Option<KeyTreeNodeId>,
    pub label: Vec<u8>,
    pub children: Vec<KeyTreeNode>,
    pub is_key: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KeyTreeState {
    pub expanded: HashSet<KeyTreeNodeId>,
    pub selected: Option<KeyTreeNodeId>,
    pub nodes: Vec<KeyTreeNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VisibleKeyTreeRow {
    pub id: KeyTreeNodeId,
    pub parent: Option<KeyTreeNodeId>,
    pub label: Vec<u8>,
    pub depth: usize,
    pub is_key: bool,
    pub expandable: bool,
    pub expanded: bool,
}

impl KeyTreeState {
    pub fn visible_rows(&self) -> Vec<VisibleKeyTreeRow> {
        fn visit(
            state: &KeyTreeState,
            nodes: &[KeyTreeNode],
            parent: Option<&KeyTreeNodeId>,
            depth: usize,
            output: &mut Vec<VisibleKeyTreeRow>,
        ) {
            for node in nodes {
                let expanded = state.expanded.contains(&node.id);
                output.push(VisibleKeyTreeRow {
                    id: node.id.clone(),
                    parent: parent.cloned(),
                    label: node.label.clone(),
                    depth,
                    is_key: node.id.is_key(),
                    expandable: !node.children.is_empty(),
                    expanded,
                });
                if let Some(key_id) = &node.key_id {
                    output.push(VisibleKeyTreeRow {
                        id: key_id.clone(),
                        parent: Some(node.id.clone()),
                        label: node.label.clone(),
                        depth: depth + 1,
                        is_key: true,
                        expandable: false,
                        expanded: false,
                    });
                }
                if expanded {
                    visit(state, &node.children, Some(&node.id), depth + 1, output);
                }
            }
        }
        let mut output = Vec::new();
        visit(self, &self.nodes, None, 0, &mut output);
        output
    }

    pub fn visible_ids(&self) -> Vec<KeyTreeNodeId> {
        fn visit(state: &KeyTreeState, nodes: &[KeyTreeNode], output: &mut Vec<KeyTreeNodeId>) {
            for node in nodes {
                output.push(node.id.clone());
                if let Some(key_id) = &node.key_id {
                    output.push(key_id.clone());
                }
                if state.expanded.contains(&node.id) {
                    visit(state, &node.children, output);
                }
            }
        }
        let mut output = Vec::new();
        visit(self, &self.nodes, &mut output);
        output
    }

    pub fn move_selection(&mut self, delta: isize) -> Option<KeyTreeNodeId> {
        let ids = self.visible_ids();
        if ids.is_empty() {
            self.selected = None;
            return None;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|selected| ids.iter().position(|id| id == selected))
            .map(|current| current as isize + delta)
            .unwrap_or_else(|| if delta < 0 { ids.len() as isize - 1 } else { 0 });
        let next = current.clamp(0, ids.len() as isize - 1) as usize;
        self.selected = Some(ids[next].clone());
        self.selected.clone()
    }

    pub fn ensure_selected_visible(&self, scroll: &mut usize, viewport_rows: usize) {
        let Some(selected) = self.selected.as_ref() else {
            return;
        };
        let ids = self.visible_ids();
        let Some(index) = ids.iter().position(|id| id == selected) else {
            return;
        };
        if index < *scroll {
            *scroll = index;
        } else if viewport_rows > 0 && index >= scroll.saturating_add(viewport_rows) {
            *scroll = index + 1 - viewport_rows;
        }
    }

    pub fn rebuild(&mut self, keys: &[RedisKeyId]) {
        let mut root = BTreeMap::new();
        for key in keys {
            insert_key(&mut root, &key.key);
        }
        self.nodes = root.into_values().map(NodeBuilder::build).collect();
        self.selected = self.selected.take().filter(|id| self.contains(id));
        let valid = self
            .expanded
            .iter()
            .filter(|id| self.contains(id))
            .cloned()
            .collect();
        self.expanded = valid;
    }

    pub fn contains(&self, id: &KeyTreeNodeId) -> bool {
        fn visit(nodes: &[KeyTreeNode], id: &KeyTreeNodeId) -> bool {
            nodes.iter().any(|node| {
                &node.id == id || node.key_id.as_ref() == Some(id) || visit(&node.children, id)
            })
        }
        visit(&self.nodes, id)
    }

    pub fn select(&mut self, id: Option<KeyTreeNodeId>) {
        self.selected = id.filter(|id| self.contains(id));
    }

    pub fn selected_key(&self) -> Option<&[u8]> {
        match self.selected.as_ref()? {
            KeyTreeNodeId::Key(key) => Some(key),
            KeyTreeNodeId::Prefix(_) => None,
        }
    }

    pub fn parent_of(&self, target: &KeyTreeNodeId) -> Option<KeyTreeNodeId> {
        fn find(
            nodes: &[KeyTreeNode],
            target: &KeyTreeNodeId,
            parent: Option<&KeyTreeNodeId>,
        ) -> Option<KeyTreeNodeId> {
            for node in nodes {
                if &node.id == target || node.key_id.as_ref() == Some(target) {
                    return parent.cloned();
                }
                if let Some(parent) = find(&node.children, target, Some(&node.id)) {
                    return Some(parent);
                }
            }
            None
        }
        find(&self.nodes, target, None)
    }

    pub fn first_child(&self, id: &KeyTreeNodeId) -> Option<KeyTreeNodeId> {
        fn find<'a>(nodes: &'a [KeyTreeNode], id: &KeyTreeNodeId) -> Option<&'a KeyTreeNode> {
            for node in nodes {
                if &node.id == id {
                    return Some(node);
                }
                if let Some(found) = find(&node.children, id) {
                    return Some(found);
                }
            }
            None
        }
        find(&self.nodes, id)?
            .children
            .first()
            .map(|node| node.id.clone())
    }
}

impl KeyTreeNodeId {
    pub const fn is_key(&self) -> bool {
        matches!(self, Self::Key(_))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NodeBuilder {
    id: KeyTreeNodeId,
    key_id: Option<KeyTreeNodeId>,
    label: Vec<u8>,
    children: BTreeMap<Vec<u8>, NodeBuilder>,
    is_key: bool,
}

impl NodeBuilder {
    fn build(self) -> KeyTreeNode {
        KeyTreeNode {
            id: self.id,
            key_id: self.key_id,
            label: self.label,
            children: self.children.into_values().map(Self::build).collect(),
            is_key: self.is_key,
        }
    }
}

fn insert_key(root: &mut BTreeMap<Vec<u8>, NodeBuilder>, key: &[u8]) {
    let parts = key.split(|byte| *byte == b':').collect::<Vec<_>>();
    insert_parts(root, &parts, 0, &mut Vec::new(), key);
}

fn insert_parts(
    current: &mut BTreeMap<Vec<u8>, NodeBuilder>,
    parts: &[&[u8]],
    index: usize,
    prefix: &mut Vec<u8>,
    key: &[u8],
) {
    let part = parts[index];
    let is_last = index + 1 == parts.len();
    prefix.extend_from_slice(part);
    if !is_last {
        prefix.push(b':');
    }
    let id = if is_last {
        KeyTreeNodeId::Key(key.to_vec())
    } else {
        KeyTreeNodeId::Prefix(prefix.clone())
    };
    let entry = current.entry(part.to_vec()).or_insert_with(|| NodeBuilder {
        id,
        key_id: None,
        label: part.to_vec(),
        children: BTreeMap::new(),
        is_key: is_last,
    });
    entry.is_key |= is_last;
    if is_last && matches!(entry.id, KeyTreeNodeId::Prefix(_)) {
        entry.key_id = Some(KeyTreeNodeId::Key(key.to_vec()));
    }
    if !is_last {
        insert_parts(&mut entry.children, parts, index + 1, prefix, key);
    }
}
