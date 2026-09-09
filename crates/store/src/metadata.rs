//! `_superquery_metadata` — the node's own bookkeeping table.
//!
//! One key/value row per fact, per guide §3.4:
//!
//! ```text
//! _superquery_metadata
//! ├── project_id          ├── chain_id
//! ├── schema_version      ├── indexed_height
//! ├── manifest_hash       ├── finalized_height
//! ├── indexed_block_hash  └── updated_at
//! ```
//!
//! Key/value rather than a one-row wide table so adding a fact needs no
//! migration, and so a partially-written row cannot exist.
//!
//! This is **not** a compatibility surface with SubQuery's `_metadata`; it is our
//! own contract, and the divergent name is deliberate — pointing this node at a
//! SubQuery-indexed database should be an obvious mismatch, not a silent merge.

use crate::error::{Result, StoreError};
use crate::postgres::{validate_ident, Database};

/// The metadata table's name.
pub const METADATA_TABLE: &str = "_superquery_metadata";

/// Schema layout version. Bump when the shape of generated tables changes in a
/// way that existing data cannot be read under.
pub const SCHEMA_VERSION: &str = "1";

/// Well-known metadata keys.
pub mod keys {
    /// Project identifier from the manifest.
    pub const PROJECT_ID: &str = "project_id";
    /// Layout version of the generated schema.
    pub const SCHEMA_VERSION: &str = "schema_version";
    /// Hash of the manifest that produced this schema.
    pub const MANIFEST_HASH: &str = "manifest_hash";
    /// Network identifier being indexed.
    pub const CHAIN_ID: &str = "chain_id";
    /// Highest fully-committed block height.
    pub const INDEXED_HEIGHT: &str = "indexed_height";
    /// Hash of the block at `indexed_height`.
    pub const INDEXED_BLOCK_HASH: &str = "indexed_block_hash";
    /// Highest height known to be final.
    pub const FINALIZED_HEIGHT: &str = "finalized_height";
    /// Timestamp of the last metadata write.
    pub const UPDATED_AT: &str = "updated_at";
    /// Mapping ABI version the project was built against.
    pub const MAPPING_ABI_VERSION: &str = "mapping_abi_version";
}

/// Typed access to `_superquery_metadata` within one schema.
pub struct MetadataStore {
    schema: String,
}

impl MetadataStore {
    /// Bind to a schema.
    pub fn new(schema: impl Into<String>) -> Result<Self> {
        let schema = schema.into();
        validate_ident(&schema)?;
        Ok(Self { schema })
    }

    /// The DDL creating the metadata table.
    pub fn create_table_sql(&self) -> String {
        format!(
            "CREATE TABLE IF NOT EXISTS \"{}\".\"{METADATA_TABLE}\" (\n  \
               key text PRIMARY KEY,\n  \
               value text NOT NULL,\n  \
               updated_at timestamptz NOT NULL DEFAULT now()\n\
             );",
            self.schema
        )
    }

    /// Create the table if it is absent.
    pub async fn ensure_table(&self, db: &Database) -> Result<()> {
        db.batch_execute(&self.create_table_sql()).await
    }

    /// Read one key.
    pub async fn get(&self, db: &Database, key: &str) -> Result<Option<String>> {
        let sql = format!(
            "SELECT value FROM \"{}\".\"{METADATA_TABLE}\" WHERE key = $1",
            self.schema
        );
        let rows = db.query(&sql, &[&key]).await?;
        Ok(rows.first().map(|r| r.get::<_, String>(0)))
    }

    /// Read one key as a `u64`.
    pub async fn get_u64(&self, db: &Database, key: &str) -> Result<Option<u64>> {
        match self.get(db, key).await? {
            None => Ok(None),
            Some(raw) => raw
                .parse::<u64>()
                .map(Some)
                .map_err(|e| StoreError::Decode(format!("metadata '{key}' = {raw:?}: {e}"))),
        }
    }

    /// Write one key, refreshing `updated_at`.
    pub async fn set(&self, db: &Database, key: &str, value: &str) -> Result<()> {
        db.execute(&self.upsert_sql(), &[&key, &value]).await?;
        Ok(())
    }

    /// Write one key inside an existing transaction.
    ///
    /// This is how the checkpoint update joins a block's entity writes in the same
    /// commit (guide Milestone 10).
    pub async fn set_tx(
        &self,
        tx: &tokio_postgres::Transaction<'_>,
        key: &str,
        value: &str,
    ) -> Result<()> {
        tx.execute(&self.upsert_sql(), &[&key, &value]).await?;
        Ok(())
    }

    fn upsert_sql(&self) -> String {
        format!(
            "INSERT INTO \"{}\".\"{METADATA_TABLE}\" (key, value, updated_at) \
             VALUES ($1, $2, now()) \
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
            self.schema
        )
    }

    /// Assert the database's identity matches this run's, and stamp it on a fresh
    /// database.
    ///
    /// Guards the failure that is worst to debug: pointing a node at a schema
    /// another project or chain already wrote. On a mismatch nothing is written.
    pub async fn verify_or_initialize(
        &self,
        db: &Database,
        project_id: &str,
        chain_id: &str,
    ) -> Result<InitOutcome> {
        let checks = [
            (keys::PROJECT_ID, project_id),
            (keys::CHAIN_ID, chain_id),
            (keys::SCHEMA_VERSION, SCHEMA_VERSION),
        ];

        let mut existing = Vec::new();
        for (key, expected) in checks {
            match self.get(db, key).await? {
                Some(found) if found != expected => {
                    return Err(StoreError::MetadataConflict(format!(
                        "schema '{}' holds {key}={found:?} but this run expects {expected:?}",
                        self.schema
                    )));
                }
                Some(_) => existing.push(key),
                None => {}
            }
        }

        if existing.len() == checks.len() {
            return Ok(InitOutcome::Resumed);
        }

        for (key, value) in checks {
            self.set(db, key, value).await?;
        }
        Ok(InitOutcome::Initialized)
    }
}

/// Whether a run started fresh or picked up existing state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitOutcome {
    /// The schema was empty; identity has now been stamped.
    Initialized,
    /// The schema already held matching identity; indexing resumes.
    Resumed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddl_targets_the_right_schema_and_table() {
        let m = MetadataStore::new("erc20").unwrap();
        let sql = m.create_table_sql();
        assert!(sql.contains("\"erc20\".\"_superquery_metadata\""), "{sql}");
        assert!(sql.contains("key text PRIMARY KEY"));
        assert!(sql.contains("IF NOT EXISTS"));
    }

    #[test]
    fn upsert_refreshes_updated_at() {
        let m = MetadataStore::new("app").unwrap();
        let sql = m.upsert_sql();
        assert!(sql.contains("ON CONFLICT (key) DO UPDATE"));
        assert!(sql.contains("updated_at = now()"));
        // Values must be bound, never interpolated.
        assert!(sql.contains("VALUES ($1, $2, now())"));
    }

    #[test]
    fn schema_name_is_validated() {
        assert!(MetadataStore::new("ok_schema").is_ok());
        assert!(matches!(
            MetadataStore::new("bad\"; DROP SCHEMA public; --"),
            Err(StoreError::InvalidIdentifier(_))
        ));
    }
}
