//! The dispatcher: the pipeline between fetched blocks and committed state.
//!
//! ```text
//! Fetcher 1 --+
//! Fetcher 2 --+--> bounded queue --> workers --> ordered commit
//! Fetcher N --+
//! ```
//!
//! Its whole job is to allow concurrency everywhere it is safe and forbid it at
//! the one place it is not. Fetching and mapping run in parallel; commits are
//! serialised through [`crate::ordered_commit::OrderedCommitBuffer`], so the
//! database sees blocks in height order regardless of how they arrived.
//!
//! Upstream analogue: `node-core/src/indexer/blockDispatcher/`.

use async_trait::async_trait;

use crate::error::Result;
use crate::limits::DispatchLimits;
use crate::ordered_commit::OrderedCommitBuffer;
use crate::worker::WorkerOutput;

/// Live pipeline state, for metrics and `/health`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DispatchStatus {
    /// Blocks fetched but not yet processed.
    pub queued: u32,
    /// Blocks currently in a worker.
    pub in_flight: u32,
    /// Processed blocks waiting for a predecessor before they may commit.
    pub awaiting_commit: u32,
    /// Highest height committed.
    pub last_committed: u64,
}

/// Drives blocks from the queue through mapping to a committed state.
#[async_trait]
pub trait BlockDispatcher: Send + Sync {
    /// The fetched-block type flowing through the pipeline.
    type Block: Send + Sync;

    /// Enqueue a fetched block, waiting while the queue is full.
    async fn enqueue(&self, height: u64, block: Self::Block) -> Result<()>;

    /// Current pipeline state.
    fn status(&self) -> DispatchStatus;

    /// Discard queued and buffered blocks, resuming at `height`.
    ///
    /// Called before a rewind: work from the abandoned branch must be dropped
    /// before state is undone, never after.
    async fn flush(&self, height: u64) -> Result<()>;

    /// Wait for everything currently queued to commit.
    async fn drain(&self) -> Result<()>;
}

/// Commits one block's writes.
///
/// Separate from [`BlockDispatcher`] so the commit boundary — the transaction
/// required by guide Milestone 10 — can be tested against a fake.
#[async_trait]
pub trait BlockCommitter: Send + Sync {
    /// Commit one block's entity operations, dynamic data sources and checkpoint
    /// in a single transaction.
    async fn commit(&self, output: WorkerOutput) -> Result<()>;
}

/// The single-process Tokio dispatcher.
///
/// # Milestone
///
/// The ordering logic this type coordinates is complete and tested in
/// [`crate::ordered_commit`]; the task wiring that drives it lands with guide
/// Milestone 7 (task plan phase B3).
pub struct TokioDispatcher {
    limits: DispatchLimits,
    buffer: tokio::sync::Mutex<OrderedCommitBuffer<WorkerOutput>>,
}

impl TokioDispatcher {
    /// Create a dispatcher expecting `start_height` first.
    pub fn new(start_height: u64, limits: DispatchLimits) -> Self {
        Self {
            buffer: tokio::sync::Mutex::new(OrderedCommitBuffer::new(
                start_height,
                limits.reorder_capacity,
            )),
            limits,
        }
    }

    /// The configured limits.
    pub fn limits(&self) -> DispatchLimits {
        self.limits
    }

    /// The current generation. Workers capture this when they take a block, and
    /// pass it back to [`TokioDispatcher::take_committable`].
    pub async fn generation(&self) -> u64 {
        self.buffer.lock().await.generation()
    }

    /// Record a completed block and take everything now ready to commit, in
    /// height order.
    ///
    /// `generation` is the value read when the work started. Output from a
    /// generation a reorg has abandoned is discarded.
    pub async fn take_committable(
        &self,
        generation: u64,
        output: WorkerOutput,
    ) -> Vec<WorkerOutput> {
        let mut buffer = self.buffer.lock().await;
        buffer
            .complete_from(generation, output.height, output)
            .into_iter()
            .map(|(_, output)| output)
            .collect()
    }

    /// Discard buffered work and resume at `height`, returning how much was
    /// dropped.
    ///
    /// Starts a new generation, so work already in flight cannot commit.
    pub async fn reset_to(&self, height: u64) -> usize {
        self.buffer.lock().await.reset_to(height)
    }

    /// The height that must arrive before anything can commit.
    pub async fn next_expected(&self) -> u64 {
        self.buffer.lock().await.next_expected()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use superquery_core::ProcessedBlock;

    fn output(height: u64) -> WorkerOutput {
        WorkerOutput {
            height,
            processed: ProcessedBlock::default(),
        }
    }

    #[tokio::test]
    async fn commits_are_released_in_height_order() {
        let d = TokioDispatcher::new(100, DispatchLimits::default());
        let generation = d.generation().await;

        // Out of order in...
        assert!(d.take_committable(generation, output(102)).await.is_empty());
        assert!(d.take_committable(generation, output(101)).await.is_empty());

        // ...in order out.
        let released = d.take_committable(generation, output(100)).await;
        assert_eq!(
            released.iter().map(|o| o.height).collect::<Vec<_>>(),
            vec![100, 101, 102]
        );
        assert_eq!(d.next_expected().await, 103);
    }

    #[tokio::test]
    async fn reset_drops_buffered_work_from_the_abandoned_branch() {
        let d = TokioDispatcher::new(100, DispatchLimits::default());
        let old = d.generation().await;
        d.take_committable(old, output(102)).await;
        d.take_committable(old, output(103)).await;

        assert_eq!(d.reset_to(100).await, 2);
        assert_eq!(d.next_expected().await, 100);

        // A worker that started before the reorg finishes late; its height is
        // plausible on the new branch, but its generation is not.
        assert!(d.take_committable(old, output(101)).await.is_empty());

        // The canonical branch commits normally.
        let current = d.generation().await;
        let released = d.take_committable(current, output(100)).await;
        assert_eq!(
            released.iter().map(|o| o.height).collect::<Vec<_>>(),
            vec![100]
        );
    }
}
