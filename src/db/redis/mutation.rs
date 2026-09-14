use thiserror::Error;
use uuid::Uuid;

use super::read::{RedisType, TtlState};
use super::types::RedisKeyId;
use super::{RedisAdapter, redis_error};
use crate::{
    db::{DatabaseError, ErrorCategory},
    identity::ConnectionIdentity,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisMutationMode {
    Create,
    Edit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedisValueDraft {
    String(Vec<u8>),
    Hash(Vec<(Vec<u8>, Vec<u8>)>),
    List(Vec<Vec<u8>>),
    Set(Vec<Vec<u8>>),
    SortedSet(Vec<(String, Vec<u8>)>),
    Stream(Vec<(Vec<u8>, Vec<u8>)>),
}

impl RedisValueDraft {
    pub fn value_type(&self) -> RedisType {
        match self {
            Self::String(_) => RedisType::String,
            Self::Hash(_) => RedisType::Hash,
            Self::List(_) => RedisType::List,
            Self::Set(_) => RedisType::Set,
            Self::SortedSet(_) => RedisType::SortedSet,
            Self::Stream(_) => RedisType::Stream,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedisMutationOperation {
    Replace(RedisValueDraft),
    SetString {
        value: Vec<u8>,
        expected: Option<Vec<u8>>,
    },
    SetHashField {
        field: Vec<u8>,
        value: Vec<u8>,
        expected: Option<Vec<u8>>,
    },
    DeleteHashField {
        field: Vec<u8>,
        expected: Option<Vec<u8>>,
    },
    SetListElement {
        index: i64,
        value: Vec<u8>,
        expected: Vec<u8>,
    },
    DeleteListElement {
        index: i64,
        expected: Vec<u8>,
    },
    AddSetMember {
        member: Vec<u8>,
    },
    RemoveSetMember {
        member: Vec<u8>,
    },
    SetSortedSetMember {
        member: Vec<u8>,
        score: String,
        expected_score: Option<String>,
    },
    RemoveSortedSetMember {
        member: Vec<u8>,
        expected_score: Option<String>,
    },
    AppendStream {
        fields: Vec<(Vec<u8>, Vec<u8>)>,
    },
}

impl RedisMutationOperation {
    pub fn value_type(&self) -> RedisType {
        match self {
            Self::Replace(value) => value.value_type(),
            Self::SetString { .. } => RedisType::String,
            Self::SetHashField { .. } | Self::DeleteHashField { .. } => RedisType::Hash,
            Self::SetListElement { .. } | Self::DeleteListElement { .. } => RedisType::List,
            Self::AddSetMember { .. } | Self::RemoveSetMember { .. } => RedisType::Set,
            Self::SetSortedSetMember { .. } | Self::RemoveSortedSetMember { .. } => {
                RedisType::SortedSet
            }
            Self::AppendStream { .. } => RedisType::Stream,
        }
    }

    fn is_targeted_edit(&self) -> bool {
        !matches!(self, Self::Replace(_))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisTtlMutation {
    Preserve,
    Persist,
    SetMillis(u64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisMutationRequest {
    pub connection: ConnectionIdentity,
    pub request_id: u64,
    pub mode: RedisMutationMode,
    pub key: RedisKeyId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RedisKeyBaseline {
    pub value_type: RedisType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisMutationCommand {
    pub name: String,
    pub args: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisMutationPlan {
    pub request: RedisMutationRequest,
    pub operation: RedisMutationOperation,
    pub ttl: RedisTtlMutation,
    pub baseline: Option<RedisKeyBaseline>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisMutationResult {
    pub key: RedisKeyId,
    pub value_type: RedisType,
    pub ttl_millis: Option<u64>,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum RedisMutationError {
    #[error("Redis mutation request is invalid: {reason}")]
    InvalidRequest { reason: String },
    #[error("Redis mutation draft is invalid: {reason}")]
    InvalidDraft { reason: String },
    #[error("Redis mutation baseline is stale")]
    StaleBaseline,
    #[error("Redis mutation profile does not match the selected key")]
    ProfileMismatch,
}

impl RedisMutationPlan {
    pub fn new(
        request: RedisMutationRequest,
        draft: RedisValueDraft,
        ttl_millis: Option<u64>,
        baseline: Option<RedisKeyBaseline>,
    ) -> Result<Self, RedisMutationError> {
        let ttl = match ttl_millis {
            Some(millis) => RedisTtlMutation::SetMillis(millis),
            None if request.mode == RedisMutationMode::Edit => RedisTtlMutation::Preserve,
            None => RedisTtlMutation::Persist,
        };
        Self::new_operation(
            request,
            RedisMutationOperation::Replace(draft),
            ttl,
            baseline,
        )
    }

    pub fn new_operation(
        request: RedisMutationRequest,
        operation: RedisMutationOperation,
        ttl: RedisTtlMutation,
        baseline: Option<RedisKeyBaseline>,
    ) -> Result<Self, RedisMutationError> {
        let plan = Self {
            request,
            operation,
            ttl,
            baseline,
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), RedisMutationError> {
        if self.request.request_id == 0 || self.request.key.key.is_empty() {
            return Err(RedisMutationError::InvalidRequest {
                reason: "request id and key are required".into(),
            });
        }
        if self.request.connection.profile_id != self.request.key.target.profile_id {
            return Err(RedisMutationError::ProfileMismatch);
        }
        match self.request.mode {
            RedisMutationMode::Create => {
                if self.baseline.is_some() || self.operation.is_targeted_edit() {
                    return Err(RedisMutationError::InvalidRequest {
                        reason: "create requires a complete value draft without an edit baseline"
                            .into(),
                    });
                }
            }
            RedisMutationMode::Edit => {
                let Some(baseline) = self.baseline else {
                    return Err(RedisMutationError::StaleBaseline);
                };
                if baseline.value_type != self.operation.value_type() {
                    return Err(RedisMutationError::InvalidDraft {
                        reason: "operation type does not match the selected Redis value".into(),
                    });
                }
            }
        }
        match &self.operation {
            RedisMutationOperation::Replace(value) => validate_value(value)?,
            RedisMutationOperation::SetString { .. } => {}
            RedisMutationOperation::SetHashField { .. }
            | RedisMutationOperation::DeleteHashField { .. } => {}
            RedisMutationOperation::SetListElement { index, .. }
            | RedisMutationOperation::DeleteListElement { index, .. }
                if *index < 0 =>
            {
                return Err(RedisMutationError::InvalidDraft {
                    reason: "list element index must not be negative".into(),
                });
            }
            RedisMutationOperation::AddSetMember { .. }
            | RedisMutationOperation::RemoveSetMember { .. } => {}
            RedisMutationOperation::SetSortedSetMember { score, .. }
            | RedisMutationOperation::RemoveSortedSetMember {
                expected_score: Some(score),
                ..
            } => validate_score(score)?,
            RedisMutationOperation::RemoveSortedSetMember {
                expected_score: None,
                ..
            } => {}
            RedisMutationOperation::AppendStream { fields } => {
                if fields.is_empty() {
                    return Err(RedisMutationError::InvalidDraft {
                        reason: "a stream append requires at least one field".into(),
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Returns the native command sequence used to describe the mutation.
    /// Execution uses an equivalent Lua script so TTL preservation and
    /// expected-value checks happen atomically on the Redis server.
    pub fn commands(&self) -> Result<Vec<RedisMutationCommand>, RedisMutationError> {
        self.validate()?;
        let key = self.request.key.key.clone();
        let mut commands = Vec::new();
        let mut add = |name: &str, args: Vec<Vec<u8>>| {
            commands.push(RedisMutationCommand {
                name: name.to_owned(),
                args,
            });
        };
        match &self.operation {
            RedisMutationOperation::Replace(RedisValueDraft::String(value)) => {
                add("SET", vec![key.clone(), value.clone()]);
            }
            RedisMutationOperation::Replace(RedisValueDraft::Hash(entries)) => {
                add("DEL", vec![key.clone()]);
                add("HSET", hash_args(&key, entries));
            }
            RedisMutationOperation::Replace(RedisValueDraft::List(values)) => {
                add("DEL", vec![key.clone()]);
                add("RPUSH", list_args(&key, values));
            }
            RedisMutationOperation::Replace(RedisValueDraft::Set(values)) => {
                add("DEL", vec![key.clone()]);
                add("SADD", values_args(&key, values));
            }
            RedisMutationOperation::Replace(RedisValueDraft::SortedSet(entries)) => {
                add("DEL", vec![key.clone()]);
                let mut args = vec![key.clone()];
                for (score, member) in entries {
                    args.extend([score.as_bytes().to_vec(), member.clone()]);
                }
                add("ZADD", args);
            }
            RedisMutationOperation::Replace(RedisValueDraft::Stream(entries)) => {
                add("DEL", vec![key.clone()]);
                add("XADD", stream_args(&key, entries));
            }
            RedisMutationOperation::SetString { value, .. } => {
                add("SET", vec![key, value.clone()]);
            }
            RedisMutationOperation::SetHashField { field, value, .. } => {
                add("HSET", vec![key, field.clone(), value.clone()]);
            }
            RedisMutationOperation::DeleteHashField { field, .. } => {
                add("HDEL", vec![key, field.clone()]);
            }
            RedisMutationOperation::SetListElement { index, value, .. } => {
                add(
                    "LSET",
                    vec![key, index.to_string().into_bytes(), value.clone()],
                );
            }
            RedisMutationOperation::DeleteListElement { .. } => {
                add("EVAL", vec![b"atomic list-index delete".to_vec(), key]);
            }
            RedisMutationOperation::AddSetMember { member } => {
                add("SADD", vec![key, member.clone()]);
            }
            RedisMutationOperation::RemoveSetMember { member } => {
                add("SREM", vec![key, member.clone()]);
            }
            RedisMutationOperation::SetSortedSetMember { member, score, .. } => {
                add("ZADD", vec![key, score.as_bytes().to_vec(), member.clone()]);
            }
            RedisMutationOperation::RemoveSortedSetMember { member, .. } => {
                add("ZREM", vec![key, member.clone()]);
            }
            RedisMutationOperation::AppendStream { fields } => {
                add("XADD", stream_args(&key, fields));
            }
        }
        match self.ttl {
            RedisTtlMutation::Preserve => {}
            RedisTtlMutation::Persist => add("PERSIST", vec![self.request.key.key.clone()]),
            RedisTtlMutation::SetMillis(millis) => add(
                "PEXPIRE",
                vec![
                    self.request.key.key.clone(),
                    millis.to_string().into_bytes(),
                ],
            ),
        }
        Ok(commands)
    }

    fn script(&self) -> Result<RedisScript, RedisMutationError> {
        self.validate()?;
        let expected_type = match self.request.mode {
            RedisMutationMode::Create => "none",
            RedisMutationMode::Edit => redis_type_name(self.operation.value_type()),
        };
        let mut args = vec![
            expected_type.as_bytes().to_vec(),
            ttl_mode(self.ttl).as_bytes().to_vec(),
            ttl_value(self.ttl),
        ];
        let body = match &self.operation {
            RedisMutationOperation::Replace(value) => replace_script(value, &mut args),
            RedisMutationOperation::SetString { value, expected } => {
                args.extend(expected_args(expected));
                args.push(value.clone());
                "if ARGV[4] == '1' and redis.call('GET', KEYS[1]) ~= ARGV[5] then return -1 end\nredis.call('SET', KEYS[1], ARGV[6])".to_owned()
            }
            RedisMutationOperation::SetHashField {
                field,
                value,
                expected,
            } => {
                args.push(field.clone());
                args.extend(expected_args(expected));
                args.push(value.clone());
                "local exists = redis.call('HEXISTS', KEYS[1], ARGV[4]) == 1\nif ARGV[5] == '1' and (not exists or redis.call('HGET', KEYS[1], ARGV[4]) ~= ARGV[6]) then return -1 end\nredis.call('HSET', KEYS[1], ARGV[4], ARGV[7])".to_owned()
            }
            RedisMutationOperation::DeleteHashField { field, expected } => {
                args.push(field.clone());
                args.extend(expected_args(expected));
                "local exists = redis.call('HEXISTS', KEYS[1], ARGV[4]) == 1\nif ARGV[5] == '1' and (not exists or redis.call('HGET', KEYS[1], ARGV[4]) ~= ARGV[6]) then return -1 end\nredis.call('HDEL', KEYS[1], ARGV[4])".to_owned()
            }
            RedisMutationOperation::SetListElement {
                index,
                value,
                expected,
            } => {
                args.extend([
                    index.to_string().into_bytes(),
                    expected.clone(),
                    value.clone(),
                ]);
                "if redis.call('LINDEX', KEYS[1], ARGV[4]) ~= ARGV[5] then return -1 end\nredis.call('LSET', KEYS[1], ARGV[4], ARGV[6])".to_owned()
            }
            RedisMutationOperation::DeleteListElement { index, expected } => {
                args.extend([index.to_string().into_bytes(), expected.clone()]);
                "local index = tonumber(ARGV[4])\nlocal values = redis.call('LRANGE', KEYS[1], 0, -1)\nif values[index + 1] ~= ARGV[5] then return -1 end\nredis.call('DEL', KEYS[1])\nfor i, value in ipairs(values) do if i ~= index + 1 then redis.call('RPUSH', KEYS[1], value) end end".to_owned()
            }
            RedisMutationOperation::AddSetMember { member } => {
                args.push(member.clone());
                "redis.call('SADD', KEYS[1], ARGV[4])".to_owned()
            }
            RedisMutationOperation::RemoveSetMember { member } => {
                args.push(member.clone());
                "redis.call('SREM', KEYS[1], ARGV[4])".to_owned()
            }
            RedisMutationOperation::SetSortedSetMember {
                member,
                score,
                expected_score,
            } => {
                args.extend([
                    member.clone(),
                    score.as_bytes().to_vec(),
                    expected_score.is_some().to_string().into_bytes(),
                    expected_score.clone().unwrap_or_default().into_bytes(),
                ]);
                "if ARGV[6] == 'true' and tostring(redis.call('ZSCORE', KEYS[1], ARGV[4]) or '') ~= ARGV[7] then return -1 end\nredis.call('ZADD', KEYS[1], ARGV[5], ARGV[4])".to_owned()
            }
            RedisMutationOperation::RemoveSortedSetMember {
                member,
                expected_score,
            } => {
                args.push(member.clone());
                args.push(expected_score.is_some().to_string().into_bytes());
                args.push(expected_score.clone().unwrap_or_default().into_bytes());
                "if ARGV[5] == 'true' and tostring(redis.call('ZSCORE', KEYS[1], ARGV[4]) or '') ~= ARGV[6] then return -1 end\nredis.call('ZREM', KEYS[1], ARGV[4])".to_owned()
            }
            RedisMutationOperation::AppendStream { fields } => {
                args.extend(stream_fields(fields));
                "redis.call('XADD', KEYS[1], '*', unpack(ARGV, 4))".to_owned()
            }
        };
        let body = format!(
            "local previous_ttl = redis.call('PTTL', KEYS[1])\nlocal actual_type = redis.call('TYPE', KEYS[1]).ok\nif ARGV[1] ~= '' and actual_type ~= ARGV[1] then return -2 end\n{body}\nif ARGV[2] == 'preserve' then\n  if previous_ttl >= 0 and redis.call('EXISTS', KEYS[1]) == 1 then redis.call('PEXPIRE', KEYS[1], previous_ttl) end\nelseif ARGV[2] == 'persist' then\n  redis.call('PERSIST', KEYS[1])\nelse\n  redis.call('PEXPIRE', KEYS[1], ARGV[3])\nend\nreturn 1"
        );
        Ok(RedisScript { body, args })
    }
}

#[derive(Clone, Debug)]
struct RedisScript {
    body: String,
    args: Vec<Vec<u8>>,
}

impl RedisAdapter {
    pub fn plan_mutation(
        request: RedisMutationRequest,
        draft: RedisValueDraft,
        ttl_millis: Option<u64>,
        baseline: Option<RedisKeyBaseline>,
    ) -> Result<RedisMutationPlan, RedisMutationError> {
        RedisMutationPlan::new(request, draft, ttl_millis, baseline)
    }

    pub fn plan_operation(
        request: RedisMutationRequest,
        operation: RedisMutationOperation,
        ttl: RedisTtlMutation,
        baseline: Option<RedisKeyBaseline>,
    ) -> Result<RedisMutationPlan, RedisMutationError> {
        RedisMutationPlan::new_operation(request, operation, ttl, baseline)
    }

    pub async fn execute_mutation(
        &self,
        plan: &RedisMutationPlan,
    ) -> Result<RedisMutationResult, DatabaseError> {
        plan.validate()
            .map_err(|error| DatabaseError::configuration(error.to_string()))?;
        if plan.request.connection.profile_id != self.connection_id {
            return Err(DatabaseError::configuration(
                "Redis mutation connection does not match the adapter",
            ));
        }
        if plan.request.key.target.database != self.database() {
            return Err(DatabaseError::configuration(
                "Redis mutation targets another database",
            ));
        }
        self.invalidate_metadata(&plan.request.key.key);
        let current = self.key_metadata(&plan.request.key).await?;
        match plan.request.mode {
            RedisMutationMode::Create if current.value_type != RedisType::Missing => {
                return Err(DatabaseError::configuration(
                    "Redis key already exists; use edit mode to replace it",
                ));
            }
            RedisMutationMode::Edit
                if plan.baseline.map(|baseline| baseline.value_type)
                    != Some(current.value_type) =>
            {
                return Err(DatabaseError::configuration(
                    RedisMutationError::StaleBaseline.to_string(),
                ));
            }
            _ => {}
        }
        let script = plan
            .script()
            .map_err(|error| DatabaseError::configuration(error.to_string()))?;
        let mut connection = self.connection_clone();
        let mut command = redis::cmd("EVAL");
        command.arg(script.body).arg(1).arg(&plan.request.key.key);
        for argument in script.args {
            command.arg(argument);
        }
        let result = command.query_async::<i64>(&mut connection).await;
        self.invalidate_metadata(&plan.request.key.key);
        let result = result.map_err(|error| redis_error(error, ErrorCategory::Network))?;
        match result {
            -1 => {
                return Err(DatabaseError::configuration(
                    RedisMutationError::StaleBaseline.to_string(),
                ));
            }
            -2 => {
                return Err(DatabaseError::configuration(
                    "Redis key type changed while applying the mutation",
                ));
            }
            _ => {}
        }
        let metadata = self.key_metadata(&plan.request.key).await?;
        Ok(RedisMutationResult {
            key: plan.request.key.clone(),
            value_type: metadata.value_type,
            ttl_millis: match metadata.ttl {
                TtlState::ExpiresIn { millis } => Some(millis),
                _ => None,
            },
        })
    }
}

fn validate_value(value: &RedisValueDraft) -> Result<(), RedisMutationError> {
    match value {
        RedisValueDraft::String(_) => {}
        RedisValueDraft::Hash(entries) if entries.is_empty() => {
            return Err(RedisMutationError::InvalidDraft {
                reason: "a hash requires at least one field".into(),
            });
        }
        RedisValueDraft::List(values) | RedisValueDraft::Set(values) if values.is_empty() => {
            return Err(RedisMutationError::InvalidDraft {
                reason: "a collection requires at least one value".into(),
            });
        }
        RedisValueDraft::SortedSet(entries) if entries.is_empty() => {
            return Err(RedisMutationError::InvalidDraft {
                reason: "a sorted set requires at least one member".into(),
            });
        }
        RedisValueDraft::SortedSet(entries) => {
            for (score, _) in entries {
                validate_score(score)?;
            }
        }
        RedisValueDraft::Stream(entries) if entries.is_empty() => {
            return Err(RedisMutationError::InvalidDraft {
                reason: "a stream requires at least one field/value pair".into(),
            });
        }
        _ => {}
    }
    Ok(())
}

fn validate_score(score: &str) -> Result<(), RedisMutationError> {
    if score
        .parse::<f64>()
        .map_or(true, |value| !value.is_finite())
    {
        return Err(RedisMutationError::InvalidDraft {
            reason: "sorted-set scores must be finite numbers".into(),
        });
    }
    Ok(())
}

fn redis_type_name(value_type: RedisType) -> &'static str {
    match value_type {
        RedisType::String => "string",
        RedisType::Hash => "hash",
        RedisType::List => "list",
        RedisType::Set => "set",
        RedisType::SortedSet => "zset",
        RedisType::Stream => "stream",
        _ => "none",
    }
}

fn ttl_mode(ttl: RedisTtlMutation) -> &'static str {
    match ttl {
        RedisTtlMutation::Preserve => "preserve",
        RedisTtlMutation::Persist => "persist",
        RedisTtlMutation::SetMillis(_) => "set",
    }
}

fn ttl_value(ttl: RedisTtlMutation) -> Vec<u8> {
    match ttl {
        RedisTtlMutation::SetMillis(millis) => millis.to_string().into_bytes(),
        RedisTtlMutation::Preserve | RedisTtlMutation::Persist => Vec::new(),
    }
}

fn expected_args(expected: &Option<Vec<u8>>) -> Vec<Vec<u8>> {
    match expected {
        Some(value) => vec![b"1".to_vec(), value.clone()],
        None => vec![b"0".to_vec(), Vec::new()],
    }
}

fn replace_script(value: &RedisValueDraft, args: &mut Vec<Vec<u8>>) -> String {
    match value {
        RedisValueDraft::String(value) => {
            args.push(value.clone());
            "redis.call('SET', KEYS[1], ARGV[4])".to_owned()
        }
        RedisValueDraft::Hash(entries) => {
            args.extend(
                entries
                    .iter()
                    .flat_map(|(field, value)| [field.clone(), value.clone()]),
            );
            "redis.call('DEL', KEYS[1])\nredis.call('HSET', KEYS[1], unpack(ARGV, 4))".to_owned()
        }
        RedisValueDraft::List(values) => {
            args.extend(values.iter().cloned());
            "redis.call('DEL', KEYS[1])\nredis.call('RPUSH', KEYS[1], unpack(ARGV, 4))".to_owned()
        }
        RedisValueDraft::Set(values) => {
            args.extend(values.iter().cloned());
            "redis.call('DEL', KEYS[1])\nredis.call('SADD', KEYS[1], unpack(ARGV, 4))".to_owned()
        }
        RedisValueDraft::SortedSet(entries) => {
            args.extend(
                entries
                    .iter()
                    .flat_map(|(score, member)| [score.as_bytes().to_vec(), member.clone()]),
            );
            "redis.call('DEL', KEYS[1])\nredis.call('ZADD', KEYS[1], unpack(ARGV, 4))".to_owned()
        }
        RedisValueDraft::Stream(entries) => {
            args.extend(stream_fields(entries));
            "redis.call('DEL', KEYS[1])\nredis.call('XADD', KEYS[1], '*', unpack(ARGV, 4))"
                .to_owned()
        }
    }
}

fn hash_args(key: &[u8], entries: &[(Vec<u8>, Vec<u8>)]) -> Vec<Vec<u8>> {
    std::iter::once(key.to_vec())
        .chain(
            entries
                .iter()
                .flat_map(|(field, value)| [field.clone(), value.clone()]),
        )
        .collect()
}

fn list_args(key: &[u8], values: &[Vec<u8>]) -> Vec<Vec<u8>> {
    std::iter::once(key.to_vec())
        .chain(values.iter().cloned())
        .collect()
}

fn values_args(key: &[u8], values: &[Vec<u8>]) -> Vec<Vec<u8>> {
    list_args(key, values)
}

fn stream_args(key: &[u8], entries: &[(Vec<u8>, Vec<u8>)]) -> Vec<Vec<u8>> {
    std::iter::once(key.to_vec())
        .chain(std::iter::once(b"*".to_vec()))
        .chain(stream_fields(entries))
        .collect()
}

fn stream_fields(entries: &[(Vec<u8>, Vec<u8>)]) -> Vec<Vec<u8>> {
    entries
        .iter()
        .flat_map(|(field, value)| [field.clone(), value.clone()])
        .collect()
}

pub fn connection_identity(profile_id: Uuid, generation: u64) -> ConnectionIdentity {
    ConnectionIdentity {
        profile_id,
        generation,
    }
}
