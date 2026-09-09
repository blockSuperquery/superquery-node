//! The [`EntityStore`] trait — the interface mappings see.
//!
//! Guide §3.4 sketches `get`/`set`/`remove`; this adds the bulk and query calls a
//! real mapping needs, and threads `block_height` through every mutation because
//! historical mode versions writes by height (guide Milestone 11).
//!
//! Entities are `serde_json::Value` objects rather than typed structs: the schema
//! is the project's, known only at runtime, and mappings cross a WASM boundary
//! where JSON is the wire format anyway.
//!
//! Upstream analogue: `node-core/src/indexer/store/store.ts`.

use async_trait::async_trait;
use serde_json::Value;

use crate::error::Result;

/// A dynamic entity: a JSON object carrying at least a string `id`.
pub type Entity = Value;

/// Comparison operators available to `get_by_fields`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldOperator {
    /// Equal.
    Eq,
    /// Not equal.
    Ne,
    /// Member of a list.
    In,
    /// Not a member of a list.
    NotIn,
    /// Less than.
    Lt,
    /// Less than or equal.
    Lte,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
}

impl FieldOperator {
    /// The SQL operator, for the `In` and `NotIn` cases handled by the caller.
    pub fn sql(&self) -> &'static str {
        match self {
            FieldOperator::Eq => "=",
            FieldOperator::Ne => "!=",
            FieldOperator::Lt => "<",
            FieldOperator::Lte => "<=",
            FieldOperator::Gt => ">",
            FieldOperator::Gte => ">=",
            FieldOperator::In => "IN",
            FieldOperator::NotIn => "NOT IN",
        }
    }
}

/// A `[field, operator, value]` filter.
#[derive(Debug, Clone)]
pub struct FieldExpression {
    /// Field name as written in the schema.
    pub field: String,
    /// Comparison to apply.
    pub operator: FieldOperator,
    /// Value to compare against.
    pub value: Value,
}

/// Ordering and pagination for entity queries.
#[derive(Debug, Clone)]
pub struct GetOptions {
    /// Rows to skip.
    pub offset: u32,
    /// Maximum rows to return.
    pub limit: u32,
    /// Field to order by.
    pub order_by: String,
    /// Sort direction.
    pub order_direction: crate::entity::OrderDir,
}

impl Default for GetOptions {
    fn default() -> Self {
        Self {
            offset: 0,
            // Matches the `--query-limit` default: an unbounded query from a
            // mapping is a memory hazard, so there is always a cap.
            limit: 100,
            order_by: "id".to_string(),
            order_direction: crate::entity::OrderDir::Asc,
        }
    }
}

/// Entity persistence as mappings see it.
///
/// `block_height` on every mutation is what lets historical mode version writes,
/// and therefore what makes rewind possible. Implementations running with history
/// disabled ignore it.
#[async_trait]
pub trait EntityStore: Send + Sync {
    /// Fetch one entity by id.
    async fn get(&self, entity: &str, id: &str) -> Result<Option<Entity>>;

    /// Query entities by ANDed field expressions.
    async fn get_by_fields(
        &self,
        entity: &str,
        filters: &[FieldExpression],
        options: &GetOptions,
    ) -> Result<Vec<Entity>>;

    /// Fetch the first entity matching one field.
    async fn get_one_by_field(
        &self,
        entity: &str,
        field: &str,
        value: Value,
    ) -> Result<Option<Entity>>;

    /// Insert or update one entity.
    async fn set(&self, entity: &str, id: &str, data: Entity, block_height: u64) -> Result<()>;

    /// Insert or update many entities of one type.
    async fn bulk_create(&self, entity: &str, data: Vec<Entity>, block_height: u64) -> Result<()>;

    /// Delete one entity by id.
    async fn remove(&self, entity: &str, id: &str, block_height: u64) -> Result<()>;

    /// Delete many entities by id.
    async fn bulk_remove(&self, entity: &str, ids: Vec<String>, block_height: u64) -> Result<()>;
}

/// A store mutation, recorded in order.
///
/// Feeds two things: the rewind log (guide Milestone 11) and, later, Proof of
/// Index. The `Set`/`Remove` spelling is load-bearing for PoI — SubQuery hashes
/// those exact strings into its merkle leaves
/// (`node-core/src/indexer/StoreOperations.ts`), so a PoI meant to be comparable
/// must match them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationType {
    /// An entity was written.
    Set,
    /// An entity was deleted.
    Remove,
}

impl OperationType {
    /// The string hashed into a PoI leaf.
    pub fn as_str(&self) -> &'static str {
        match self {
            OperationType::Set => "Set",
            OperationType::Remove => "Remove",
        }
    }
}

/// One recorded store mutation.
#[derive(Debug, Clone)]
pub struct Operation {
    /// What happened.
    pub operation: OperationType,
    /// Entity type name.
    pub entity_type: String,
    /// The full entity for `Set`, or just the id for `Remove`.
    pub data: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_strings_match_upstream_poi_input() {
        // These exact strings are hashed into PoI leaves upstream; changing them
        // silently breaks proof comparability.
        assert_eq!(OperationType::Set.as_str(), "Set");
        assert_eq!(OperationType::Remove.as_str(), "Remove");
    }

    #[test]
    fn default_options_cap_unbounded_queries() {
        let o = GetOptions::default();
        assert_eq!(o.limit, 100);
        assert_eq!(o.offset, 0);
        assert_eq!(o.order_by, "id");
    }

    #[test]
    fn operator_sql_spellings() {
        assert_eq!(FieldOperator::Eq.sql(), "=");
        assert_eq!(FieldOperator::Ne.sql(), "!=");
        assert_eq!(FieldOperator::Gte.sql(), ">=");
        assert_eq!(FieldOperator::NotIn.sql(), "NOT IN");
    }
}
