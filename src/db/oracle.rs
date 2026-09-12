#[cfg(feature = "driver-oracle")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "driver-oracle")]
use std::time::Instant;

#[cfg(feature = "driver-oracle")]
use secrecy::ExposeSecret;
use secrecy::SecretString;

use super::catalog::{
    CatalogCount, CatalogEntry, CatalogId, CatalogKind, CatalogMetadata, CatalogPage,
    CatalogRequest, CatalogTarget, ColumnMetadata, DdlProvenance, DiscoveredDatabase, ObjectGroup,
    OptionalMetadata, QualifiedName, RelationDdl, finalize_keyset_page,
};
#[cfg(feature = "driver-oracle")]
use super::query::QueryStats;
#[cfg(feature = "driver-oracle")]
use super::query::{ColumnMeta, ResultSet};
use super::query::{QueryBudget, QueryOutcome};
use super::transaction::{TransactionBackend, TransactionError};
use super::{DatabaseError, ErrorCategory, ServerInfo};
use crate::db::RelationPreview;
use crate::profile::{ConnectionProfile, DatabaseKind};
#[cfg(feature = "driver-oracle")]
use crate::security::sanitize_terminal_text;
use futures_util::future::BoxFuture;

#[derive(Clone)]
pub struct OracleAdapter {
    #[cfg(feature = "driver-oracle")]
    connection: Arc<Mutex<oracle::Connection>>,
    connection_id: uuid::Uuid,
    database: String,
}

impl std::fmt::Debug for OracleAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OracleAdapter")
            .field("connection_id", &self.connection_id)
            .finish_non_exhaustive()
    }
}

impl OracleAdapter {
    pub async fn connect(
        profile: &ConnectionProfile,
        password: Option<&SecretString>,
    ) -> Result<Self, DatabaseError> {
        #[cfg(not(feature = "driver-oracle"))]
        {
            let _ = (profile, password);
            Err(DatabaseError {
                category: ErrorCategory::Unsupported,
                code: Some("oracle_driver_not_enabled".into()),
                message: "Oracle support requires the driver-oracle feature".into(),
                diagnostic: None,
            })
        }
        #[cfg(feature = "driver-oracle")]
        {
            if profile.kind != DatabaseKind::Oracle {
                return Err(DatabaseError::configuration("profile is not Oracle"));
            }
            let host = profile
                .host
                .clone()
                .ok_or_else(|| DatabaseError::configuration("Oracle profile has no host"))?;
            let port = profile.port.unwrap_or(1521);
            let service = profile.database.clone().ok_or_else(|| {
                DatabaseError::configuration("Oracle profile has no service name")
            })?;
            let user = profile
                .user
                .clone()
                .ok_or_else(|| DatabaseError::configuration("Oracle profile has no user"))?;
            let password = password
                .map(|value| value.expose_secret().to_owned())
                .ok_or_else(|| {
                    DatabaseError::configuration("Oracle profile requires a password")
                })?;
            let connect_string = format!("//{host}:{port}/{service}");
            let connection = tokio::task::spawn_blocking(move || {
                super::oracle_client::initialize()?;
                oracle::Connection::connect(user, password, connect_string)
                    .map_err(|error| oracle_error(error.to_string()))
            })
            .await
            .map_err(|error| DatabaseError {
                category: ErrorCategory::Internal,
                code: Some("oracle_connect_task_failed".into()),
                message: sanitize_terminal_text(&error.to_string()),
                diagnostic: None,
            })?
            .map_err(|error| DatabaseError {
                category: ErrorCategory::Network,
                code: Some("oracle_connect_failed".into()),
                message: sanitize_terminal_text(&error.to_string()),
                diagnostic: None,
            })?;
            Ok(Self {
                connection: Arc::new(Mutex::new(connection)),
                connection_id: profile.id,
                database: service,
            })
        }
    }

