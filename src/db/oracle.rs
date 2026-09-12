#[cfg(feature = "driver-oracle")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "driver-oracle")]
use std::time::Instant;

#[cfg(feature = "driver-oracle")]
use secrecy::ExposeSecret;
use secrecy::SecretString;

#[cfg(feature = "driver-oracle")]
use super::DatabaseDiagnostic;
#[cfg(feature = "driver-oracle")]
use super::catalog::{
    CatalogCount, CatalogCursor, CatalogMetadata, ColumnMetadata, DiscoveredDatabase, ObjectGroup,
    finalize_keyset_page,
};
use super::catalog::{
    CatalogEntry, CatalogId, CatalogKind, CatalogPage, CatalogRequest, CatalogTarget,
    DdlProvenance, OptionalMetadata, QualifiedName, RelationDdl,
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
                if matches!(request.key.target, CatalogTarget::Groups { .. }) {
                    return oracle_group_page(&connection, &request);
                }
                if matches!(request.key.target, CatalogTarget::Objects { .. }) {
                    return oracle_object_page(&connection, &request, &database);
                }
                let mut entries = oracle_catalog_entries(&connection, &request, &database)?;
                let next_cursor = finalize_keyset_page(
                    &mut entries,
                    request.page_size,
                    |entry| entry.qualified_name.object.clone(),
                    |entry| entry.qualified_name.object.clone(),
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
            let sql = crate::sql::oracle::prepare_oracle_statement(sql)
                .map_err(|error| DatabaseError {
                    category: ErrorCategory::Sql,
                    code: Some("oracle_sql_prepare_error".into()),
                    message: error.to_string(),
                    diagnostic: None,
                })?
                .into_owned();
            let connection = Arc::clone(&self.connection);
            tokio::task::spawn_blocking(move || {
                let connection = connection
                    .lock()
                    .map_err(|_| oracle_error("Oracle connection lock poisoned"))?;
                let mut statement = connection
                    .statement(&sql)
                    .build()
                    .map_err(|error| oracle_error_with_query(error, &sql))?;
                if !statement.is_query() {
                    statement
                        .execute(&[])
                        .map_err(|error| oracle_error_with_query(error, &sql))?;
                    let affected_rows = statement
                        .row_count()
                        .map_err(|error| oracle_error_with_query(error, &sql))?;
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
                let rows = statement
                    .query(&[])
                    .map_err(|error| oracle_error_with_query(error, &sql))?;
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
                    let row = row.map_err(|error| oracle_error_with_query(error, &sql))?;
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
fn oracle_group_page(
    connection: &oracle::Connection,
    request: &CatalogRequest,
) -> Result<CatalogPage, DatabaseError> {
    let owner = match &request.key.target {
        CatalogTarget::Groups { schema } => {
            let [_, owner] = schema.native_path.as_slice() else {
                return Err(oracle_error("invalid Oracle schema identity"));
            };
            owner.clone()
        }
        _ => return Err(oracle_error("invalid Oracle groups request")),
    };
    let rows = connection
        .query(
            "SELECT 'tables' AS group_key, COUNT(*) AS object_count FROM all_tables WHERE owner = :1 \
             UNION ALL \
             SELECT 'views' AS group_key, COUNT(*) AS object_count FROM all_views WHERE owner = :2 \
             UNION ALL \
             SELECT 'sequences' AS group_key, COUNT(*) AS object_count FROM all_sequences WHERE sequence_owner = :3",
            &[&owner, &owner, &owner],
        )
        .map_err(oracle_error)?;
    let mut counts = [None; 3];
    for row in rows {
        let row = row.map_err(oracle_error)?;
        let key: String = row.get(0).map_err(oracle_error)?;
        let count: i64 = row.get(1).map_err(oracle_error)?;
        let count = u64::try_from(count).map_err(oracle_error)?;
        let index = oracle_object_groups()
            .iter()
            .position(|description| description.key == key)
            .ok_or_else(|| oracle_error(format!("unknown Oracle object group: {key}")))?;
        if counts[index].replace(CatalogCount::Exact(count)).is_some() {
            return Err(oracle_error(format!(
                "duplicate Oracle object group: {key}"
            )));
        }
    }
    let groups = oracle_object_groups()
        .into_iter()
        .enumerate()
        .map(|(index, description)| {
            Ok((
                format!("{:02}_{}", index + 1, description.key),
                super::catalog::CatalogGroupSummary {
                    group: description.group,
                    object_count: counts[index]
                        .ok_or_else(|| oracle_error("Oracle group count row is missing"))?,
                },
            ))
        })
        .collect::<Result<Vec<_>, DatabaseError>>()?;
    oracle_group_page_from_summaries(request, groups)
}

#[cfg(feature = "driver-oracle")]
fn oracle_group_page_from_summaries(
    request: &CatalogRequest,
    mut groups: Vec<(String, super::catalog::CatalogGroupSummary)>,
) -> Result<CatalogPage, DatabaseError> {
    if let Some(cursor) = request.key.cursor.as_ref() {
        let (sort_key, tie_breaker) = cursor
            .keyset_parts()
            .map_err(|error| oracle_error(error.to_string()))?;
        if sort_key != tie_breaker {
            return Err(oracle_error(
                "Oracle group cursor sort key and tie breaker differ",
            ));
        }
        groups.retain(|(key, _)| key.as_str() > sort_key);
    }
    let next_cursor = finalize_keyset_page(
        &mut groups,
        request.page_size,
        |(key, _)| key.clone(),
        |(key, _)| key.clone(),
    )
    .map_err(|error| oracle_error(error.to_string()))?;
    let groups = groups.into_iter().map(|(_, summary)| summary).collect();
    CatalogPage::groups(request, groups, CatalogCount::Exact(3), next_cursor)
        .map_err(|error| oracle_error(error.to_string()))
}

#[cfg(feature = "driver-oracle")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OracleObjectGroup {
    group: ObjectGroup,
    key: &'static str,
    dictionary: &'static str,
    owner_column: &'static str,
    name_column: &'static str,
}

#[cfg(feature = "driver-oracle")]
fn oracle_object_groups() -> [OracleObjectGroup; 3] {
    [
        oracle_object_group(ObjectGroup::Tables).expect("Tables has an Oracle mapping"),
        oracle_object_group(ObjectGroup::Views).expect("Views has an Oracle mapping"),
        oracle_object_group(ObjectGroup::Sequences).expect("Sequences has an Oracle mapping"),
    ]
}

#[cfg(feature = "driver-oracle")]
fn oracle_object_group(group: ObjectGroup) -> Option<OracleObjectGroup> {
    Some(match group {
        ObjectGroup::Tables => OracleObjectGroup {
            group,
            key: "tables",
            dictionary: "all_tables",
            owner_column: "owner",
            name_column: "table_name",
        },
        ObjectGroup::Views => OracleObjectGroup {
            group,
            key: "views",
            dictionary: "all_views",
            owner_column: "owner",
            name_column: "view_name",
        },
        ObjectGroup::Sequences => OracleObjectGroup {
            group,
            key: "sequences",
            dictionary: "all_sequences",
            owner_column: "sequence_owner",
            name_column: "sequence_name",
        },
        _ => return None,
    })
}

#[cfg(feature = "driver-oracle")]
fn oracle_object_cursor(request: &CatalogRequest) -> Result<Option<(&str, &str)>, DatabaseError> {
    oracle_object_cursor_parts(request.key.cursor.as_ref())
}

#[cfg(feature = "driver-oracle")]
fn oracle_object_cursor_parts(
    cursor: Option<&CatalogCursor>,
) -> Result<Option<(&str, &str)>, DatabaseError> {
    cursor
        .map(CatalogCursor::keyset_parts)
        .transpose()
        .map_err(|error| oracle_error(error.to_string()))?
        .map(|(sort_key, tie_breaker)| {
            if sort_key != tie_breaker {
                Err(oracle_error(
                    "Oracle object cursor sort key and tie breaker differ",
                ))
            } else {
                Ok((sort_key, tie_breaker))
            }
        })
        .transpose()
}

#[cfg(feature = "driver-oracle")]
fn oracle_object_page(
    connection: &oracle::Connection,
    request: &CatalogRequest,
    configured_database: &str,
) -> Result<CatalogPage, DatabaseError> {
    let CatalogTarget::Objects { schema, group } = &request.key.target else {
        return Err(oracle_error("invalid Oracle objects request"));
    };
    let Some(description) = oracle_object_group(*group) else {
        return Err(oracle_error("unsupported Oracle object group"));
    };
    let owner = schema
        .native_path
        .last()
        .ok_or_else(|| oracle_error("invalid Oracle schema identity"))?;
    let cursor = oracle_object_cursor(request)?;
    let limit = request.page_size.saturating_add(1);
    let cursor_predicate = cursor.map_or(String::new(), |_| {
        format!(
            " AND NLSSORT({name}, 'NLS_SORT=BINARY') > NLSSORT(:3, 'NLS_SORT=BINARY')",
            name = description.name_column
        )
    });
    let sql = format!(
        "WITH object_count AS (\
             SELECT COUNT(*) AS total_count FROM {dictionary} WHERE {owner_column} = :1\
         ), page_rows AS (\
             SELECT {name_column} AS object_name FROM {dictionary}\
             WHERE {owner_column} = :2{cursor_predicate}\
             ORDER BY NLSSORT({name_column}, 'NLS_SORT=BINARY')\
             FETCH FIRST {limit} ROWS ONLY\
         )\
         SELECT c.total_count, p.object_name FROM object_count c\
         LEFT JOIN page_rows p ON 1 = 1\
         ORDER BY NLSSORT(p.object_name, 'NLS_SORT=BINARY')",
        dictionary = description.dictionary,
        owner_column = description.owner_column,
        name_column = description.name_column,
        cursor_predicate = cursor_predicate,
    );
    let rows = match cursor {
        Some((_, cursor_name)) => connection.query(&sql, &[owner, owner, &cursor_name]),
        None => connection.query(&sql, &[owner, owner]),
    }
    .map_err(oracle_error)?;
    let mut total_count = None;
    let mut names = Vec::new();
    for row in rows {
        let row = row.map_err(oracle_error)?;
        let count: i64 = row.get(0).map_err(oracle_error)?;
        let count = u64::try_from(count).map_err(oracle_error)?;
        if total_count
            .replace(count)
            .is_some_and(|previous| previous != count)
        {
            return Err(oracle_error("Oracle object count changed within page"));
        }
        let name: Option<String> = row.get(1).map_err(oracle_error)?;
        if let Some(name) = name {
            names.push(name);
        }
    }
    let total_count =
        total_count.ok_or_else(|| oracle_error("Oracle object count row is missing"))?;
    let next_cursor = finalize_oracle_object_names(&mut names, request.page_size)?;
    let entries = names
        .into_iter()
        .map(|name| oracle_catalog_object_entry(request, configured_database, name))
        .collect::<Result<Vec<_>, _>>()?;
    CatalogPage::new(
        request,
        entries,
        CatalogCount::Exact(total_count),
        next_cursor,
    )
    .map_err(|error| oracle_error(error.to_string()))
}

#[cfg(feature = "driver-oracle")]
fn finalize_oracle_object_names(
    names: &mut Vec<String>,
    page_size: usize,
) -> Result<Option<CatalogCursor>, DatabaseError> {
    finalize_keyset_page(names, page_size, |name| name.clone(), |name| name.clone())
        .map_err(|error| oracle_error(error.to_string()))
}

#[cfg(feature = "driver-oracle")]
fn oracle_catalog_object_entry(
    request: &CatalogRequest,
    configured_database: &str,
    name: String,
) -> Result<CatalogEntry, DatabaseError> {
    let CatalogTarget::Objects { schema, group } = &request.key.target else {
        return Err(oracle_error("invalid Oracle objects request"));
    };
    let kind = match group {
        ObjectGroup::Tables => CatalogKind::Table,
        ObjectGroup::Views => CatalogKind::View,
        ObjectGroup::Sequences => CatalogKind::Sequence,
        _ => return Err(oracle_error("unsupported Oracle object group")),
    };
    let qualified_name = QualifiedName {
        database: Some(configured_database.to_owned()),
        schema: schema.native_path.last().cloned(),
        object: name,
    };
    let id = CatalogId::new(
        request.key.connection.profile_id,
        kind,
        [
            configured_database.to_owned(),
            schema.native_path.last().cloned().unwrap_or_default(),
            qualified_name.object.clone(),
        ],
    );
    if kind == CatalogKind::Sequence {
        CatalogEntry::object(
            id,
            schema.clone(),
            qualified_name,
            "sequence",
            OptionalMetadata::Unsupported,
            false,
        )
    } else {
        CatalogEntry::relation(
            id,
            schema.clone(),
            qualified_name,
            "relation",
            OptionalMetadata::Unsupported,
            true,
        )
    }
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
        CatalogTarget::Objects { .. } => {
            return Err(oracle_error("Oracle objects must use the object page loader"));
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
    if !matches!(request.key.target, CatalogTarget::Objects { .. })
        && let Some(cursor) = request.key.cursor.as_ref()
    {
        let (sort_key, tie_breaker) = cursor
            .keyset_parts()
            .map_err(|error| oracle_error(error.to_string()))?;
        names.retain(|name| (name.as_str(), name.as_str()) > (sort_key, tie_breaker));
    }
    let mut entries = Vec::new();
    for name in names {
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
            CatalogTarget::Objects { .. } => unreachable!("objects use the object page loader"),
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
    let code = oracle_error_code(&message);
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
        code,
        message,
        diagnostic: None,
    }
}

#[cfg(feature = "driver-oracle")]
fn oracle_error_with_query(error: impl std::fmt::Display, query: &str) -> DatabaseError {
    let mut error = oracle_error(error);
    error.diagnostic = Some(Box::new(DatabaseDiagnostic {
        context: Some(format!("Executed SQL: {}", sanitize_terminal_text(query))),
        ..DatabaseDiagnostic::default()
    }));
    error
}

#[cfg(feature = "driver-oracle")]
fn oracle_error_code(message: &str) -> Option<String> {
    let bytes = message.as_bytes();
    let marker = bytes
        .windows(4)
        .position(|window| window.eq_ignore_ascii_case(b"ORA-"))?;
    let digits = bytes
        .get(marker + 4..)?
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    (digits > 0).then(|| message[marker..marker + 4 + digits].to_ascii_uppercase())
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

#[cfg(all(test, feature = "driver-oracle"))]
mod tests {
    use super::*;

    #[test]
    fn oracle_object_groups_use_dictionary_specific_metadata() {
        let groups = oracle_object_groups();
        assert_eq!(groups[0].key, "tables");
        assert_eq!(groups[0].dictionary, "all_tables");
        assert_eq!(groups[0].owner_column, "owner");
        assert_eq!(groups[0].name_column, "table_name");
        assert_eq!(groups[1].name_column, "view_name");
        assert_eq!(groups[2].dictionary, "all_sequences");
        assert_eq!(groups[2].owner_column, "sequence_owner");
        assert_eq!(groups[2].name_column, "sequence_name");
    }

    #[test]
    fn oracle_object_cursor_requires_matching_name_parts() {
        let valid = CatalogCursor::from_keyset("A.B", "A.B").unwrap();
        assert_eq!(
            oracle_object_cursor_parts(Some(&valid)).unwrap(),
            Some(("A.B", "A.B"))
        );

        let invalid = CatalogCursor::from_keyset("A.B", "A.B2").unwrap();
        assert!(oracle_object_cursor_parts(Some(&invalid)).is_err());
        assert!(oracle_object_cursor_parts(Some(&CatalogCursor::new("bad"))).is_err());
    }

    #[test]
    fn oracle_group_pages_use_stable_keyset_pagination_and_exact_total() {
        let profile_id = uuid::Uuid::nil();
        let schema = CatalogId::new(profile_id, CatalogKind::Schema, ["service", "APP"]);
        let mut request = CatalogRequest {
            key: crate::db::catalog::CatalogRequestKey {
                connection: crate::identity::ConnectionIdentity {
                    profile_id,
                    generation: 1,
                },
                catalog_epoch: 1,
                request_id: 1,
                target: CatalogTarget::Groups {
                    schema: schema.clone(),
                },
                cursor: None,
            },
            scope: crate::profile::CatalogScope::for_profile(
                DatabaseKind::Oracle,
                "service",
                Some("APP"),
            ),
            page_size: 1,
        };
        let summaries: Vec<(String, crate::db::catalog::CatalogGroupSummary)> =
            oracle_object_groups()
                .into_iter()
                .enumerate()
                .map(|(index, description)| {
                    (
                        format!("{:02}_{}", index + 1, description.key),
                        crate::db::catalog::CatalogGroupSummary {
                            group: description.group,
                            object_count: CatalogCount::Exact((index as u64) * 2),
                        },
                    )
                })
                .collect();

        let first = oracle_group_page_from_summaries(&request, summaries.clone()).unwrap();
        assert_eq!(first.total_count, CatalogCount::Exact(3));
        assert_eq!(
            first.group_summaries[0].object_count,
            CatalogCount::Exact(0)
        );
        assert_eq!(
            first.next_cursor.as_ref().unwrap().keyset_parts().unwrap(),
            ("01_tables", "01_tables")
        );
        first.validate_for(&request).unwrap();

        request.key.cursor = first.next_cursor;
        let second = oracle_group_page_from_summaries(&request, summaries.clone()).unwrap();
        assert_eq!(second.group_summaries[0].group, ObjectGroup::Views);
        second.validate_for(&request).unwrap();

        request.key.cursor = second.next_cursor;
        let third = oracle_group_page_from_summaries(&request, summaries).unwrap();
        assert_eq!(third.group_summaries[0].group, ObjectGroup::Sequences);
        assert!(third.next_cursor.is_none());
        third.validate_for(&request).unwrap();
    }

    #[test]
    fn oracle_object_name_pages_preserve_page_size_plus_one_boundaries() {
        for (count, page_size, has_next) in [
            (0, 3, false),
            (2, 3, false),
            (3, 3, false),
            (4, 3, true),
            (7, 3, true),
        ] {
            let mut names = (0..count).map(|i| format!("OBJECT_{i}")).collect();
            let next = finalize_oracle_object_names(&mut names, page_size).unwrap();
            assert_eq!(names.len(), count.min(page_size));
            assert_eq!(next.is_some(), has_next);
            if has_next {
                let cursor = next.unwrap();
                let expected = format!("OBJECT_{}", page_size - 1);
                assert_eq!(
                    cursor.keyset_parts().unwrap(),
                    (expected.as_str(), expected.as_str())
                );
            }
        }
    }
}
