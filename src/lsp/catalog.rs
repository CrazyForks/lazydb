use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, OnceCell};

use crate::sql::CompletionIndex;
use crate::{
    db::{
        DatabaseConnection,
        catalog::{
            CatalogEntry, CatalogKind, CatalogRequest, CatalogRequestKey, CatalogTarget,
            ObjectGroup,
        },
    },
    identity::ConnectionIdentity,
    persistence::{
        credentials::CredentialResolver, local_credentials::LocalCredentialStore, paths::AppPaths,
        profiles::ProfileStore, secrets::NativeSecretStore,
    },
    profile::ConnectionProfile,
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CatalogKey {
    pub connection: String,
    pub database: Option<String>,
    pub schema: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CatalogSnapshot {
    pub generation: u64,
    pub index: Arc<CompletionIndex>,
    pub complete: bool,
}

#[derive(Debug, Default)]
pub struct CatalogCache {
    entries: Mutex<HashMap<CatalogKey, Arc<OnceCell<CatalogSnapshot>>>>,
}

#[derive(Clone, Debug)]
pub struct CatalogProvider {
    connection: std::sync::Arc<DatabaseConnection>,
    profile: ConnectionProfile,
}

impl CatalogProvider {
    pub fn profile_id(&self) -> uuid::Uuid {
        self.profile.id
    }
}

impl CatalogProvider {
    pub async fn from_config(
        project: Option<&std::path::Path>,
        config: Option<std::path::PathBuf>,
        selector: Option<&str>,
    ) -> anyhow::Result<Option<Self>> {
        let project = crate::agent::context::AgentProjectContext::resolve(project)?;
        let paths = AppPaths::discover()?;
        let profiles = ProfileStore::new(config.unwrap_or_else(|| paths.profiles_file())).load()?;
        let visible = project.visible_profiles(&profiles.profiles);
        let selected = crate::agent::selection::select_profile(&visible, selector)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let resolver = CredentialResolver::new(
            std::sync::Arc::new(NativeSecretStore),
            LocalCredentialStore::from_paths(&paths, "lazydb"),
        );
        let password = resolver.resolve_headless(selected.profile).await?;
        let connection = DatabaseConnection::connect(selected.profile, password.as_ref()).await?;
        Ok(Some(Self {
            connection: std::sync::Arc::new(connection),
            profile: selected.profile.clone(),
        }))
    }

    pub async fn load_index(&self) -> anyhow::Result<CompletionIndex> {
        let connection = ConnectionIdentity {
            profile_id: self.profile.id,
            generation: 0,
        };
        let database_name = self.profile.database.as_deref().unwrap_or_default();
        let schema_name = self.profile.default_schema.as_deref().unwrap_or("public");
        let database = CatalogEntry::database(
            crate::db::catalog::CatalogId::new(
                self.profile.id,
                CatalogKind::Database,
                [database_name],
            ),
            crate::db::catalog::QualifiedName {
                database: None,
                schema: None,
                object: database_name.into(),
            },
            "database",
            crate::db::catalog::OptionalMetadata::Supported(None),
            true,
        )?;
        let schema = CatalogEntry::schema(
            crate::db::catalog::CatalogId::new(
                self.profile.id,
                CatalogKind::Schema,
                [database_name, schema_name],
            ),
            database.id.clone(),
            crate::db::catalog::QualifiedName {
                database: Some(database_name.into()),
                schema: None,
                object: schema_name.into(),
            },
            "schema",
            crate::db::catalog::OptionalMetadata::Supported(None),
            true,
        )?;
        let mut entries = Vec::new();
        for group in [
            ObjectGroup::Tables,
            ObjectGroup::Views,
            ObjectGroup::MaterializedViews,
        ] {
            let target = CatalogTarget::objects(schema.id.clone(), group)?;
            let relations = self
                .load_target(target, connection, self.profile.catalog_scope.clone())
                .await?;
            for relation in relations {
                let children = self
                    .load_target(
                        CatalogTarget::relation_children(relation.id.clone())?,
                        connection,
                        self.profile.catalog_scope.clone(),
                    )
                    .await?;
                entries.push(relation);
                entries.extend(children);
            }
        }
        Ok(CompletionIndex::new(&entries))
    }

    async fn load_target(
        &self,
        target: CatalogTarget,
        connection: ConnectionIdentity,
        scope: crate::profile::CatalogScope,
    ) -> anyhow::Result<Vec<CatalogEntry>> {
        let mut cursor = None;
        let mut request_id = 1;
        let mut entries = Vec::new();
        loop {
            let request = CatalogRequest {
                key: CatalogRequestKey {
                    connection,
                    catalog_epoch: 0,
                    request_id,
                    target: target.clone(),
                    cursor: cursor.clone(),
                },
                scope: scope.clone(),
                page_size: 500,
            };
            let page = self.connection.load_catalog_page(&request).await?;
            entries.extend(page.entries);
            let Some(next) = page.next_cursor else {
                break;
            };
            cursor = Some(next);
            request_id += 1;
        }
        Ok(entries)
    }
}

impl CatalogCache {
    pub async fn snapshot<F, Fut>(&self, key: CatalogKey, loader: F) -> Arc<CatalogSnapshot>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = CatalogSnapshot>,
    {
        let cell = {
            let mut entries = self.entries.lock().await;
            entries
                .entry(key)
                .or_insert_with(|| Arc::new(OnceCell::new()))
                .clone()
        };
        cell.get_or_init(loader).await.clone().into()
    }

    pub async fn invalidate(&self, key: &CatalogKey) {
        self.entries.lock().await.remove(key);
    }
}

use std::future::Future;

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[tokio::test]
    async fn same_key_loads_once_and_invalidating_reloads() {
        let cache = Arc::new(CatalogCache::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let key = CatalogKey {
            connection: "test".into(),
            database: None,
            schema: None,
        };
        let first = cache
            .snapshot(key.clone(), {
                let calls = calls.clone();
                move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    CatalogSnapshot {
                        generation: 1,
                        index: Arc::new(CompletionIndex::new(&[])),
                        complete: false,
                    }
                }
            })
            .await;
        let second = cache
            .snapshot(key.clone(), || async {
                CatalogSnapshot {
                    generation: 2,
                    index: Arc::new(CompletionIndex::new(&[])),
                    complete: true,
                }
            })
            .await;
        assert_eq!(first.generation, second.generation);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        cache.invalidate(&key).await;
        let third = cache
            .snapshot(key, || async {
                CatalogSnapshot {
                    generation: 3,
                    index: Arc::new(CompletionIndex::new(&[])),
                    complete: true,
                }
            })
            .await;
        assert_eq!(third.generation, 3);
    }
}