    pub async fn probe(&self) -> Result<ServerInfo, DatabaseError> {
        #[cfg(not(feature = "driver-oracle"))]
        {
            Err(DatabaseError {
                category: ErrorCategory::Unsupported,
                code: Some("oracle_driver_not_enabled".into()),
                message: "Oracle support requires the driver-oracle feature".into(),
                diagnostic: None,
            })
        }
        #[cfg(feature = "driver-oracle")]
        {
            let connection = Arc::clone(&self.connection);
            tokio::task::spawn_blocking(move || {
                let connection = connection
                    .lock()
                    .map_err(|_| oracle_error("Oracle connection lock poisoned"))?;
                let row = connection
                    .query_row(
                        "SELECT SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA'), \
                            SYS_CONTEXT('USERENV', 'SERVICE_NAME'), \
                            (SELECT version FROM product_component_version WHERE product LIKE 'Oracle Database%') FROM dual",
                        &[],
                    )
                    .map_err(oracle_error)?;
                let schema: String = row.get(0).map_err(oracle_error)?;
                let service: String = row.get(1).map_err(oracle_error)?;
                let version: Option<String> = row.get(2).map_err(oracle_error)?;
                Ok(ServerInfo {
                    kind: DatabaseKind::Oracle,
                    version: version.unwrap_or_else(|| "unknown".to_owned()),
                    database: service,
                    current_user: Some(schema),
                })
            })
            .await
            .map_err(|error| oracle_task_error(error.to_string()))?
        }
    }

    pub async fn discover_catalog_scope(
        &self,
    ) -> Result<super::catalog::CatalogDiscovery, DatabaseError> {
        #[cfg(not(feature = "driver-oracle"))]
        {
            Err(oracle_disabled())
        }
        #[cfg(feature = "driver-oracle")]
        {
            let connection = Arc::clone(&self.connection);
            let database = self.database.clone();
            tokio::task::spawn_blocking(move || {
                let connection = connection
                    .lock()
                    .map_err(|_| oracle_error("Oracle connection lock poisoned"))?;
                let row = connection
                    .query_row(
                        "SELECT SYS_CONTEXT('USERENV', 'SERVICE_NAME'), SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM dual",
                        &[],
                    )
                    .map_err(oracle_error)?;
                let _service: String = row.get(0).map_err(oracle_error)?;
                let schema: String = row.get(1).map_err(oracle_error)?;
                Ok(super::catalog::CatalogDiscovery {
                    databases: vec![DiscoveredDatabase {
                        name: database,
                        schemas: vec![schema],
                    }],
                    warnings: Vec::new(),
                })
            })
            .await
            .map_err(|error| oracle_task_error(error.to_string()))?
        }
    }

    pub async fn load_catalog_page(
        &self,
        request: &CatalogRequest,
    ) -> Result<CatalogPage, DatabaseError> {
        #[cfg(not(feature = "driver-oracle"))]
        {
            let _ = request;
            Err(oracle_disabled())
        }
        #[cfg(feature = "driver-oracle")]
        {
            request
                .validate_for_profile(self.connection_id)
                .map_err(DatabaseError::invalid_catalog_request)?;
            let request = request.clone();
            let connection = Arc::clone(&self.connection);
            let database = self.database.clone();
            tokio::task::spawn_blocking(move || {
                let connection = connection
                    .lock()
                    .map_err(|_| oracle_error("Oracle connection lock poisoned"))?;
                let mut entries = oracle_catalog_entries(&connection, &request, &database)?;
                if matches!(request.key.target, CatalogTarget::Groups { .. }) {
                    return oracle_group_page(&request);
                }
                let next_cursor = finalize_keyset_page(
                    &mut entries,
                    request.page_size,
                    |entry| entry.qualified_name.object.clone(),
                    |entry| entry.id.native_path.join("."),
                )
                .map_err(|error| oracle_error(error.to_string()))?;
                CatalogPage::new(&request, entries, CatalogCount::Unknown, next_cursor)
                    .map_err(|error| oracle_error(error.to_string()))
            })
            .await
            .map_err(|error| oracle_task_error(error.to_string()))?
        }
    }

    pub async fn preview_relation(
        &self,
        relation: &CatalogId,
        options: &crate::model::relation::RelationPreviewOptions,
        page: crate::model::pagination::PageRequest,
    ) -> Result<RelationPreview, DatabaseError> {
        if relation.profile_id() != self.connection_id || !relation.kind.is_relation() {
            return Err(DatabaseError::configuration(
                "invalid Oracle relation target",
            ));
        }
        let [database, schema, name] = relation.native_path.as_slice() else {
            return Err(DatabaseError::configuration(
                "invalid Oracle relation identity",
            ));
        };
        if database != &self.database {
            return Err(DatabaseError::configuration(
                "Oracle relation belongs to another service",
            ));
        }
        let qualified = format!("{}.{}", quote_identifier(schema), quote_identifier(name));
        let mut sql = format!("SELECT * FROM {qualified}");
        if let Some(clause) = &options.where_clause {
            sql.push_str(" WHERE ");
            sql.push_str(clause);
        }
        if let Some(clause) = &options.order_by_clause {
            sql.push_str(" ORDER BY ");
            sql.push_str(clause);
        }
        let limit = page.size.lookahead_limit();
        sql.push_str(&format!(
            " OFFSET {} ROWS FETCH NEXT {limit} ROWS ONLY",
            page.offset
        ));
        let result = self
            .execute_pool_with_budget(
                &sql,
                QueryBudget {
                    max_rows: limit,
                    max_bytes: usize::MAX,
                },
            )
            .await?;
        let fetched_len = result.result_sets.first().map_or(0, |set| set.rows.len());
        let pagination = crate::model::pagination::ResultPagination::from_page(page, fetched_len);
        Ok(RelationPreview {
            sql,
            result,
            pagination,
            row_versions: None,
        })
    }

