//! The fetch loop: decide what to fetch, fetch it, hand it to the dispatcher.
//!
//! ```text
//! latest safe height
//!        |
//!        v
//! calculate [start..end]        <- fetch::range
//!        |
//!        v
//! fetch concurrently            <- bounded by fetch::backpressure
//!        |
//!        v
//! ordered dispatch              <- superquery-dispatcher
//! ```
//!
//! Upstream analogue: `node-core/src/indexer/fetch.service.ts`.

use std::sync::Arc;

use superquery_chain_api::ChainAdapter;

use crate::error::Result;
use crate::fetch::backpressure::Backpressure;
use crate::fetch::range::{safe_head, RangePlan};

/// Runtime settings for one fetch loop.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// First height to index.
    pub start_height: u64,
    /// Last height to index, if the run is bounded.
    pub end_height: Option<u64>,
    /// Blocks per batch.
    pub batch_size: u32,
    /// Whether to index past the finalized height.
    pub index_unfinalized: bool,
    /// Depth treated as final on chains without a finality gadget.
    pub finality_confirmations: u64,
    /// Concurrency and queue limits.
    pub backpressure: Backpressure,
}

/// Drives block fetching for one chain.
pub struct FetchScheduler<A: ChainAdapter> {
    adapter: Arc<A>,
    config: SchedulerConfig,
    plan: RangePlan,
    next_height: u64,
}

impl<A: ChainAdapter> FetchScheduler<A> {
    /// Create a scheduler positioned at `config.start_height`.
    pub fn new(adapter: Arc<A>, config: SchedulerConfig, plan: RangePlan) -> Self {
        let next_height = config.start_height;
        Self {
            adapter,
            config,
            plan,
            next_height,
        }
    }

    /// The next height not yet dispatched.
    pub fn next_height(&self) -> u64 {
        self.next_height
    }

    /// Resume from a checkpoint: continue at the block after the last committed one.
    pub fn resume_from(&mut self, last_indexed: u64) {
        self.next_height = last_indexed.saturating_add(1).max(self.config.start_height);
    }

    /// Ask the chain how far it is currently safe to index.
    pub async fn safe_head(&self) -> Result<u64> {
        let latest = self.adapter.latest_height().await?;
        let finalized = self.adapter.finalized_height().await?;
        Ok(safe_head(
            latest,
            finalized,
            self.config.index_unfinalized,
            self.config.finality_confirmations,
        ))
    }

    /// The heights to fetch next, or an empty vec when caught up.
    ///
    /// Advances the cursor, so each call yields a distinct batch.
    pub async fn next_heights(&mut self, in_flight: u32, queued: u32) -> Result<Vec<u64>> {
        let head = self.safe_head().await?;
        let budget =
            self.config
                .backpressure
                .clamp_batch(self.config.batch_size, in_flight, queued);
        if budget == 0 {
            return Ok(Vec::new());
        }

        let Some(range) =
            self.plan
                .next_batch(self.next_height, head, budget, self.config.end_height)
        else {
            return Ok(Vec::new());
        };

        self.next_height = range.end.saturating_add(1);
        Ok(self.plan.heights_in(&range))
    }

    /// Run the fetch loop until the end height is reached or the task is
    /// cancelled.
    ///
    /// # Milestone
    ///
    /// Not yet implemented — needs the dispatcher wiring from guide Milestone 7.
    /// [`FetchScheduler::next_heights`] is the decision half and is usable and
    /// tested now; this is the driving half.
    pub async fn run(&mut self) -> Result<()> {
        unimplemented!(
            "fetch loop needs dispatcher wiring; guide Milestone 5/7, task plan phase B3"
        )
    }
}
