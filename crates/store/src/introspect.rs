//! Schema introspection — the foundation of the GATE 2 parity differ.
//!
//! The TS node creates tables via Sequelize's `sync()`, so there is no canonical
//! `CREATE TABLE` string to match. Instead we compare the *resulting* Postgres
//! schema: introspect both the TS-produced and Rust-produced schemas into this
//! canonical, order-normalized [`SchemaInfo`] and assert they are equal. Row-level
//! data diffing is layered on top (also GATE 2).

use std::collections::BTreeMap;

use tokio_postgres::Client;

use crate::error::StoreError;

/// A canonical, comparison-ready description of a Postgres schema.
///
/// Everything is sorted (BTreeMap / pre-sorted Vecs) so two schemas built by
/// different engines compare equal iff they are structurally identical,
/// independent of creation order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaInfo {
    /// Tables in the schema, keyed by name.
    pub tables: BTreeMap<String, TableInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One table's columns and indexes, order-normalized.
pub struct TableInfo {
    /// Columns sorted by name.
    pub columns: Vec<ColumnInfo>,
    /// Index definitions sorted by name.
    pub indexes: Vec<IndexInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One column's canonical description.
pub struct ColumnInfo {
    /// Column name.
    pub name: String,
    /// Canonical Postgres type, e.g. `text`, `integer`, `numeric`, `int8range`.
    pub data_type: String,
    /// Whether the column accepts NULL.
    pub is_nullable: bool,
    /// Column default expression, if any.
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One index's canonical description.
pub struct IndexInfo {
    /// Index name.
    pub name: String,
    /// Whether the index enforces uniqueness.
    pub is_unique: bool,
    /// The index definition (`pg_indexes.indexdef`), with the schema-qualified
    /// prefix stripped so it is comparable across differently-named schemas.
    pub definition: String,
}

/// Introspect a single schema into a [`SchemaInfo`].
pub async fn introspect(client: &Client, schema: &str) -> Result<SchemaInfo, StoreError> {
    let column_rows = client
        .query(
            "SELECT table_name, column_name, \
                    COALESCE(domain_name, udt_name) AS data_type, \
                    is_nullable, column_default \
             FROM information_schema.columns \
             WHERE table_schema = $1 \
             ORDER BY table_name, column_name",
            &[&schema],
        )
        .await?;

    let mut tables: BTreeMap<String, TableInfo> = BTreeMap::new();
    for row in column_rows {
        let table: String = row.get("table_name");
        let name: String = row.get("column_name");
        let data_type: String = row.get("data_type");
        let is_nullable: String = row.get("is_nullable");
        let default: Option<String> = row.get("column_default");
        tables
            .entry(table)
            .or_insert_with(|| TableInfo {
                columns: Vec::new(),
                indexes: Vec::new(),
            })
            .columns
            .push(ColumnInfo {
                name,
                data_type,
                is_nullable: is_nullable == "YES",
                default,
            });
    }

    let index_rows = client
        .query(
            "SELECT tablename, indexname, indexdef \
             FROM pg_indexes WHERE schemaname = $1 \
             ORDER BY tablename, indexname",
            &[&schema],
        )
        .await?;

    for row in index_rows {
        let table: String = row.get("tablename");
        let name: String = row.get("indexname");
        let raw_def: String = row.get("indexdef");
        // Strip the schema-qualified prefix so definitions compare across schemas.
        let definition = raw_def
            .replace(&format!("\"{schema}\"."), "")
            .replace(&format!("{schema}."), "");
        let is_unique = raw_def.starts_with("CREATE UNIQUE INDEX");
        if let Some(t) = tables.get_mut(&table) {
            t.indexes.push(IndexInfo {
                name,
                is_unique,
                definition,
            });
        }
    }

    Ok(SchemaInfo { tables })
}
