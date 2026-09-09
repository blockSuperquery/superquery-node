//! # superquery-runtime
//!
//! Runs project mappings in a WebAssembly sandbox.
//!
//! ```text
//! Rust mapping project
//!       |  cargo build --target wasm32-wasip1
//!       v
//!  mapping.wasm
//!       |
//!       v
//!   Wasmtime  --+-- sq_store_get
//!               +-- sq_store_set
//!               +-- sq_store_remove
//!               +-- sq_log
//!               +-- sq_chain_call
//! ```
//!
//! ## Why not a JavaScript sandbox
//!
//! SubQuery runs mappings in a JS VM. Reproducing that would inherit its
//! weaknesses without a compensating benefit, so SuperQuery uses WASM instead
//! (guide §3.5). The three properties that matter:
//!
//! - **determinism** — the same inputs produce the same writes, which is what
//!   makes replay-after-reorg and Proof of Index meaningful;
//! - **enforceable limits** — fuel and memory caps are engine properties, not
//!   promises from cooperating guest code;
//! - **default deny** — WASI is not linked, so a module's only reach into the
//!   world is the five host functions above.
//!
//! ## Layout
//!
//! - [`abi`] — the versioned contract, mirroring `ABI.md`.
//! - [`host`] — what a mapping is allowed to do.
//! - [`runtime`] — load-time validation and the runtime trait.
//! - [`wasmtime_host`] — the Wasmtime engine configuration.
//! - [`limits`] — fuel, timeout and memory bounds.

pub mod abi;
pub mod error;
pub mod host;
pub mod limits;
pub mod runtime;
pub mod wasmtime_host;

pub use abi::{BlockContext, DataSourceContext, HandlerInput, LogLevel, Status, ABI_VERSION};
pub use error::{Result, RuntimeError};
pub use host::HostContext;
pub use limits::RuntimeLimits;
pub use runtime::{check_abi_version, check_imports, check_required_exports, MappingRuntime};
pub use wasmtime_host::{build_engine, WasmtimeRuntime};
