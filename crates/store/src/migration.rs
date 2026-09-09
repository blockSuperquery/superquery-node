//! Schema setup and migration.
//!
//! Two different problems share this module:
//!
//! 1. **Project schema creation** — turning the project's GraphQL entities into
//!    tables. Driven by [`crate::ddl`], and re-run on every start (the DDL is
//!    `IF NOT EXISTS`-shaped, so it is idempotent).
//! 2. **Internal schema versioning** — our own bookkeeping tables changing shape
//!    between releases. Versioned `.sql` files under `migrations/`, applied in
//!    order and recorded, so an upgrade cannot half-apply.
//!
//! Plain SQL files rather than a migration framework: the set is small, the
//! ordering is obvious, and it stays readable to anyone auditing what the node
//! does to their database.

use crate::ddl;
use crate::error::Result;
use crate::metadata::{keys, MetadataStore, SCHEMA_VERSION};
use crate::postgres::Database;
use crate::schema::EntityModel;

/// Create the project's schema, internal tables and entity tables.
///
/// Idempotent: safe to call on every start.
pub async fn initialize_schema(
    db: &Database,
    models: &[EntityModel],
    project_id: &str,
    chain_id: &str,
) -> Result<()> {
    db.ensure_schema().await?;

    let checkpoints = crate::checkpoint::CheckpointStore::new(db.schema())?;
    checkpoints.ensure_tables(db).await?;

    let metadata = MetadataStore::new(db.schema())?;
    let outcome = metadata
        .verify_or_initialize(db, project_id, chain_id)
        .await?;
    tracing::info!(?outcome, schema = db.schema(), "project schema ready");

    for statement in ddl::create_tables(models, db.schema())? {
        // `create_table` emits plain CREATE TABLE; make re-runs idempotent here
        // rather than in the DDL, which is also used to render schemas for tests.
        let idempotent = statement.replacen("CREATE TABLE ", "CREATE TABLE IF NOT EXISTS ", 1);
        db.batch_execute(&idempotent).await?;
    }

    metadata
        .set(db, keys::SCHEMA_VERSION, SCHEMA_VERSION)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::ddl;
    use crate::schema::parse_entities;

    #[test]
    fn generated_ddl_can_be_made_idempotent() {
        let models = parse_entities("type Transfer @entity { id: ID! amount: BigInt! }").unwrap();
        let stmt = &ddl::create_tables(&models, "app").unwrap()[0];
        let idempotent = stmt.replacen("CREATE TABLE ", "CREATE TABLE IF NOT EXISTS ", 1);

        assert!(idempotent.starts_with("CREATE TABLE IF NOT EXISTS \"app\".\"transfers\""));
        // Exactly one substitution — the table body must be untouched.
        assert_eq!(idempotent.matches("IF NOT EXISTS").count(), 1);
    }
}