    pub async fn relation_ddl(&self, relation: &CatalogId) -> Result<RelationDdl, DatabaseError> {
        let [database, schema, _] = relation.native_path.as_slice() else {
            return Err(DatabaseError::configuration(
                "invalid Oracle relation identity",
            ));
        };
        let scope =
            crate::profile::CatalogScope::for_profile(DatabaseKind::Oracle, database, Some(schema));
        self.relation_ddl_with_scope(relation, &scope).await
    }

    pub async fn relation_ddl_with_scope(
        &self,
        relation: &CatalogId,
        scope: &crate::profile::CatalogScope,
    ) -> Result<RelationDdl, DatabaseError> {
        if relation.profile_id() != self.connection_id || !relation.kind.is_relation() {
            return Err(DatabaseError::configuration(
                "invalid Oracle relation target",
            ));
        }
        let [database, schema, name] = relation.native_path.as_slice() else {
            return Err(DatabaseError::configuration(
                "invalid Oracle relation identity",
            ));
        };
        if database != &self.database {
            return Err(DatabaseError::configuration(
                "Oracle relation belongs to another service",
            ));
        }
        if !scope.allows_schema(database, schema) {
            return Err(DatabaseError::configuration(
                "Oracle relation is outside the active catalog scope",
            ));
        }
        let relation_entry = CatalogEntry::relation(
            relation.clone(),
            CatalogId::new(self.connection_id, CatalogKind::Schema, [database, schema]),
            QualifiedName {
                database: Some(database.clone()),
                schema: Some(schema.clone()),
                object: name.clone(),
            },
            match relation.kind {
                CatalogKind::Table => "TABLE",
                CatalogKind::View => "VIEW",
                CatalogKind::MaterializedView => "MATERIALIZED_VIEW",
                _ => unreachable!("relation kind was validated above"),
            },
            OptionalMetadata::Unsupported,
            true,
        )
        .map_err(|error| DatabaseError::configuration(error.to_string()))?;
        let request = CatalogRequest {
            key: super::catalog::CatalogRequestKey {
                connection: crate::identity::ConnectionIdentity {
                    profile_id: self.connection_id,
                    generation: 0,
                },
                catalog_epoch: 0,
                request_id: 0,
                target: CatalogTarget::RelationChildren {
                    relation: relation.clone(),
                },
                cursor: None,
            },
            scope: scope.clone(),
            page_size: 500,
        };
        let children = self.load_catalog_page(&request).await?;
        if children.completeness != super::catalog::CatalogCompleteness::Complete {
            return Err(DatabaseError::configuration(
                "Oracle relation metadata exceeds the catalog page limit",
            ));
        }
        let sql = self
            .native_relation_ddl(relation.kind, schema, name)
            .await?;
        Ok(RelationDdl {
            relation: relation_entry,
            children,
            sql,
            provenance: DdlProvenance::NativeCatalog,
        })
    }

