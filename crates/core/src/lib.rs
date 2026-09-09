//! # superquery-core
//!
//! The chain-agnostic indexing engine: it decides which blocks to fetch, runs the
//! project's handlers against them, and keeps indexed state consistent with the
//! canonical chain.
//!
//! ```text
//!   ChainAdapter                        superquery-store
//!        |                                     ^
//!        v                                     |
//!   fetch::scheduler --> dispatcher --> indexer::manager
//!        ^                                     |
//!        |                                     v
//!   finality::tracker <----------------- checkpoints
//! ```
//!
//! ## The one hard rule
//!
//! This crate must never depend on a chain SDK. Everything it knows about
//! blockchains comes through [`superquery_chain_api`]; anything EVM-specific lives
//! in `superquery-chain-evm`. That is guide Milestone 3's acceptance criterion, and
//! `tests/integration/crate_boundaries.rs` enforces it.
//!
//! ## Where the interesting logic is
//!
//! - [`fetch::range`] — batch calculation, bypass spans, safe-head selection.
//! - [`fetch::backpressure`] — why the fetcher cannot outrun the pipeline.
//! - [`finality::reorg`] — common-ancestor search.
//! - [`project::BlockHeightMap`] — which data sources are active at a height.

pub mod error;
pub mod fetch;
pub mod finality;
pub mod indexer;
pub mod metrics;
pub mod project;

pub use error::{CoreError, Result};
pub use fetch::{Backpressure, BlockRange, FetchScheduler, RangePlan, SchedulerConfig};
pub use finality::{FinalityTracker, ReorgDecision};
pub use indexer::{IndexerManager, ProcessedBlock};
pub use project::{BlockHeightMap, DataSource, DynamicDataSource};
