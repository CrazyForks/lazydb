use super::{RedisAdapter, types::RedisKeyId};
use crate::db::{DatabaseError, ErrorCategory};

pub const MAX_STRING_PREVIEW_BYTES: usize = 64 * 1024;
pub const MAX_COLLECTION_PREVIEW_ITEMS: usize = 200;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisType {
    String,
    Hash,
    List,
    Set,
    SortedSet,
    Stream,
    Module,
    Missing,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TtlState {
    Missing,
    Persistent,
    ExpiresIn { millis: u64 },
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisKeyMetadata {
    pub key: RedisKeyId,
    pub value_type: RedisType,
    pub ttl: TtlState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedisReadRequest {
    StringRange {
        key: RedisKeyId,
        start: u64,
        end: u64,
    },
    HashScan {
        key: RedisKeyId,
        cursor: u64,
        count: u32,
    },
    ListRange {
        key: RedisKeyId,
        start: u64,
        end: u64,
    },
    SetScan {
        key: RedisKeyId,
        cursor: u64,
        count: u32,
    },
    SortedSetRange {
        key: RedisKeyId,
        start: u64,
        end: u64,
    },
}

impl RedisReadRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::StringRange { start, end, .. } if end < start => {
                Err("string range end precedes start")
            }
            Self::StringRange { start, end, .. }
                if end.saturating_sub(*start) + 1 > MAX_STRING_PREVIEW_BYTES as u64 =>
            {
                Err("string range exceeds preview budget")
            }
            Self::HashScan { count, .. } | Self::SetScan { count, .. }
                if *count as usize > MAX_COLLECTION_PREVIEW_ITEMS =>
            {
                Err("collection count exceeds preview budget")
            }
            Self::ListRange { start, end, .. } | Self::SortedSetRange { start, end, .. }
                if end < start
                    || end.saturating_sub(*start) + 1 > MAX_COLLECTION_PREVIEW_ITEMS as u64 =>
            {
                Err("collection range exceeds preview budget")
            }
            _ => Ok(()),
        }
    }
}

impl RedisAdapter {
    pub async fn preview_key(&self, key: &RedisKeyId) -> Result<String, DatabaseError> {
        if key.target.database != self.database() {
            return Err(DatabaseError::configuration(
                "Redis key target database mismatch",
            ));
        }
        let mut connection = self.connection_clone();
        let value_type: String = redis::cmd("TYPE")
            .arg(&key.key)
            .query_async(&mut connection)
            .await
            .map_err(|error| redis_error(error, ErrorCategory::Network))?;
        let ttl: i64 = redis::cmd("PTTL")
            .arg(&key.key)
            .query_async(&mut connection)
            .await
            .map_err(|error| redis_error(error, ErrorCategory::Network))?;
        let header = format!("type={value_type} ttl={ttl}ms\n");
        let body = match value_type.as_str() {
            "none" => "<missing>".to_owned(),
            "string" => {
                let value: Vec<u8> = redis::cmd("GETRANGE")
                    .arg(&key.key)
                    .arg(0)
                    .arg(MAX_STRING_PREVIEW_BYTES as i64 - 1)
                    .query_async(&mut connection)
                    .await
                    .map_err(|error| redis_error(error, ErrorCategory::Network))?;
                display_bytes(&value)
            }
            "hash" => scan_pairs(&mut connection, "HSCAN", &key.key).await?,
            "set" => scan_values(&mut connection, "SSCAN", &key.key).await?,
            "list" => {
                let values: Vec<Vec<u8>> = redis::cmd("LRANGE")
                    .arg(&key.key)
                    .arg(0)
                    .arg(MAX_COLLECTION_PREVIEW_ITEMS as i64 - 1)
                    .query_async(&mut connection)
                    .await
                    .map_err(|error| redis_error(error, ErrorCategory::Network))?;
                values
                    .iter()
                    .enumerate()
                    .map(|(i, value)| format!("{i}\t{}", display_bytes(value)))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            "zset" => {
                let values: Vec<Vec<u8>> = redis::cmd("ZRANGE")
                    .arg(&key.key)
                    .arg(0)
                    .arg(MAX_COLLECTION_PREVIEW_ITEMS as i64 - 1)
                    .arg("WITHSCORES")
                    .query_async(&mut connection)
                    .await
                    .map_err(|error| redis_error(error, ErrorCategory::Network))?;
                values
                    .chunks(2)
                    .map(|pair| {
                        format!(
                            "{}\t{}",
                            display_bytes(&pair[0]),
                            pair.get(1).map_or(String::new(), |v| display_bytes(v))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            other => format!("<preview for {other} is not supported>"),
        };
        Ok(format!("{header}{body}"))
    }
}

async fn scan_values(
    connection: &mut redis::aio::MultiplexedConnection,
    command_name: &str,
    key: &[u8],
) -> Result<String, DatabaseError> {
    let reply: (u64, Vec<Vec<u8>>) = redis::cmd(command_name)
        .arg(key)
        .arg(0)
        .arg("COUNT")
        .arg(MAX_COLLECTION_PREVIEW_ITEMS)
        .query_async(connection)
        .await
        .map_err(|error| redis_error(error, ErrorCategory::Network))?;
    Ok(reply
        .1
        .iter()
        .map(|value| display_bytes(value))
        .collect::<Vec<_>>()
        .join("\n"))
}

async fn scan_pairs(
    connection: &mut redis::aio::MultiplexedConnection,
    command_name: &str,
    key: &[u8],
) -> Result<String, DatabaseError> {
    let reply: (u64, Vec<Vec<u8>>) = redis::cmd(command_name)
        .arg(key)
        .arg(0)
        .arg("COUNT")
        .arg(MAX_COLLECTION_PREVIEW_ITEMS)
        .query_async(connection)
        .await
        .map_err(|error| redis_error(error, ErrorCategory::Network))?;
    Ok(reply
        .1
        .chunks(2)
        .map(|pair| {
            format!(
                "{}\t{}",
                display_bytes(&pair[0]),
                pair.get(1).map_or(String::new(), |v| display_bytes(v))
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

fn display_bytes(value: &[u8]) -> String {
    String::from_utf8(value.to_vec())
        .unwrap_or_else(|_| value.iter().map(|byte| format!("\\x{byte:02x}")).collect())
}

fn redis_error(error: redis::RedisError, category: ErrorCategory) -> DatabaseError {
    DatabaseError {
        category,
        code: error.code().map(str::to_owned),
        message: crate::security::sanitize_terminal_text(&error.to_string()),
        diagnostic: None,
    }
}
