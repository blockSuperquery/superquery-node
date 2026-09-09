//! Rewinding after a reorg, and re-indexing the canonical branch.
//!
//! The store owns the SQL that undoes entity writes ([`superquery_store::rewind`]);
//! this module owns the sequencing around it:
//!
//! ```text
//! pause fetching
//!   -> drop queued blocks above the ancestor
//!   -> rewind entity state + checkpoint (one transaction)
//!   -> reset the scheduler cursor to ancestor + 1
//!   -> resume
//! ```
//!
//! Order matters. Rewinding before the queue is drained would let an in-flight
//! block from the abandoned branch commit *after* the rewind, silently restoring
//! state that no longer exists on chain.

use superquery_chain_api::BlockPtr;

use crate::error::Result;

/// What a rewind accomplished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewindReport {
    /// Block state was returned to.
    pub target: BlockPtr,
    /// Indexed blocks discarded.
    pub blocks_discarded: u64,
    /// Height indexing resumes at.
    pub resume_height: u64,
}

impl RewindReport {
    /// Build a report for a rewind to `target`.
    pub fn new(target: BlockPtr, blocks_discarded: u64) -> Self {
        let resume_height = target.height.saturating_add(1);
        Self {
            target,
            blocks_discarded,
            resume_height,
        }
    }
}

/// Execute a rewind to `target`.
///
/// # Milestone
///
/// Not yet implemented — depends on versioned entity writes
/// ([`superquery_store::rewind`], guide Milestone 11). [`RewindReport`] and the
/// detection logic in [`super::reorg`] are complete and tested, so the sequencing
/// above is the remaining piece.
pub async fn execute_rewind(
    _db: &superquery_store::Database,
    _target: &BlockPtr,
) -> Result<RewindReport> {
    unimplemented!(
        "rewind execution needs versioned entity writes; guide Milestone 11, task plan phase C2"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexing_resumes_at_the_block_after_the_ancestor() {
        let report = RewindReport::new(BlockPtr::new(100, "0x64"), 3);
        assert_eq!(report.resume_height, 101);
        assert_eq!(report.blocks_discarded, 3);
    }

    #[test]
    fn a_rewind_to_genesis_resumes_at_one() {
        let report = RewindReport::new(BlockPtr::new(0, "0x0"), 1);
        assert_eq!(report.resume_height, 1);
    }
}
