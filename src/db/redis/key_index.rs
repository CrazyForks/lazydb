use std::path::{Path, PathBuf};

use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

use super::{
    key_store::{KeyPage, TreePageEntry},
    types::{RedisKeyId, RedisTarget},
};

#[derive(Debug, thiserror::Error)]
pub enum KeyIndexError {
    #[error("key index I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("key index database failed: {0}")]
    Database(#[from] sqlx::Error),
}

pub struct SqliteKeyStore {
    target: RedisTarget,
    path: PathBuf,
    pool: SqlitePool,
    count: usize,
    bytes: usize,
}

impl std::fmt::Debug for SqliteKeyStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SqliteKeyStore")
            .field("path", &self.path)
            .finish()
    }
}

impl SqliteKeyStore {
    pub async fn new_temp(target: RedisTarget) -> Result<Self, KeyIndexError> {
        let directory = std::env::temp_dir().join("lazydb");
        tokio::fs::create_dir_all(&directory).await?;
        let path = directory.join(format!("redis-keys-{}.sqlite3", uuid::Uuid::new_v4()));
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await?;
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA synchronous = NORMAL")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE TABLE keys (key BLOB PRIMARY KEY) WITHOUT ROWID")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE INDEX keys_lexical ON keys(key)")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE TABLE tree_entries (parent BLOB NOT NULL, name BLOB NOT NULL, path BLOB NOT NULL, is_leaf INTEGER NOT NULL, total_keys INTEGER NOT NULL, PRIMARY KEY(parent, is_leaf, name)) WITHOUT ROWID")
            .execute(&pool)
            .await?;
        Ok(Self {
            target,
            path,
            pool,
            count: 0,
            bytes: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn insert_batch(&mut self, keys: &[Vec<u8>]) -> Result<usize, KeyIndexError> {
        let mut transaction = self.pool.begin().await?;
        let mut inserted = 0;
        for key in keys {
            let result = sqlx::query("INSERT OR IGNORE INTO keys (key) VALUES (?)")
                .bind(key)
                .execute(&mut *transaction)
                .await?;
            if result.rows_affected() > 0 {
                inserted += 1;
                self.count += 1;
                self.bytes += key.len();
                insert_tree_entries(&mut transaction, key).await?;
            }
        }
        transaction.commit().await?;
        Ok(inserted)
    }

    pub async fn page_after_async(
        &self,
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<KeyPage, KeyIndexError> {
        let rows = if let Some(after) = after {
            sqlx::query("SELECT key FROM keys WHERE key > ? ORDER BY key LIMIT ?")
                .bind(after)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query("SELECT key FROM keys ORDER BY key LIMIT ?")
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
        };
        let keys = rows
            .into_iter()
            .map(|row| row.get::<Vec<u8>, _>("key"))
            .collect::<Vec<_>>();
        let next_after = keys.last().cloned();
        Ok(KeyPage {
            complete: keys.len() < limit,
            keys: keys
                .into_iter()
                .map(|key| RedisKeyId {
                    target: self.target.clone(),
                    key,
                })
                .collect(),
            next_after,
        })
    }

    pub async fn tree_page_async(
        &self,
        parent: &[u8],
        after: Option<&[u8]>,
        limit: usize,
    ) -> Result<Vec<TreePageEntry>, KeyIndexError> {
        let rows = if let Some(after) = after {
            sqlx::query("SELECT parent, name, path, is_leaf, total_keys FROM tree_entries WHERE parent = ? AND name > ? ORDER BY is_leaf, name LIMIT ?").bind(parent).bind(after).bind(limit as i64).fetch_all(&self.pool).await?
        } else {
            sqlx::query("SELECT parent, name, path, is_leaf, total_keys FROM tree_entries WHERE parent = ? ORDER BY is_leaf, name LIMIT ?").bind(parent).bind(limit as i64).fetch_all(&self.pool).await?
        };
        Ok(rows
            .into_iter()
            .map(|row| TreePageEntry {
                parent: row.get("parent"),
                name: row.get("name"),
                path: row.get("path"),
                is_leaf: row.get::<i64, _>("is_leaf") != 0,
                total_keys: row.get::<i64, _>("total_keys") as usize,
            })
            .collect())
    }

    pub async fn close(self) -> Result<(), KeyIndexError> {
        self.pool.close().await;
        tokio::fs::remove_file(&self.path).await.ok();
        tokio::fs::remove_file(format!("{}-wal", self.path.display()))
            .await
            .ok();
        tokio::fs::remove_file(format!("{}-shm", self.path.display()))
            .await
            .ok();
        Ok(())
    }
}

impl SqliteKeyStore {
    pub fn count(&self) -> usize {
        self.count
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

async fn insert_tree_entries(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    key: &[u8],
) -> Result<(), KeyIndexError> {
    let mut parent = Vec::new();
    let mut start = 0;
    for (offset, byte) in key.iter().enumerate() {
        if *byte == b':' {
            let name = &key[start..offset];
            let path = &key[..=offset];
            sqlx::query("INSERT INTO tree_entries(parent, name, path, is_leaf, total_keys) VALUES (?, ?, ?, 0, 1) ON CONFLICT(parent, is_leaf, name) DO UPDATE SET total_keys = total_keys + 1")
                .bind(&parent).bind(name).bind(path).execute(&mut **transaction).await?;
            parent = path.to_vec();
            start = offset + 1;
        }
    }
    sqlx::query("INSERT INTO tree_entries(parent, name, path, is_leaf, total_keys) VALUES (?, ?, ?, 1, 1) ON CONFLICT(parent, is_leaf, name) DO UPDATE SET total_keys = total_keys + 1")
        .bind(&parent).bind(&key[start..]).bind(key).execute(&mut **transaction).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> RedisTarget {
        RedisTarget {
            profile_id: uuid::Uuid::nil(),
            database: 0,
        }
    }

    #[tokio::test]
    async fn inserts_binary_keys_and_paginates_without_offset() {
        let mut store = SqliteKeyStore::new_temp(target()).await.unwrap();
        let path = store.path().to_path_buf();
        assert_eq!(
            store
                .insert_batch(&[b"b".to_vec(), vec![0, 1], b"b".to_vec(), b"a:1".to_vec()])
                .await
                .unwrap(),
            3
        );
        assert_eq!(store.count(), 3);
        assert_eq!(store.bytes(), 6);
        let first = store.page_after_async(None, 2).await.unwrap();
        assert_eq!(
            first
                .keys
                .iter()
                .map(|key| key.key.clone())
                .collect::<Vec<_>>(),
            vec![vec![0, 1], b"a:1".to_vec()]
        );
        let second = store
            .page_after_async(first.next_after.as_deref(), 2)
            .await
            .unwrap();
        assert_eq!(second.keys[0].key, b"b");
        assert!(second.complete);
        let tree = store.tree_page_async(b"", None, 10).await.unwrap();
        assert!(tree.iter().any(|entry| entry.name == vec![0, 1]));
        store.close().await.unwrap();
        assert!(!path.exists());
    }
}
