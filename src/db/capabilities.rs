use crate::profile::DatabaseKind;

/// Static capabilities describe what an adapter can expose before connecting.
/// Permission errors and server-version gates remain runtime results.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatabaseCapabilities {
    pub catalog: bool,
    pub relation_ddl: bool,
    pub relation_edit: bool,
    pub monitoring: bool,
    pub manual_transactions: bool,
    pub cancellation: bool,
}

impl DatabaseCapabilities {
    pub const fn for_kind(kind: DatabaseKind) -> Self {
        match kind {
            DatabaseKind::Postgres => Self {
                catalog: true,
                relation_ddl: true,
                relation_edit: true,
                monitoring: true,
                manual_transactions: true,
                cancellation: true,
            },
            DatabaseKind::MySql => Self {
                catalog: true,
                relation_ddl: true,
                relation_edit: false,
                monitoring: true,
                manual_transactions: true,
                cancellation: true,
            },
            DatabaseKind::MariaDb => Self {
                catalog: true,
                relation_ddl: true,
                relation_edit: false,
                monitoring: true,
                manual_transactions: true,
                cancellation: true,
            },
            DatabaseKind::SqlServer => Self {
                catalog: true,
                relation_ddl: true,
                relation_edit: false,
                monitoring: false,
                manual_transactions: true,
                cancellation: true,
            },
            DatabaseKind::Sqlite => Self {
                catalog: true,
                relation_ddl: true,
                relation_edit: false,
                monitoring: false,
                manual_transactions: true,
                cancellation: true,
            },
            DatabaseKind::Oracle => Self {
                catalog: true,
                relation_ddl: true,
                relation_edit: false,
                monitoring: false,
                manual_transactions: true,
                cancellation: false,
            },
        }
    }
}
