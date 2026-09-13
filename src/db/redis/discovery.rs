use std::collections::BTreeMap;

use redis::Value;

use super::RedisAdapter;
use crate::db::{DatabaseError, ErrorCategory};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedisDiscoveryCompleteness {
    Complete,
    Partial,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisDatabaseInfo {
    pub database: u32,
    pub keys: Option<u64>,
    pub expires: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisDatabaseDiscovery {
    pub databases: Vec<RedisDatabaseInfo>,
    pub completeness: RedisDiscoveryCompleteness,
    pub warnings: Vec<String>,
}

impl RedisAdapter {
    pub async fn discover_databases(
        &self,
        current_database: u32,
        explicit_databases: &[u32],
    ) -> Result<RedisDatabaseDiscovery, DatabaseError> {
        let mut connection = self.connection_clone();
        let configured = redis::cmd("CONFIG")
            .arg("GET")
            .arg("databases")
            .query_async::<Value>(&mut connection)
            .await;
        let keyspace = redis::cmd("INFO")
            .arg("keyspace")
            .query_async::<String>(&mut connection)
            .await;

        let stats = parse_keyspace(keyspace.as_ref().ok().map(String::as_str));
        let mut warnings = Vec::new();
        let databases = if let Ok(value) = configured {
            let count = parse_config_databases(value)?;
            (0..count)
                .map(|database| RedisDatabaseInfo {
                    database,
                    keys: stats.get(&database).and_then(|value| value.0),
                    expires: stats.get(&database).and_then(|value| value.1),
                })
                .collect()
        } else {
            warnings.push(
                "Redis CONFIG databases is unavailable; the DB list may be incomplete".into(),
            );
            let mut ids = vec![current_database];
            ids.extend(explicit_databases.iter().copied());
            ids.extend(stats.keys().copied());
            ids.sort_unstable();
            ids.dedup();
            ids.into_iter()
                .map(|database| {
                    let (keys, expires) = stats.get(&database).copied().unwrap_or((None, None));
                    RedisDatabaseInfo {
                        database,
                        keys,
                        expires,
                    }
                })
                .collect()
        };
        if keyspace.is_err() {
            warnings.push("Redis INFO keyspace is unavailable; key counts are unknown".into());
        }
        Ok(RedisDatabaseDiscovery {
            databases,
            completeness: if warnings.is_empty() {
                RedisDiscoveryCompleteness::Complete
            } else {
                RedisDiscoveryCompleteness::Partial
            },
            warnings,
        })
    }
}

fn parse_config_databases(value: Value) -> Result<u32, DatabaseError> {
    let values = match value {
        Value::Array(values) => values,
        _ => {
            return Err(discovery_error(
                "Redis CONFIG databases returned an invalid reply",
            ));
        }
    };
    let raw = values
        .get(1)
        .and_then(value_as_bytes)
        .ok_or_else(|| discovery_error("Redis CONFIG databases returned no count"))?;
    String::from_utf8(raw)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|count| *count <= 1_000_000)
        .ok_or_else(|| discovery_error("Redis CONFIG databases returned an invalid count"))
}

fn parse_keyspace(info: Option<&str>) -> BTreeMap<u32, (Option<u64>, Option<u64>)> {
    let mut result = BTreeMap::new();
    for line in info.unwrap_or_default().lines() {
        let Some((database, values)) = line
            .strip_prefix("db")
            .and_then(|line| line.split_once(':'))
        else {
            continue;
        };
        let Ok(database) = database.parse::<u32>() else {
            continue;
        };
        let mut keys = None;
        let mut expires = None;
        for field in values.split(',') {
            let Some((name, value)) = field.split_once('=') else {
                continue;
            };
            match name {
                "keys" => keys = value.parse().ok(),
                "expires" => expires = value.parse().ok(),
                _ => {}
            }
        }
        result.insert(database, (keys, expires));
    }
    result
}

fn value_as_bytes(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::BulkString(value) => Some(value.clone()),
        Value::SimpleString(value) => Some(value.as_bytes().to_vec()),
        _ => None,
    }
}

fn discovery_error(message: &'static str) -> DatabaseError {
    DatabaseError {
        category: ErrorCategory::Permission,
        code: Some("redis_database_discovery_failed".into()),
        message: message.into(),
        diagnostic: None,
    }
}
