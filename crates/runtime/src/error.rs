//! Errors raised by the mapping runtime.

use thiserror::Error;

/// Result alias for runtime operations.
pub type Result<T, E = RuntimeError> = std::result::Result<T, E>;

/// Failures loading or running a mapping module.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// The module declares an ABI version this host does not implement.
    #[error("mapping ABI version mismatch: module declares {found}, host implements {expected}")]
    AbiVersionMismatch {
        /// Version the module declared.
        found: i32,
        /// Version this host implements.
        expected: i32,
    },

    /// The module is missing an export the ABI requires.
    #[error("mapping module is missing required export '{0}'")]
    MissingExport(String),

    /// The module imports something outside the permitted host surface.
    ///
    /// Refused at load rather than trapped at call time, so a mapping that wants
    /// filesystem or network access fails immediately and visibly.
    #[error("mapping module imports '{module}::{name}', which is not permitted")]
    ForbiddenImport {
        /// Import module name.
        module: String,
        /// Import function name.
        name: String,
    },

    /// The Wasm module could not be compiled or instantiated.
    #[error("failed to load mapping module: {0}")]
    Load(String),

    /// A handler exhausted its fuel budget.
    #[error("handler '{handler}' exhausted its fuel budget")]
    FuelExhausted {
        /// The handler that ran out.
        handler: String,
    },

    /// A handler exceeded its wall-clock budget.
    #[error("handler '{handler}' timed out after {elapsed_ms}ms")]
    Timeout {
        /// The handler that timed out.
        handler: String,
        /// How long it ran.
        elapsed_ms: u64,
    },

    /// A handler exceeded its memory ceiling.
    #[error("handler '{handler}' exceeded its memory limit")]
    OutOfMemory {
        /// The handler that overran.
        handler: String,
    },

    /// A handler trapped.
    #[error("handler '{handler}' trapped: {reason}")]
    Trap {
        /// The handler that trapped.
        handler: String,
        /// The trap message.
        reason: String,
    },

    /// A value could not be moved across the ABI boundary.
    #[error("ABI boundary error: {0}")]
    Abi(String),

    /// The store rejected an operation a host function attempted.
    #[error(transparent)]
    Store(#[from] superquery_store::StoreError),
}
