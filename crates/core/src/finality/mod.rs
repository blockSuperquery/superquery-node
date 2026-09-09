//! Finality and reorg handling.
//!
//! Guide §3.6. Three concerns, deliberately separated so the risky logic can be
//! tested without a chain:
//!
//! - [`reorg`] — pure decision logic: given stored vs canonical hashes, is this a
//!   fork, and how deep?
//! - [`tracker`] — the I/O that gathers those hashes and tracks finality.
//! - [`rewind`] — sequencing the undo once a fork is confirmed.

pub mod reorg;
pub mod rewind;
pub mod tracker;

pub use reorg::{continues_chain, find_common_ancestor, HeightComparison, ReorgDecision};
pub use rewind::RewindReport;
pub use tracker::{FinalityTracker, DEFAULT_REORG_SEARCH_DEPTH};
