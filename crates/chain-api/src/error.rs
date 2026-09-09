//! Errors raised at the chain boundary.

use thiserror::Error;

/// Result alias for chain operations.
pub type Result<T, E = ChainError> = std::result::Result<T, E>;

/// Failures a [`ChainAdapter`](crate::ChainAdapter) can surface.
///
/// The split matters to the fetch scheduler: [`ChainError::Transport`] and
/// [`ChainError::RateLimited`] are retryable, the rest are not. Adapters must
/// classify accordingly rather than collapsing everything into `Other`.
#[derive(Debug, Error)]
pub enum ChainError {
    /// The RPC endpoint could not be reached, or the response was malformed.
    /// Retryable.
    #[error("transport error: {0}")]
    Transport(String),

    /// The endpoint applied rate limiting. Retryable, ideally after `retry_after`.
    #[error("rate limited by endpoint{}", .retry_after_ms.map(|ms| format!(" (retry after {ms}ms)")).unwrap_or_default())]
    RateLimited {
        /// Server-advised backoff in milliseconds, when provided.
        retry_after_ms: Option<u64>,
    },

    /// The requested height is not available on this endpoint (pruned, or ahead
    /// of the chain head).
    #[error("block {height} unavailable")]
    BlockUnavailable {
        /// The height that could not be served.
        height: u64,
    },

    /// The endpoint served a chain other than the one the project targets.
    /// Fatal — indexing the wrong chain silently corrupts state.
    #[error("chain id mismatch: project expects {expected}, endpoint reports {actual}")]
    ChainIdMismatch {
        /// Chain id the project manifest declares.
        expected: String,
        /// Chain id the endpoint reported.
        actual: String,
    },

    /// A block or event payload could not be decoded.
    #[error("decode error: {0}")]
    Decode(String),

    /// Anything else the adapter needs to report.
    #[error("{0}")]
    Other(String),
}

impl ChainError {
    /// Whether the fetch scheduler should retry the operation that produced this
    /// error, rather than failing the block.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ChainError::Transport(_) | ChainError::RateLimited { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_classification() {
        assert!(ChainError::Transport("timeout".into()).is_retryable());
        assert!(ChainError::RateLimited {
            retry_after_ms: Some(500)
        }
        .is_retryable());

        assert!(!ChainError::BlockUnavailable { height: 7 }.is_retryable());
        assert!(!ChainError::ChainIdMismatch {
            expected: "1".into(),
            actual: "137".into(),
        }
        .is_retryable());
        assert!(!ChainError::Decode("bad rlp".into()).is_retryable());
    }
}
