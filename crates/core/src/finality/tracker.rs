//! Tracks how far finality has advanced, and drives the reorg check.
//!
//! Sits between the chain adapter (which knows the canonical chain) and the store
//! (which knows what we indexed), turning the pure decision logic in
//! [`super::reorg`] into an actual verdict.

use std::sync::Arc;

use superquery_chain_api::{BlockPtr, ChainAdapter};
use superquery_store::{CheckpointStore, Database};

use crate::error::Result;
use crate::finality::reorg::{find_common_ancestor, HeightComparison, ReorgDecision};

/// How many blocks of header history to compare when checking for a reorg.
///
/// Reorgs deeper than this on a chain with real finality indicate something has
/// gone badly wrong, and [`ReorgDecision::BeyondHistory`] is the honest answer.
pub const DEFAULT_REORG_SEARCH_DEPTH: u64 = 200;

/// Watches finality and detects forks.
pub struct FinalityTracker<A: ChainAdapter> {
    adapter: Arc<A>,
    checkpoints: CheckpointStore,
    search_depth: u64,
    finalized_height: u64,
}

impl<A: ChainAdapter> FinalityTracker<A> {
    /// Build a tracker over a chain and a project schema.
    pub fn new(adapter: Arc<A>, checkpoints: CheckpointStore) -> Self {
        Self {
            adapter,
            checkpoints,
            search_depth: DEFAULT_REORG_SEARCH_DEPTH,
            finalized_height: 0,
        }
    }

    /// Override how deep the reorg comparison walks.
    pub fn with_search_depth(mut self, depth: u64) -> Self {
        self.search_depth = depth.max(1);
        self
    }

    /// Last observed finalized height.
    pub fn finalized_height(&self) -> u64 {
        self.finalized_height
    }

    /// Refresh the finalized height from the chain.
    pub async fn refresh(&mut self) -> Result<u64> {
        // Finality only moves forward; a lower reading is endpoint lag, not a
        // rollback of finality, and must not un-finalize committed blocks.
        let reported = self.adapter.finalized_height().await?;
        self.finalized_height = self.finalized_height.max(reported);
        Ok(self.finalized_height)
    }

    /// Check indexed state against the canonical chain.
    ///
    /// Cheap when nothing is wrong: one header lookup at the tip, and the walk
    /// only continues while hashes keep disagreeing.
    pub async fn check(&self, db: &Database, tip: &BlockPtr) -> Result<ReorgDecision> {
        let floor = tip.height.saturating_sub(self.search_depth);
        let stored = self
            .checkpoints
            .headers_descending(db, tip.height, floor)
            .await?;

        let mut comparisons = Vec::with_capacity(stored.len());
        for header in stored {
            let canonical = match self.adapter.header_at(header.height).await {
                Ok(h) => Some(h.hash),
                // A height the endpoint cannot serve is treated as diverged: the
                // walk continues deeper rather than declaring the chain canonical
                // on missing evidence.
                Err(e) if !e.is_retryable() => None,
                Err(e) => return Err(e.into()),
            };

            let matched = canonical.as_deref() == Some(header.hash.as_str());
            comparisons.push(HeightComparison {
                height: header.height,
                stored_hash: header.hash,
                canonical_hash: canonical,
            });

            // Stop as soon as a height agrees: everything below it agrees too, by
            // the hash chain.
            if matched {
                break;
            }
        }

        Ok(find_common_ancestor(&comparisons))
    }
}
