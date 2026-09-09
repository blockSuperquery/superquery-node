//! # superquery-chain-api
//!
//! The chain abstraction boundary. Everything the SuperQuery engine knows about
//! blockchains is declared here; everything a blockchain knows about SuperQuery is
//! implemented against it.
//!
//! ```text
//! superquery-core ──depends on──> superquery-chain-api <──implements── superquery-chain-evm
//! ```
//!
//! This crate deliberately has **no chain SDK dependencies**. Adding one here
//! would leak chain specifics into the engine and break guide Milestone 3's
//! acceptance ("core crate has no Alloy imports").
//!
//! ## Where to start
//!
//! - [`ChainAdapter`] — the trait a new chain implements.
//! - [`Header`] / [`BlockPtr`] — how the engine names blocks.
//! - [`Filter`] — how handler filters reach the adapter.
//!
//! See `.claude/docs/SUBQUERY_REFERENCE_MAP.md` for the upstream sources each
//! item was derived from.

#![doc(html_no_source)]

pub mod adapter;
pub mod block;
pub mod error;
pub mod filter;

pub use adapter::{BlockCandidateProvider, ChainAdapter};
pub use block::{BlockPtr, FinalityState, GenericBlock, Header, IBlock};
pub use error::{ChainError, Result};
pub use filter::{BlockFilter, Filter, HandlerKind};
