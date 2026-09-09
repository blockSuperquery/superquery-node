//! # superquery-chain-evm
//!
//! The EVM implementation of [`superquery_chain_api::ChainAdapter`].
//!
//! ## The boundary this crate defends
//!
//! `alloy` is a dependency **here and nowhere else**. Guide Milestone 3's
//! acceptance is that the core crate has no chain-SDK imports, and
//! `tests/integration/crate_boundaries.rs` fails the build if that stops being
//! true. Adding a chain means adding a sibling crate, not editing the engine.
//!
//! ## What is complete
//!
//! - [`filter`] — log and transaction matching, the hot path that screens every
//!   log of every block before any mapping runs (guide Milestone 6).
//! - [`block`] — the block/transaction/log shapes mappings receive.
//! - [`ChainAdapter::decode_events`](superquery_chain_api::ChainAdapter::decode_events)
//!   on [`EvmAdapter`] — pairing logs with the transactions that emitted them.
//!
//! The RPC-backed methods return a clear error naming their milestone until phase
//! B2 lands them.
//!
//! Upstream analogue:
//! [`subquery/subql-ethereum`](https://github.com/subquery/subql-ethereum).

pub mod adapter;
pub mod block;
pub mod filter;

pub use adapter::{EvmAdapter, EvmAdapterConfig, SharedEvmAdapter};
pub use block::{EvmBlock, EvmEvent, EvmLog, EvmTransaction};
pub use filter::{EvmLogFilter, EvmTransactionFilter, LogView, TopicFilter, TransactionView};
