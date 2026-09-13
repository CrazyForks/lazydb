use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RedisReply {
    Null,
    Integer(i64),
    Bytes(Vec<u8>),
    Status(String),
    Error {
        code: Option<String>,
        message: String,
    },
    Array(Vec<RedisReply>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyBudget {
    pub max_nodes: usize,
    pub max_bytes: usize,
    pub max_depth: usize,
}

impl Default for ReplyBudget {
    fn default() -> Self {
        Self {
            max_nodes: 10_000,
            max_bytes: 4 * 1024 * 1024,
            max_depth: 16,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedReply {
    pub reply: RedisReply,
    pub truncated: bool,
    pub original_nodes: usize,
    pub original_bytes: usize,
}

impl RedisReply {
    pub fn bound(&self, budget: ReplyBudget) -> BoundedReply {
        let mut state = BoundState::default();
        let reply = state.visit(self, budget, 0);
        BoundedReply {
            reply,
            truncated: state.truncated,
            original_nodes: state.nodes,
            original_bytes: state.bytes,
        }
    }
}

#[derive(Default)]
struct BoundState {
    nodes: usize,
    bytes: usize,
    truncated: bool,
}

impl BoundState {
    fn visit(&mut self, reply: &RedisReply, budget: ReplyBudget, depth: usize) -> RedisReply {
        self.nodes = self.nodes.saturating_add(1);
        if depth > budget.max_depth || self.nodes > budget.max_nodes {
            self.truncated = true;
            return RedisReply::Status("<reply truncated>".into());
        }
        match reply {
            RedisReply::Bytes(value) => {
                self.bytes = self.bytes.saturating_add(value.len());
                if self.bytes > budget.max_bytes {
                    self.truncated = true;
                    RedisReply::Status("<reply bytes truncated>".into())
                } else {
                    RedisReply::Bytes(value.clone())
                }
            }
            RedisReply::Array(values) => RedisReply::Array(
                values
                    .iter()
                    .map(|value| self.visit(value, budget, depth + 1))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}