    async fn native_relation_ddl(
        &self,
        kind: CatalogKind,
        schema: &str,
        name: &str,
    ) -> Result<String, DatabaseError> {
        #[cfg(not(feature = "driver-oracle"))]
        {
            let _ = (kind, schema, name);
            Err(oracle_disabled())
        }
        #[cfg(feature = "driver-oracle")]
        {
            let connection = Arc::clone(&self.connection);
            let object_type = match kind {
                CatalogKind::Table => "TABLE",
                CatalogKind::View => "VIEW",
                CatalogKind::MaterializedView => "MATERIALIZED_VIEW",
                _ => {
                    return Err(DatabaseError::configuration(
                        "unsupported Oracle relation kind",
                    ));
                }
            };
            let schema = schema.to_owned();
            let name = name.to_owned();
            tokio::task::spawn_blocking(move || {
                let connection = connection
                    .lock()
                    .map_err(|_| oracle_error("Oracle connection lock poisoned"))?;
                let row = connection.query_row(
                    "SELECT DBMS_METADATA.GET_DDL(:1, :2, :3) FROM dual",
                    &[&object_type, &name, &schema],
                );
                match row {
                    Ok(row) => {
                        let ddl: Option<String> = row.get(0).map_err(oracle_error)?;
                        ddl.filter(|value| !value.trim().is_empty())
                            .ok_or_else(|| oracle_error("Oracle returned empty relation DDL"))
                    }
                    Err(error) => Err(oracle_error(error)),
                }
            })
            .await
            .map_err(|error| oracle_task_error(error.to_string()))?
        }
    }

    pub(crate) async fn execute_pool_with_budget(
        &self,
        sql: &str,
        budget: QueryBudget,
    ) -> Result<QueryOutcome, DatabaseError> {
        #[cfg(not(feature = "driver-oracle"))]
        {
            let _ = (sql, budget);
            Err(DatabaseError {
                category: ErrorCategory::Unsupported,
                code: Some("oracle_driver_not_enabled".into()),
                message: "Oracle support requires the driver-oracle feature".into(),
                diagnostic: None,
            })
        }
        #[cfg(feature = "driver-oracle")]
        {
            let sql = sql.to_owned();
            let connection = Arc::clone(&self.connection);
            tokio::task::spawn_blocking(move || {
                let connection = connection
                    .lock()
                    .map_err(|_| oracle_error("Oracle connection lock poisoned"))?;
                let mut statement = connection.statement(&sql).build().map_err(oracle_error)?;
                if !statement.is_query() {
                    statement.execute(&[]).map_err(oracle_error)?;
                    let affected_rows = statement.row_count().map_err(oracle_error)?;
                    return Ok(QueryOutcome::from_result_set(
                        ResultSet {
                            columns: Vec::new(),
                            rows: Vec::new(),
                            affected_rows,
                        },
                        std::time::Duration::ZERO,
                        std::time::Duration::ZERO,
                    ));
                }
                let rows = statement.query(&[]).map_err(oracle_error)?;
                let columns = rows
                    .column_info()
                    .iter()
                    .map(|column| ColumnMeta {
                        name: column.name().to_owned(),
                        type_name: column.oracle_type().to_string(),
                    })
                    .collect::<Vec<_>>();
                let mut result = ResultSet {
                    columns,
                    rows: Vec::new(),
                    affected_rows: 0,
                };
                let started = Instant::now();
                let mut fetched_row_count: usize = 0;
                let mut retained_bytes = 0usize;
                let mut truncated = false;
                for row in rows {
                    let row = row.map_err(oracle_error)?;
                    fetched_row_count = fetched_row_count.saturating_add(1);
                    if result.rows.len() >= budget.max_rows {
                        truncated = true;
                        break;
                    }
                    let values = (0..result.columns.len())
                        .map(|index| decode_value(&row, index, &result.columns[index].type_name))
                        .collect::<Result<Vec<_>, DatabaseError>>()?;
                    let row_bytes = serde_json::to_vec(&values)
                        .map_err(|error| oracle_error(error.to_string()))?
                        .len();
                    if budget.max_bytes != usize::MAX
                        && retained_bytes.saturating_add(row_bytes) > budget.max_bytes
                    {
                        truncated = true;
                        break;
                    }
                    retained_bytes = retained_bytes.saturating_add(row_bytes);
                    result.rows.push(values);
                }
                let elapsed = started.elapsed();
                let row_count = result.rows.len();
                Ok(QueryOutcome {
                    result_sets: vec![result],
                    stats: QueryStats {
                        execution: elapsed,
                        fetch: std::time::Duration::ZERO,
                        row_count,
                        fetched_row_count,
                        truncated,
                    },
                })
            })
            .await
            .map_err(|error| oracle_task_error(error.to_string()))?
        }
    }

    pub async fn close(self) {
        let _ = self;
    }

    pub(crate) async fn transaction_backend(
        &self,
    ) -> Result<OracleTransactionBackend, DatabaseError> {
        Ok(OracleTransactionBackend {
            adapter: self.clone(),
            depth: 0,
        })
    }
}

pub(crate) struct OracleTransactionBackend {
    adapter: OracleAdapter,
    depth: usize,
}

