use uuid::Uuid;

use crate::{
    model::{
        execution_target::ExecutionTarget,
        transaction::{TransactionMode, TransactionState},
    },
    sql::{ScopeKind, ScopeSource, SqlDialect},
};

/// Immutable request retained while the connection needed by a console is established.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingExecution {
    pub request_id: u64,
    pub console_id: Uuid,
    pub target: Option<ExecutionTarget>,
    pub document_revision: u64,
    pub scope: ScopeKind,
    pub source: ScopeSource,
    pub sql: String,
    pub dialect: SqlDialect,
    pub transaction_generation: u64,
    pub transaction_mode: TransactionMode,
    pub transaction_state: TransactionState,
}
