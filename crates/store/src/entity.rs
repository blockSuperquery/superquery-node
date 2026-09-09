//! Entity persistence: reading and writing the tables generated from a project's
//! GraphQL schema.
//!
//! [`PlainModel`] writes current state directly, one table per entity. Semantics:
//!
//! - `upsert` → `INSERT … ON CONFLICT (id) DO UPDATE`, so a handler that sets the
//!   same entity twice in a block converges rather than conflicting;
//! - `remove` → `DELETE WHERE id = ANY($1)`;
//! - reads are parameterized `SELECT`s.
//!
//! Values bind as text and are cast in SQL (`$n::text::numeric`,
//! `decode($n,'hex')` for bytea). Entity values arrive as JSON from the mapping
//! runtime, so text is the one encoding that covers every scalar without a
//! per-type binding path — and it keeps `BigInt` exact, which an `i64` bind would
//! not.
//!
//! Rows read back are canonicalized through `col::text` so two engines writing the
//! same logical data produce byte-identical dumps. That is what makes the parity
//! tests meaningful.
//!
//! Historical (versioned) writes for rewind arrive with guide Milestone 11; see
//! [`crate::rewind`].

use std::collections::BTreeMap;

use serde_json::Value;
use tokio_postgres::types::ToSql;

use crate::error::{Result, StoreError};
use crate::naming::{model_to_table_name, underscored};
use crate::postgres::Database;
use crate::schema::{EntityField, EntityModel};

/// A canonical row: column name → text rendering (`None` = SQL NULL).
pub type CanonicalRow = BTreeMap<String, Option<String>>;

/// Sort direction for queries.
#[derive(Debug, Clone, Copy)]
pub enum OrderDir {
    /// Ascending.
    Asc,
    /// Descending.
    Desc,
}

/// Query shaping options (port of the relevant `GetOptions` fields).
#[derive(Debug, Clone)]
pub struct QueryOptions {
    /// Field name to order by, as written in the schema.
    pub order_by: String,
    /// Sort direction.
    pub order_direction: OrderDir,
    /// Maximum rows to return.
    pub limit: i64,
    /// Rows to skip.
    pub offset: i64,
}

/// A direct-DB model bound to one entity's table in a schema.
pub struct PlainModel {
    schema: String,
    table: String,
    fields: Vec<EntityField>,
}

impl PlainModel {
    /// Bind a model to its table in `schema`.
    pub fn new(schema: impl Into<String>, model: &EntityModel) -> Self {
        Self {
            schema: schema.into(),
            table: model_to_table_name(&model.name),
            fields: model.fields.clone(),
        }
    }

    /// The generated table name for this entity.
    pub fn table(&self) -> &str {
        &self.table
    }

    /// Column names (underscored), sorted — matches introspection/dump ordering.
    fn sorted_columns(&self) -> Vec<(&EntityField, String)> {
        let mut cols: Vec<(&EntityField, String)> = self
            .fields
            .iter()
            .map(|f| (f, underscored(&f.name)))
            .collect();
        cols.sort_by(|a, b| a.1.cmp(&b.1));
        cols
    }

    /// The `$n`-placeholder expression that casts a text param to this field's type.
    ///
    /// Params are always bound as text, so the cast goes `::text::<target>` — this
    /// forces Postgres to expect a text parameter and then cast, rather than
    /// inferring the param type from the target (which rejects a String bind).
    fn placeholder(field: &EntityField, idx: usize) -> String {
        if field.is_array {
            return format!("${idx}::text::jsonb");
        }
        match field.base_type.as_str() {
            "ID" | "String" => format!("${idx}"),
            "Int" => format!("${idx}::text::integer"),
            "BigInt" => format!("${idx}::text::numeric"),
            "Float" => format!("${idx}::text::double precision"),
            "Boolean" => format!("${idx}::text::boolean"),
            "Date" => format!("${idx}::text::timestamp"),
            "Bytes" => format!("decode(${idx}::text, 'hex')"),
            "Json" => format!("${idx}::text::jsonb"),
            _ => format!("${idx}"),
        }
    }

    /// Encode a JSON entity value to the text form bound to a parameter.
    fn encode(field: &EntityField, v: Option<&Value>) -> Option<String> {
        match v {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => {
                if field.base_type == "Bytes" {
                    Some(s.strip_prefix("0x").unwrap_or(s).to_string())
                } else {
                    Some(s.clone())
                }
            }
            Some(Value::Bool(b)) => Some(b.to_string()),
            Some(Value::Number(n)) => Some(n.to_string()),
            Some(v @ (Value::Array(_) | Value::Object(_))) => Some(v.to_string()),
        }
    }

    /// `col::text` projection for canonical row output.
    fn select_expr(field: &EntityField, col: &str) -> String {
        if field.base_type == "Bytes" && !field.is_array {
            // bytea → unprefixed hex, matching Sequelize's Bytes get hook output.
            format!("encode(\"{col}\", 'hex') AS \"{col}\"")
        } else {
            format!("\"{col}\"::text AS \"{col}\"")
        }
    }

