//! Errors raised by the dispatch pipeline.

use thiserror::Error;

/// Result alias for dispatch operations.
pub type Result<T, E = DispatchError> = std::result::Result<T, E>;

/// Failures the dispatcher can surface.
#[derive(Debug, Error)]
pub enum DispatchError {
    /// The engine failed while processing a block.
    #[error(transparent)]
    Core(#[from] superquery_core::CoreError),

    /// The commit transaction failed.
    #[error(transparent)]
    Store(#[from] superquery_store::StoreError),

    /// A handler failed. Indexing stops rather than skipping the block: a gap
    /// would produce state no replay reproduces.
    #[error("block {height} failed to process: {reason}")]
    BlockFailed {
        /// The height that failed.
        height: u64,
        /// What went wrong.
        reason: String,
    },

    /// The pipeline was shut down while work was queued.
    #[error("dispatcher is shutting down")]
    ShuttingDown,
}
