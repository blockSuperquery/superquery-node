//! Errors raised by the indexing engine.

use thiserror::Error;

/// Result alias used across the engine.
pub type Result<T, E = CoreError> = std::result::Result<T, E>;

/// Failures the engine can surface.
#[derive(Debug, Error)]
pub enum CoreError {
    /// The chain adapter failed.
    #[error(transparent)]
    Chain(#[from] superquery_chain_api::ChainError),

    /// The store failed.
    #[error(transparent)]
    Store(#[from] superquery_store::StoreError),

    /// A block could not be indexed because no data source covers its height.
    #[error("no data sources active at height {height}")]
    NoDataSources {
        /// The height with no coverage.
        height: u64,
    },

    /// A reorg was detected deeper than the retained header history, so the
    /// common ancestor cannot be established.
    #[error(
        "reorg extends below the retained header history (oldest known height {oldest_known}); \
         re-index from a known-good height"
    )]
    ReorgBeyondHistory {
        /// Oldest height still in `_superquery_blocks`.
        oldest_known: u64,
    },

    /// A mapping handler failed.
    #[error("handler '{handler}' failed at block {height}: {source}")]
    Handler {
        /// The handler function name.
        handler: String,
        /// Block being indexed.
        height: u64,
        /// Underlying cause.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The project could not be loaded or is invalid.
    #[error("project error: {0}")]
    Project(String),

    /// Anything else.
    #[error("{0}")]
    Other(String),
}
