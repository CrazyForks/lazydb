pub mod discovery;
pub mod key_index;
pub mod key_store;
pub mod metadata_cache;
pub mod monitor;
pub mod preview_scheduler;
pub mod read;
pub mod reconnect;
pub mod reply;
pub mod scan_scheduler;
pub mod types;

use redis::{AsyncConnectionConfig, Client, aio::MultiplexedConnection};
use secrecy::{ExposeSecret, SecretString};

use crate::{
    db::{DatabaseError, ErrorCategory, ServerInfo},
    profile::{ConnectionProfile, DatabaseKind, SslMode},
};

#[derive(Clone, Debug)]
pub struct RedisAdapter {
    connection: MultiplexedConnection,
    target_database: u32,
}

impl RedisAdapter {
    pub async fn load_monitor_snapshot(
        &self,
    ) -> Result<crate::db::monitor::MonitorSnapshot, DatabaseError> {
        let mut connection = self.connection.clone();
        let raw = redis::cmd("INFO")
            .query_async::<String>(&mut connection)
            .await
            .map_err(|error| redis_error(error, crate::db::ErrorCategory::Network))?;
        let (mut snapshot, details) =
            monitor::parse_info(&raw, chrono::Utc::now().timestamp_millis() as u64);
        snapshot.redis_details = Some(details);
        Ok(snapshot)
    }

    pub async fn connect(
        profile: &ConnectionProfile,
        password: Option<&SecretString>,
    ) -> Result<Self, DatabaseError> {
        let host = profile
            .host
            .as_deref()
            .ok_or_else(|| DatabaseError::configuration("Redis host is required"))?;
        let port = profile.port.unwrap_or(6379);
        let database = parse_database(profile.database.as_deref())?;
        let scheme = match profile.ssl_mode {
            SslMode::Disable => "redis",
            SslMode::Require | SslMode::VerifyCa | SslMode::VerifyFull => "rediss",
            SslMode::Prefer => {
                return Err(DatabaseError::configuration(
                    "Redis TLS mode must be disable or require",
                ));
            }
        };
        let credentials = password
            .map(|password| {
                let user = profile.user.as_deref().unwrap_or("default");
                format!(
                    "{}:{}@",
                    encode_url_part(user),
                    encode_url_part(password.expose_secret())
                )
            })
            .or_else(|| {
                profile
                    .user
                    .as_deref()
                    .map(|user| format!("{}@", encode_url_part(user)))
            })
            .unwrap_or_default();
        let url = format!("{scheme}://{credentials}{host}:{port}/{database}");
        let client =
            Client::open(url).map_err(|error| redis_error(error, ErrorCategory::Configuration))?;
        let config = AsyncConnectionConfig::new()
            .set_connection_timeout(Some(std::time::Duration::from_secs(5)))
            .set_response_timeout(Some(std::time::Duration::from_secs(10)));
        let connection = client
            .get_multiplexed_async_connection_with_config(&config)
            .await
            .map_err(|error| {
                let category = if password.is_some() {
                    ErrorCategory::Authentication
                } else {
                    ErrorCategory::Network
                };
                redis_error(error, category)
            })?;
        let adapter = Self {
            connection,
            target_database: database,
        };
        adapter.ping().await?;
        Ok(adapter)
    }

    pub async fn ping(&self) -> Result<(), DatabaseError> {
        let mut connection = self.connection.clone();
        redis::cmd("PING")
            .query_async::<String>(&mut connection)
            .await
            .map(|_| ())
            .map_err(|error| redis_error(error, ErrorCategory::Network))
    }

    pub async fn scan_keys(
        &self,
        cursor: u64,
        pattern: &[u8],
        count_hint: u32,
    ) -> Result<(u64, Vec<Vec<u8>>), DatabaseError> {
        let mut connection = self.connection.clone();
        let mut command = redis::cmd("SCAN");
        command.arg(cursor).arg("MATCH").arg(pattern);
        if count_hint > 0 {
            command.arg("COUNT").arg(count_hint);
        }
        command
            .query_async(&mut connection)
            .await
            .map_err(|error| redis_error(error, ErrorCategory::Network))
    }

    pub async fn probe(&self) -> Result<ServerInfo, DatabaseError> {
        let mut connection = self.connection.clone();
        let version = redis::cmd("INFO")
            .arg("server")
            .query_async::<String>(&mut connection)
            .await
            .ok()
            .and_then(|info| {
                info.lines()
                    .find_map(|line| line.strip_prefix("redis_version:"))
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "unknown".to_owned());
        Ok(ServerInfo {
            kind: DatabaseKind::Redis,
            version,
            database: self.target_database.to_string(),
            current_user: None,
        })
    }

    pub fn database(&self) -> u32 {
        self.target_database
    }

    pub async fn close(self) {
        drop(self.connection);
    }

    pub(crate) fn connection_clone(&self) -> MultiplexedConnection {
        self.connection.clone()
    }
}

fn parse_database(value: Option<&str>) -> Result<u32, DatabaseError> {
    value
        .unwrap_or("0")
        .parse::<u32>()
        .map_err(|_| DatabaseError::configuration("Redis database must be a non-negative integer"))
}

fn encode_url_part(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn redis_error(error: redis::RedisError, default_category: ErrorCategory) -> DatabaseError {
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    let category = if lower.contains("wrongpass") || lower.contains("noauth") {
        ErrorCategory::Authentication
    } else if lower.contains("noperm") {
        ErrorCategory::Permission
    } else if lower.contains("moved") || lower.contains("ask") {
        ErrorCategory::Unsupported
    } else {
        default_category
    };
    DatabaseError {
        category,
        code: error.code().map(str::to_owned),
        message: crate::security::sanitize_terminal_text(&message),
        diagnostic: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redis_error_classifies_auth_permission_and_cluster_redirects() {
        let error = redis::RedisError::from((redis::ErrorKind::AuthenticationFailed, "WRONGPASS"));
        assert_eq!(
            redis_error(error, ErrorCategory::Network).category,
            ErrorCategory::Authentication
        );
        let error = redis::RedisError::from((redis::ErrorKind::InvalidClientConfig, "NOPERM"));
        assert_eq!(
            redis_error(error, ErrorCategory::Network).category,
            ErrorCategory::Permission
        );
    }
}
