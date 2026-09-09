//! Errors raised by the store layer.

use thiserror::Error;

/// Result alias for store operations.
pub type Result<T, E = StoreError> = std::result::Result<T, E>;

/// Failures the store can surface.
#[derive(Debug, Error)]
pub enum StoreError {
    /// The database rejected a statement or the connection dropped.
    #[error("postgres error: {0}")]
    Postgres(#[from] tokio_postgres::Error),

    /// A connection could not be taken from the pool.
    #[error("connection pool error: {0}")]
    Pool(String),

    /// A name destined for SQL text failed validation. Interpolating it would be
    /// an injection risk, so the operation is refused.
    #[error("invalid sql identifier: {0}")]
    InvalidIdentifier(String),

    /// The project's GraphQL schema could not be parsed.
    #[error("failed to parse GraphQL schema: {0}")]
    Schema(String),

    /// A GraphQL type has no Postgres representation.
    #[error("unsupported GraphQL type: {0}")]
    UnsupportedType(String),

    /// A named entity is absent from the project schema.
    #[error("unknown entity: {0}")]
    UnknownEntity(String),

    /// A stored value could not be read back as the expected type.
    #[error("could not decode stored value: {0}")]
    Decode(String),

    /// The database holds state from a different project, chain or schema
    /// version. Fatal: continuing would interleave two projects' data.
    #[error("metadata conflict: {0}")]
    MetadataConflict(String),
}