#[async_trait::async_trait]
impl TransactionBackend for OracleTransactionBackend {
    async fn begin(&mut self) -> Result<(), TransactionError> {
        self.adapter
            .execute_pool_with_budget("BEGIN", QueryBudget::UNBOUNDED)
            .await
            .map(|_| {
                self.depth = self.depth.saturating_add(1);
            })
            .map_err(TransactionError::from)
    }

    async fn execute(&mut self, sql: &str) -> Result<QueryOutcome, TransactionError> {
        self.adapter
            .execute_pool_with_budget(sql, QueryBudget::UNBOUNDED)
            .await
            .map_err(TransactionError::from)
    }

    async fn commit(&mut self) -> Result<(), TransactionError> {
        self.control_statement("COMMIT").await
    }

    async fn rollback(&mut self) -> Result<(), TransactionError> {
        self.control_statement("ROLLBACK").await
    }

    async fn cancel(&mut self) -> Result<(), TransactionError> {
        Err(TransactionError(
            "Oracle cancellation is unavailable while the native client call is active".into(),
        ))
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn force_close(self) -> BoxFuture<'static, Result<(), TransactionError>> {
        Box::pin(async move {
            drop(self);
            Ok(())
        })
    }
}

impl OracleTransactionBackend {
    async fn control_statement(&mut self, sql: &str) -> Result<(), TransactionError> {
        self.adapter
            .execute_pool_with_budget(sql, QueryBudget::UNBOUNDED)
            .await
            .map(|_| {
                self.depth = 0;
            })
            .map_err(TransactionError::from)
    }
}

pub fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[cfg(feature = "driver-oracle")]
fn oracle_group_page(request: &CatalogRequest) -> Result<CatalogPage, DatabaseError> {
    let groups = [
        (ObjectGroup::Tables, "tables"),
        (ObjectGroup::Views, "views"),
        (ObjectGroup::Sequences, "sequences"),
    ]
    .into_iter()
    .map(|(group, _)| super::catalog::CatalogGroupSummary {
        group,
        object_count: CatalogCount::Unknown,
    })
    .collect();
    CatalogPage::groups(request, groups, CatalogCount::Unknown, None)
        .map_err(|error| oracle_error(error.to_string()))
}