    /// Upsert entities (set / bulkCreate / bulkUpdate).
    pub async fn upsert(&self, db: &Database, entities: &[Value]) -> Result<()> {
        if entities.is_empty() {
            return Ok(());
        }
        let cols = self.sorted_columns();
        let col_list = cols
            .iter()
            .map(|(_, c)| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let update_set = cols
            .iter()
            .filter(|(f, _)| f.base_type != "ID")
            .map(|(_, c)| format!("\"{c}\" = EXCLUDED.\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");

        for entity in entities {
            let mut placeholders = Vec::with_capacity(cols.len());
            let mut params: Vec<Option<String>> = Vec::with_capacity(cols.len());
            for (i, (field, _)) in cols.iter().enumerate() {
                placeholders.push(Self::placeholder(field, i + 1));
                params.push(Self::encode(field, entity.get(&field.name)));
            }
            let sql = format!(
                "INSERT INTO \"{}\".\"{}\" ({}) VALUES ({}) \
                 ON CONFLICT (\"id\") DO UPDATE SET {}",
                self.schema,
                self.table,
                col_list,
                placeholders.join(", "),
                update_set,
            );
            let borrowed: Vec<&(dyn ToSql + Sync)> =
                params.iter().map(|p| p as &(dyn ToSql + Sync)).collect();
            db.execute(&sql, &borrowed).await?;
        }
        Ok(())
    }

    /// Delete entities by id (non-historical bulkRemove).
    pub async fn remove(&self, db: &Database, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let sql = format!(
            "DELETE FROM \"{}\".\"{}\" WHERE id = ANY($1)",
            self.schema, self.table
        );
        db.execute(&sql, &[&ids]).await?;
        Ok(())
    }

    /// All rows, canonicalized and ordered by id — the parity/dump entry point.
    pub async fn dump_canonical(&self, db: &Database) -> Result<Vec<CanonicalRow>> {
        let cols = self.sorted_columns();
        let projection = cols
            .iter()
            .map(|(f, c)| Self::select_expr(f, c))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {projection} FROM \"{}\".\"{}\" ORDER BY id",
            self.schema, self.table
        );
        let rows = db.query(&sql, &[]).await?;
        Ok(rows
            .iter()
            .map(|r| self.row_to_canonical(r, &cols))
            .collect())
    }

    /// Get a single entity by id, canonicalized.
    pub async fn get(&self, db: &Database, id: &str) -> Result<Option<CanonicalRow>> {
        let cols = self.sorted_columns();
        let projection = cols
            .iter()
            .map(|(f, c)| Self::select_expr(f, c))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {projection} FROM \"{}\".\"{}\" WHERE id = $1",
            self.schema, self.table
        );
        let rows = db.query(&sql, &[&id]).await?;
        Ok(rows.first().map(|r| self.row_to_canonical(r, &cols)))
    }

    /// Query by ANDed equality filters with ordering/pagination (getByField).
    pub async fn get_by_fields(
        &self,
        db: &Database,
        filters: &[(String, Value)],
        opts: &QueryOptions,
    ) -> Result<Vec<CanonicalRow>> {
        let cols = self.sorted_columns();
        let projection = cols
            .iter()
            .map(|(f, c)| Self::select_expr(f, c))
            .collect::<Vec<_>>()
            .join(", ");

        let mut where_parts = Vec::new();
        let mut params: Vec<Option<String>> = Vec::new();
        for (field_name, value) in filters {
            let field = self
                .fields
                .iter()
                .find(|f| &f.name == field_name)
                .ok_or_else(|| {
                    StoreError::UnsupportedType(format!("unknown field {field_name}"))
                })?;
            let col = underscored(field_name);
            let idx = params.len() + 1;
            where_parts.push(format!("\"{col}\" = {}", Self::placeholder(field, idx)));
            params.push(Self::encode(field, Some(value)));
        }

        let order_col = underscored(&opts.order_by);
        let dir = match opts.order_direction {
            OrderDir::Asc => "ASC",
            OrderDir::Desc => "DESC",
        };
        let where_clause = if where_parts.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_parts.join(" AND "))
        };
        let sql = format!(
            "SELECT {projection} FROM \"{}\".\"{}\" {where_clause} \
             ORDER BY \"{order_col}\" {dir} LIMIT {} OFFSET {}",
            self.schema, self.table, opts.limit, opts.offset
        );
        let borrowed: Vec<&(dyn ToSql + Sync)> =
            params.iter().map(|p| p as &(dyn ToSql + Sync)).collect();
        let rows = db.query(&sql, &borrowed).await?;
        Ok(rows
            .iter()
            .map(|r| self.row_to_canonical(r, &cols))
            .collect())
    }

    fn row_to_canonical(
        &self,
        row: &tokio_postgres::Row,
        cols: &[(&EntityField, String)],
    ) -> CanonicalRow {
        let mut out = BTreeMap::new();
        for (i, (_, col)) in cols.iter().enumerate() {
            out.insert(col.clone(), row.get::<_, Option<String>>(i));
        }
        out
    }
}
