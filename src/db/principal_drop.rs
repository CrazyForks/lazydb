use thiserror::Error;

use crate::{db::principal::PrincipalEntry, identity::ConnectionIdentity};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalDropRequest {
    pub connection: ConnectionIdentity,
    pub request_id: u64,
    pub entry: PrincipalEntry,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum PrincipalDropError {
    #[error("principal drop profile does not match the connection")]
    ProfileMismatch,
    #[error("principal drop requires a non-empty principal name")]
    EmptyName,
    #[error("principal drop is not supported by this database")]
    Unsupported,
    #[error("principal drop SQL is invalid")]
    InvalidSql,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalDropPlan {
    pub request: PrincipalDropRequest,
    sql: String,
}

impl PrincipalDropRequest {
    pub fn validate(&self) -> Result<(), PrincipalDropError> {
        if self.request_id == 0 {
            return Err(PrincipalDropError::InvalidSql);
        }
        if self.entry.id.profile_id != self.connection.profile_id {
            return Err(PrincipalDropError::ProfileMismatch);
        }
        if self.entry.name.trim().is_empty() {
            return Err(PrincipalDropError::EmptyName);
        }
        Ok(())
    }
}

impl PrincipalDropPlan {
    pub fn new(
        request: PrincipalDropRequest,
        sql: impl Into<String>,
    ) -> Result<Self, PrincipalDropError> {
        request.validate()?;
        let sql = sql.into();
        if sql.trim().is_empty()
            || !sql
                .trim_start()
                .to_ascii_uppercase()
                .starts_with("DROP ROLE ")
        {
            return Err(PrincipalDropError::InvalidSql);
        }
        Ok(Self { request, sql })
    }

    pub fn sql(&self) -> &str {
        &self.sql
    }
}