#[cfg(feature = "driver-oracle")]
fn oracle_catalog_entries(
    connection: &oracle::Connection,
    request: &CatalogRequest,
    configured_database: &str,
) -> Result<Vec<CatalogEntry>, DatabaseError> {
    let _service = match &request.key.target {
        CatalogTarget::Databases => None,
        CatalogTarget::Schemas { database } => database.native_path.last().cloned(),
        CatalogTarget::Groups { schema } | CatalogTarget::Objects { schema, .. } => {
            schema.native_path.last().cloned()
        }
        CatalogTarget::RelationChildren { .. } => None,
    };
    let schema = match &request.key.target {
        CatalogTarget::Groups { schema } | CatalogTarget::Objects { schema, .. } => {
            schema.native_path.last().cloned()
        }
        _ => None,
    };
    let limit = request.page_size.saturating_add(1);
    let rows = match &request.key.target {
        CatalogTarget::Databases => connection
            .query(
                "SELECT SYS_CONTEXT('USERENV', 'SERVICE_NAME') FROM dual",
                &[],
            )
            .map_err(oracle_error)?,
        CatalogTarget::Schemas { .. } => connection
            .query(
                &format!("SELECT SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM dual FETCH FIRST {limit} ROWS ONLY"),
                &[],
            )
            .map_err(oracle_error)?,
        CatalogTarget::Objects { group, .. } => {
            let view = match group {
                ObjectGroup::Tables => "all_tables",
                ObjectGroup::Views => "all_views",
                ObjectGroup::Sequences => "all_sequences",
                _ => return Ok(Vec::new()),
            };
            let column = match group {
                ObjectGroup::Views => "view_name",
                _ => "table_name",
            };
            let owner_filter = schema.as_deref().unwrap_or_default().to_owned();
            connection
                .query(
                    &format!(
                        "SELECT {column} FROM {view} WHERE owner = :1 ORDER BY {column} FETCH FIRST {limit} ROWS ONLY"
                    ),
                    &[&owner_filter],
                )
                .map_err(oracle_error)?
        }
        CatalogTarget::Groups { .. } => return Ok(Vec::new()),
        CatalogTarget::RelationChildren { relation } => {
            let [database, schema, table] = relation.native_path.as_slice() else {
                return Ok(Vec::new());
            };
            let owner = schema.clone();
            let table = table.clone();
            let rows = connection
                .query(
                    &format!(
                        "SELECT column_name, data_type, nullable, data_default, data_length, data_precision, data_scale, column_id FROM all_tab_columns WHERE owner = :1 AND table_name = :2 ORDER BY column_id FETCH FIRST {limit} ROWS ONLY"
                    ),
                    &[&owner, &table],
                )
                .map_err(oracle_error)?;
            let mut entries = Vec::new();
            let index_rows = connection
                .query(
                    "SELECT i.index_name, i.uniqueness, c.column_name, c.column_position \
                     FROM all_indexes i \
                     JOIN all_ind_columns c ON c.index_owner = i.owner AND c.index_name = i.index_name \
                         AND c.table_owner = i.table_owner AND c.table_name = i.table_name \
                     WHERE i.table_owner = :1 AND i.table_name = :2 \
                     ORDER BY i.index_name, c.column_position",
                    &[&owner, &table],
                )
                .map_err(oracle_error)?;
            let mut indexes: std::collections::BTreeMap<String, (bool, Vec<String>)> =
                std::collections::BTreeMap::new();
            for row in index_rows {
                let row = row.map_err(oracle_error)?;
                let index_name: String = row.get(0).map_err(oracle_error)?;
                let unique: String = row.get(1).map_err(oracle_error)?;
                let column: Option<String> = row.get(2).map_err(oracle_error)?;
                let entry = indexes
                    .entry(index_name)
                    .or_insert_with(|| (unique == "UNIQUE", Vec::new()));
                if let Some(column) = column {
                    entry.1.push(column);
                }
            }
            for (index_name, (unique, columns)) in indexes {
                entries.push(
                    CatalogEntry::relation_child(
                        CatalogId::new(
                            request.key.connection.profile_id,
                            CatalogKind::Index,
                            [database.clone(), schema.clone(), table.clone(), index_name.clone()],
                        ),
                        relation.clone(),
                        QualifiedName {
                            database: Some(database.clone()),
                            schema: Some(schema.clone()),
                            object: index_name,
                        },
                        "index",
                        OptionalMetadata::Unsupported,
                        CatalogMetadata::Index(super::catalog::IndexMetadata { columns, unique }),
                    )
                    .map_err(|error| oracle_error(error.to_string()))?,
                );
            }
            let constraint_rows = connection
                .query(
                    "SELECT c.constraint_name, c.constraint_type, cc.column_name, cc.position FROM all_constraints c LEFT JOIN all_cons_columns cc ON cc.owner = c.owner AND cc.constraint_name = c.constraint_name AND cc.table_name = c.table_name WHERE c.owner = :1 AND c.table_name = :2 AND c.constraint_type IN ('P', 'U', 'C') ORDER BY c.constraint_name, cc.position",
                    &[&owner, &table],
                )
                .map_err(oracle_error)?;
            let mut constraints: std::collections::BTreeMap<
                String,
                (String, Vec<String>),
            > = std::collections::BTreeMap::new();
            for row in constraint_rows {
                let row = row.map_err(oracle_error)?;
                let constraint_name: String = row.get(0).map_err(oracle_error)?;
                let constraint_type: String = row.get(1).map_err(oracle_error)?;
                let column: Option<String> = row.get(2).map_err(oracle_error)?;
                let entry = constraints
                    .entry(constraint_name)
                    .or_insert_with(|| (constraint_type, Vec::new()));
                if let Some(column) = column {
                    entry.1.push(column);
                }
            }
            for (constraint_name, (constraint_type, columns)) in constraints {
                let kind = match constraint_type.as_str() {
                    "P" => CatalogKind::PrimaryKey,
                    "U" => CatalogKind::UniqueConstraint,
                    "C" => CatalogKind::CheckConstraint,
                    _ => continue,
                };
                let metadata = match kind {
                    CatalogKind::PrimaryKey => CatalogMetadata::Constraint(
                        super::catalog::ConstraintMetadata::PrimaryKey { columns },
                    ),
                    CatalogKind::UniqueConstraint => CatalogMetadata::Constraint(
                        super::catalog::ConstraintMetadata::Unique { columns },
                    ),
                    CatalogKind::CheckConstraint => CatalogMetadata::Constraint(
                        super::catalog::ConstraintMetadata::Check {
                            expression: constraint_name.clone(),
                        },
                    ),
                    _ => continue,
                };
                entries.push(
                    CatalogEntry::relation_child(
                        CatalogId::new(
                            request.key.connection.profile_id,
                            kind,
                            [database.clone(), schema.clone(), table.clone(), constraint_name.clone()],
                        ),
                        relation.clone(),
                        QualifiedName {
                            database: Some(database.clone()),
                            schema: Some(schema.clone()),
                            object: constraint_name,
                        },
                        "constraint",
                        OptionalMetadata::Unsupported,
                        metadata,
                    )
                    .map_err(|error| oracle_error(error.to_string()))?,
                );
            }
            for row in rows {
                let row = row.map_err(oracle_error)?;
                let name: String = row.get(0).map_err(oracle_error)?;
                let native_type: String = row.get(1).map_err(oracle_error)?;
                let nullable: String = row.get(2).map_err(oracle_error)?;
                let default_expression: Option<String> = row.get(3).map_err(oracle_error)?;
                let length: Option<i64> = row.get(4).map_err(oracle_error)?;
                let precision: Option<i64> = row.get(5).map_err(oracle_error)?;
                let scale: Option<i64> = row.get(6).map_err(oracle_error)?;
                let ordinal: i64 = row.get(7).map_err(oracle_error)?;
                let mut metadata = ColumnMetadata::new(
                    u32::try_from(ordinal).map_err(oracle_error)?,
                    native_type.clone(),
                    nullable == "Y",
                );
                metadata.type_family = OptionalMetadata::Supported(Some(native_type.clone()));
                metadata.default_expression = OptionalMetadata::Supported(default_expression);
                metadata.character_maximum_length = OptionalMetadata::Supported(
                    length.and_then(|value| u64::try_from(value).ok()),
                );
                metadata.numeric_precision = OptionalMetadata::Supported(
                    precision.and_then(|value| u32::try_from(value).ok()),
                );
                metadata.numeric_scale = OptionalMetadata::Supported(
                    scale.and_then(|value| u32::try_from(value).ok()),
                );
                entries.push(
                    CatalogEntry::relation_child(
                        CatalogId::new(
                            request.key.connection.profile_id,
                            CatalogKind::Column,
                            [database.clone(), schema.clone(), table.clone(), name.clone()],
                        ),
                        relation.clone(),
                        QualifiedName {
                            database: Some(database.clone()),
                            schema: Some(schema.clone()),
                            object: name,
                        },
                        "column",
                        OptionalMetadata::Unsupported,
                        CatalogMetadata::Column(metadata),
                    )
                    .map_err(|error| oracle_error(error.to_string()))?,
                );
            }
            return Ok(entries);
        }
    };
    let mut names = Vec::new();
    for row in rows {
        names.push(
            row.map_err(oracle_error)?
                .get::<_, String>(0)
                .map_err(oracle_error)?,
        );
    }
    if let Some(cursor) = request.key.cursor.as_ref() {
        let (sort_key, tie_breaker) = cursor
            .keyset_parts()
            .map_err(|error| oracle_error(error.to_string()))?;
        names.retain(|name| (name.as_str(), name.as_str()) > (sort_key, tie_breaker));
    }
    let mut entries = Vec::new();
    for name in names.into_iter().take(request.page_size) {
        let entry = match &request.key.target {
            CatalogTarget::Databases => CatalogEntry::database(
                CatalogId::new(
                    request.key.connection.profile_id,
                    super::catalog::CatalogKind::Database,
                    [configured_database.to_owned()],
                ),
                QualifiedName {
                    database: Some(configured_database.to_owned()),
                    schema: None,
                    object: configured_database.to_owned(),
                },
                "service",
                OptionalMetadata::Unsupported,
                true,
            ),
            CatalogTarget::Schemas { database } => CatalogEntry::schema(
                CatalogId::new(
                    request.key.connection.profile_id,
                    super::catalog::CatalogKind::Schema,
                    [configured_database.to_owned(), name.clone()],
                ),
                database.clone(),
                QualifiedName {
                    database: Some(configured_database.to_owned()),
                    schema: Some(name.clone()),
                    object: name,
                },
                "schema",
                OptionalMetadata::Unsupported,
                true,
            ),
            CatalogTarget::Objects { schema, group } => {
                let kind = match group {
                    ObjectGroup::Tables => super::catalog::CatalogKind::Table,
                    ObjectGroup::Views => super::catalog::CatalogKind::View,
                    ObjectGroup::Sequences => super::catalog::CatalogKind::Sequence,
                    _ => continue,
                };
                let qualified_name = QualifiedName {
                    database: Some(configured_database.to_owned()),
                    schema: schema.native_path.last().cloned(),
                    object: name,
                };
                if kind == CatalogKind::Sequence {
                    CatalogEntry::object(
                        CatalogId::new(
                            request.key.connection.profile_id,
                            kind,
                            [
                                configured_database.to_owned(),
                                schema.native_path.last().cloned().unwrap_or_default(),
                                qualified_name.object.clone(),
                            ],
                        ),
                        schema.clone(),
                        qualified_name,
                        "sequence",
                        OptionalMetadata::Unsupported,
                        false,
                    )
                } else {
                    CatalogEntry::relation(
                        CatalogId::new(
                            request.key.connection.profile_id,
                            kind,
                            [
                                configured_database.to_owned(),
                                schema.native_path.last().cloned().unwrap_or_default(),
                                qualified_name.object.clone(),
                            ],
                        ),
                        schema.clone(),
                        qualified_name,
                        "relation",
                        OptionalMetadata::Unsupported,
                        true,
                    )
                }
            }
            _ => continue,
        }
        .map_err(|error| oracle_error(error.to_string()))?;
        entries.push(entry);
    }
    Ok(entries)
}

