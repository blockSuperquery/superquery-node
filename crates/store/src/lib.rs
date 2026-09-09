//! # superquery-store
//!
//! PostgreSQL persistence: the project's entity tables, the node's checkpoints,
//! and the transaction boundary that keeps them consistent.
//!
//! ## Layout of a project schema
//!
//! ```text
//! <schema>
//! ├── transfers                 generated from `type Transfer @entity`
//! ├── accounts                  …one table per entity
//! ├── _superquery_metadata      project/chain identity + checkpoint
//! └── _superquery_blocks        recent header chain, for reorg detection
//! ```
//!
//! ## The commit rule
//!
//! Guide Milestone 10: a block's entity mutations and its checkpoint update land
//! in one transaction, or neither does. [`Database::transaction`] is that
//! boundary; [`checkpoint::CheckpointStore::commit_tx`] is deliberately only
//! callable inside one.
//!
//! ## Why raw SQL
//!
//! Entity tables are generated at runtime from each project's GraphQL schema, so
//! there is nothing static for a query builder or ORM to check. See task plan
//! §4.1 for the full reasoning.

pub mod checkpoint;
pub mod ddl;
pub mod entity;
pub mod error;
pub mod introspect;
pub mod metadata;
pub mod migration;
pub mod naming;
pub mod postgres;
pub mod rewind;
pub mod schema;
pub mod store;

pub use checkpoint::{Checkpoint, CheckpointStore, StoredHeader};
pub use entity::{CanonicalRow, OrderDir, PlainModel, QueryOptions};
pub use error::{Result, StoreError};
pub use introspect::{ColumnInfo, IndexInfo, SchemaInfo, TableInfo};
pub use metadata::{InitOutcome, MetadataStore};
pub use postgres::Database;
pub use schema::{parse_entities, EntityField, EntityModel};
pub use store::{
    Entity, EntityStore, FieldExpression, FieldOperator, GetOptions, Operation, OperationType,
};
