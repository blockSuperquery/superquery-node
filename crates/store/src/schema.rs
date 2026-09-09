//! GraphQL SDL → entity model. A focused port of the `@subql/utils`
//! `getAllEntitiesRelations` surface that the store needs to build tables:
//! `@entity` object types and their scalar/array fields.
//!
//! Scope (M1 first slice): scalar & scalar-array fields. Enums, `@jsonField`
//! nested types, and relations (`@derivedFrom`, foreign keys) are added in
//! later slices — each with its own ground-truth fixture.

use graphql_parser::schema::{Definition, Type, TypeDefinition};

use crate::error::StoreError;

/// A parsed entity (`type X @entity`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityModel {
    /// Entity name as written in the schema (e.g. `Transfer`).
    pub name: String,
    /// The entity's fields, in schema order.
    pub fields: Vec<EntityField>,
}

/// A single entity field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityField {
    /// Field name as written (e.g. `blockHeight`).
    pub name: String,
    /// Base scalar/type name (e.g. `String`, `BigInt`, `ID`).
    pub base_type: String,
    /// Whether the field is a list.
    pub is_array: bool,
    /// Whether the field is nullable (top-level not `!`).
    pub nullable: bool,
}

/// Parse an SDL document into its `@entity` models.
pub fn parse_entities(sdl: &str) -> Result<Vec<EntityModel>, StoreError> {
    let doc = graphql_parser::schema::parse_schema::<String>(sdl)
        .map_err(|e| StoreError::Schema(e.to_string()))?;

    let mut models = Vec::new();
    for def in doc.definitions {
        let Definition::TypeDefinition(TypeDefinition::Object(obj)) = def else {
            continue;
        };
        let is_entity = obj.directives.iter().any(|d| d.name == "entity");
        if !is_entity {
            continue;
        }
        let fields = obj
            .fields
            .into_iter()
            .map(|f| {
                let (base_type, is_array) = base_and_array(&f.field_type);
                EntityField {
                    name: f.name,
                    base_type,
                    is_array,
                    nullable: !matches!(f.field_type, Type::NonNullType(_)),
                }
            })
            .collect();
        models.push(EntityModel {
            name: obj.name,
            fields,
        });
    }
    Ok(models)
}

/// Unwrap a GraphQL type to its base named type and whether a list is present.
fn base_and_array(ty: &Type<'_, String>) -> (String, bool) {
    match ty {
        Type::NamedType(n) => (n.clone(), false),
        Type::ListType(inner) => {
            let (base, _) = base_and_array(inner);
            (base, true)
        }
        Type::NonNullType(inner) => base_and_array(inner),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SDL: &str = r#"
        type Transfer @entity {
          id: ID!
          amount: BigInt!
          recipient: String
          tags: [String!]
        }
        type NotAnEntity {
          id: ID!
        }
    "#;

    #[test]
    fn parses_only_entities() {
        let models = parse_entities(SDL).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "Transfer");
    }

    #[test]
    fn field_attributes() {
        let m = &parse_entities(SDL).unwrap()[0];
        let by = |n: &str| m.fields.iter().find(|f| f.name == n).unwrap();

        assert_eq!(by("id").base_type, "ID");
        assert!(!by("id").nullable);
        assert!(!by("id").is_array);

        assert!(!by("amount").nullable);
        assert!(by("recipient").nullable);

        assert!(by("tags").is_array);
        assert!(by("tags").nullable); // list itself is nullable
        assert_eq!(by("tags").base_type, "String");
    }
}
