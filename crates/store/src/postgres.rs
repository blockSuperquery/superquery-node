//! Postgres connection pool and schema-level operations.
//!
//! Deliberately raw SQL over `tokio-postgres`, not an ORM: entity tables are
//! generated at runtime from each project's GraphQL schema, so there are no static
//! types for a query builder to check against. Rationale in the task plan §4.1.
//!
//! Upstream analogue: `node-core/src/db/db.module.ts`.

use deadpool_postgres::{Config as PoolConfig, Pool, Runtime};
use superquery_config::DbConfig;
use tokio_postgres::NoTls;

use crate::error::{Result, StoreError};

/// A pooled Postgres connection.
///
/// Cheap to clone — clones share the underlying pool.
#[derive(Clone)]
pub struct Database {
    pool: Pool,
    schema: String,
}

impl Database {
    /// Build a connection pool from [`DbConfig`].
    ///
    /// Lazy: no connection is opened until first use. Call [`Database::ping`] to
    /// prove reachability at startup.
    pub fn connect(cfg: &DbConfig) -> Result<Self> {
        validate_ident(&cfg.schema)?;

        let mut pool_cfg = PoolConfig::new();
        pool_cfg.host = Some(cfg.host.clone());
        pool_cfg.port = Some(cfg.port);
        pool_cfg.user = Some(cfg.username.clone());
        pool_cfg.password = Some(cfg.password.clone());
        pool_cfg.dbname = Some(cfg.database.clone());

        let pool = pool_cfg
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| StoreError::Pool(e.to_string()))?;

        Ok(Self {
            pool,
            schema: cfg.schema.clone(),
        })
    }

    /// The Postgres schema this project's tables live in.
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// Round-trip check: `SELECT 1`.
    pub async fn ping(&self) -> Result<()> {
        self.client().await?.query_one("SELECT 1", &[]).await?;
        Ok(())
    }

    /// Borrow a pooled client.
    ///
    /// Prefer [`Database::transaction`] for anything that must be atomic.
    pub async fn client(&self) -> Result<deadpool_postgres::Object> {
        self.pool
            .get()
            .await
            .map_err(|e| StoreError::Pool(e.to_string()))
    }

    /// Run `f` inside a single transaction, committing on `Ok` and rolling back
    /// on `Err`.
    ///
    /// This is the boundary guide Milestone 10 requires: entity mutations, dynamic
    /// datasource changes and the checkpoint update for one block either all land
    /// or none do.
    ///
    /// ```ignore
    /// db.transaction(|tx| Box::pin(async move {
    ///     model.upsert_tx(tx, &entities).await?;
    ///     checkpoint.commit_tx(tx, &ptr).await
    /// })).await?;
    /// ```
    pub async fn transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: for<'a> FnOnce(
            &'a tokio_postgres::Transaction<'a>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>,
        >,
    {
        let mut client = self.client().await?;
        let tx = client.transaction().await?;
        match f(&tx).await {
            Ok(value) => {
                tx.commit().await?;
                Ok(value)
            }
            Err(e) => {
                // An explicit rollback; dropping would do it, but failing to roll
                // back is itself worth surfacing.
                if let Err(rollback_err) = tx.rollback().await {
                    tracing::error!(error = %rollback_err, "rollback failed after transaction error");
                }
                Err(e)
            }
        }
    }

    /// `CREATE SCHEMA IF NOT EXISTS` for this project's schema.
    pub async fn ensure_schema(&self) -> Result<()> {
        let schema = &self.schema;
        self.client()
            .await?
            .batch_execute(&format!("CREATE SCHEMA IF NOT EXISTS \"{schema}\""))
            .await?;
        Ok(())
    }

    /// `DROP SCHEMA … CASCADE`. Used by ephemeral test schemas for teardown.
    ///
    /// Destructive; never called on a normal indexing path.
    pub async fn drop_schema(&self, schema: &str) -> Result<()> {
        validate_ident(schema)?;
        self.client()
            .await?
            .batch_execute(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
            .await?;
        Ok(())
    }

    /// Run raw SQL. For DDL and test setup, not for user input.
    pub async fn batch_execute(&self, sql: &str) -> Result<()> {
        self.client().await?.batch_execute(sql).await?;
        Ok(())
    }

    /// Execute a parameterized statement, returning the affected row count.
    pub async fn execute(
        &self,
        sql: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<u64> {
        Ok(self.client().await?.execute(sql, params).await?)
    }

    /// Run a parameterized query, returning the rows.
    pub async fn query(
        &self,
        sql: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<Vec<tokio_postgres::Row>> {
        Ok(self.client().await?.query(sql, params).await?)
    }

    /// Introspect `schema` into a canonical, comparison-ready
    /// [`SchemaInfo`](crate::introspect::SchemaInfo).
    pub async fn introspect_schema(&self, schema: &str) -> Result<crate::introspect::SchemaInfo> {
        let client = self.client().await?;
        crate::introspect::introspect(&client, schema).await
    }
}

/// Postgres identifiers are interpolated into DDL — they cannot be bound
/// parameters — so restrict them to a safe character set.
///
/// Every code path that puts a name into SQL text goes through here.
pub(crate) fn validate_ident(ident: &str) -> Result<()> {
    let ok = !ident.is_empty()
        && ident.len() <= 63
        && ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        // A leading digit would need quoting to be a legal identifier.
        && !ident.starts_with(|c: char| c.is_ascii_digit());
    if ok {
        Ok(())
    } else {
        Err(StoreError::InvalidIdentifier(ident.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_identifiers() {
        assert!(validate_ident("app").is_ok());
        assert!(validate_ident("app_1").is_ok());
        assert!(validate_ident("_metadata").is_ok());
        assert!(validate_ident("transfers").is_ok());
    }

    #[test]
    fn rejects_injection_attempts() {
        assert!(validate_ident("bad-name").is_err());
        assert!(validate_ident("drop\";--").is_err());
        assert!(validate_ident("a b").is_err());
        assert!(validate_ident("tbl\"; DROP SCHEMA public; --").is_err());
    }

    #[test]
    fn rejects_empty_overlong_and_leading_digit() {
        assert!(validate_ident("").is_err());
        assert!(validate_ident(&"x".repeat(64)).is_err());
        assert!(validate_ident("1st_table").is_err());
    }
}
