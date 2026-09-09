//! DDL generation — GraphQL entity models → Postgres `CREATE TABLE`.
//!
//! Reproduces the schema the TS node produces via Sequelize `sync()`. The port
//! targets, verified against ground-truth fixtures (see `tests/schema_parity.rs`):
//!
//! - table name  = `modelToTableName` = `underscored(pluralize(Name))`
//! - column name = `underscored(fieldName)`
//! - column type = `getColumnOption` + `sequelizeToPostgresTypeMap`: arrays →
//!   jsonb; ID/String → text; Int → integer; BigInt → numeric; Float → double
//!   precision; Boolean → boolean; Bytes → bytea; Date → timestamp.
//! - `id` (type `ID`) is the primary key; non-nullable fields get `NOT NULL`.
//!
//! Comparison is done on the *introspected* result, not this DDL text, so the
//! exact type keywords here need only resolve to the same Postgres type.

use crate::error::StoreError;
use crate::naming::{model_to_table_name, underscored};
use crate::schema::{EntityField, EntityModel};

/// Map a field to its Postgres column type (the `getColumnOption` type branch).
fn column_type(field: &EntityField) -> Result<&'static str, StoreError> {
    if field.is_array {
        // getColumnOption: isArray || jsonInterface → Json (JSONB)
        return Ok("jsonb");
    }
    Ok(match field.base_type.as_str() {
        "ID" | "String" => "text",
        "Int" => "integer",
        "BigInt" => "numeric",
        "Float" => "double precision",
        "Boolean" => "boolean",
        "Bytes" => "bytea",
        "Date" => "timestamp",
        "Json" => "jsonb",
        other => return Err(StoreError::UnsupportedType(other.to_string())),
    })
}

/// Generate the `CREATE TABLE` statement for one entity in `schema`.
pub fn create_table(model: &EntityModel, schema: &str) -> Result<String, StoreError> {
    let table = model_to_table_name(&model.name);

    let mut columns = Vec::with_capacity(model.fields.len());
    let mut primary_key: Option<String> = None;

    for field in &model.fields {
        let col = underscored(&field.name);
        let ty = column_type(field)?;
        let not_null = if field.nullable { "" } else { " NOT NULL" };
        columns.push(format!("  \"{col}\" {ty}{not_null}"));
        if field.base_type == "ID" {
            primary_key = Some(col);
        }
    }

    if let Some(pk) = primary_key {
        columns.push(format!("  PRIMARY KEY (\"{pk}\")"));
    }

    Ok(format!(
        "CREATE TABLE \"{schema}\".\"{table}\" (\n{}\n);",
        columns.join(",\n")
    ))
}

/// Generate DDL for every model in a schema.
pub fn create_tables(models: &[EntityModel], schema: &str) -> Result<Vec<String>, StoreError> {
    models.iter().map(|m| create_table(m, schema)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::parse_entities;

    #[test]
    fn generates_expected_ddl_shape() {
        let sdl =
            "type Transfer @entity { id: ID! amount: BigInt! recipient: String tags: [String!] }";
        let models = parse_entities(sdl).unwrap();
        let ddl = create_table(&models[0], "app").unwrap();

        assert!(
            ddl.starts_with("CREATE TABLE \"app\".\"transfers\" ("),
            "got: {ddl}"
        );
        assert!(ddl.contains("\"id\" text NOT NULL"));
        assert!(ddl.contains("\"amount\" numeric NOT NULL"));
        assert!(ddl.contains("\"recipient\" text"));
        assert!(!ddl.contains("\"recipient\" text NOT NULL"));
        assert!(ddl.contains("\"tags\" jsonb"));
        assert!(ddl.contains("PRIMARY KEY (\"id\")"));
    }

    #[test]
    fn unsupported_type_errors() {
        let sdl = "type X @entity { id: ID! weird: SomethingUnknown }";
        let models = parse_entities(sdl).unwrap();
        assert!(matches!(
            create_table(&models[0], "app"),
            Err(StoreError::UnsupportedType(_))
        ));
    }
}