#[cfg(not(feature = "driver-oracle"))]
fn oracle_disabled() -> DatabaseError {
    DatabaseError {
        category: ErrorCategory::Unsupported,
        code: Some("oracle_driver_not_enabled".into()),
        message: "Oracle support requires the driver-oracle feature".into(),
        diagnostic: None,
    }
}

#[cfg(feature = "driver-oracle")]
fn decode_value(
    row: &oracle::Row,
    index: usize,
    type_name: &str,
) -> Result<crate::db::value::CellValue, DatabaseError> {
    let upper = type_name.to_ascii_uppercase();
    if upper.contains("NUMBER") || upper.contains("DECIMAL") || upper.contains("NUMERIC") {
        let value = row.get::<_, Option<String>>(index).map_err(oracle_error)?;
        return Ok(match value {
            None => crate::db::value::CellValue::Null,
            Some(value) => value
                .parse::<i64>()
                .map(crate::db::value::CellValue::Integer)
                .unwrap_or(crate::db::value::CellValue::Text(value)),
        });
    }
    if upper.contains("TIMESTAMP") || upper == "DATE" {
        let value = row
            .get::<_, Option<chrono::NaiveDateTime>>(index)
            .map_err(oracle_error)?;
        return Ok(value.map_or(
            crate::db::value::CellValue::Null,
            crate::db::value::CellValue::DateTime,
        ));
    }
    if upper == "RAW" || upper.contains("BINARY") {
        let value = row.get::<_, Option<Vec<u8>>>(index).map_err(oracle_error)?;
        return Ok(value.map_or(
            crate::db::value::CellValue::Null,
            crate::db::value::CellValue::Bytes,
        ));
    }
    let value = row.get::<_, Option<String>>(index).map_err(oracle_error)?;
    Ok(value.map_or(
        crate::db::value::CellValue::Null,
        crate::db::value::CellValue::Text,
    ))
}

#[cfg(feature = "driver-oracle")]
fn oracle_error(error: impl std::fmt::Display) -> DatabaseError {
    let message = sanitize_terminal_text(&error.to_string());
    let lowered = message.to_ascii_lowercase();
    let category = if lowered.contains("ora-01017")
        || lowered.contains("ora-28000")
        || lowered.contains("authentication")
    {
        ErrorCategory::Authentication
    } else if lowered.contains("ora-00942")
        || lowered.contains("ora-01031")
        || lowered.contains("insufficient privileges")
    {
        ErrorCategory::Permission
    } else if lowered.contains("ora-00001")
        || lowered.contains("ora-02291")
        || lowered.contains("ora-02292")
    {
        ErrorCategory::Constraint
    } else if lowered.contains("dpi-")
        || lowered.contains("ora-121")
        || lowered.contains("ora-125")
        || lowered.contains("network")
    {
        ErrorCategory::Network
    } else {
        ErrorCategory::Sql
    };
    DatabaseError {
        category,
        code: Some("oracle_error".into()),
        message,
        diagnostic: None,
    }
}

#[cfg(feature = "driver-oracle")]
fn oracle_task_error(message: String) -> DatabaseError {
    DatabaseError {
        category: ErrorCategory::Internal,
        code: Some("oracle_task_failed".into()),
        message: sanitize_terminal_text(&message),
        diagnostic: None,
    }
}
