//! The reorder buffer that makes concurrent fetching safe.
//!
//! Guide §3.3 states the rule:
//!
//! > Fetching may be concurrent, but database commits that affect deterministic
//! > indexed state must respect the ordering guarantees required by your project
//! > model.
//!
//! Blocks come back from the RPC endpoint in whatever order the network delivers
//! them. Committing in that order would be wrong: handlers read the state earlier
//! blocks wrote, so block 101 committing before block 100 produces a different
//! database than the same blocks indexed sequentially. Guide Milestone 7's
//! acceptance — "varying fetch completion order yields the same final DB state" —
//! is exactly this.
//!
//! So completions are buffered by height and released only as a contiguous run:
//!
//! ```text
//! arrive:  103, 101, 100, 102
//! release: (none), (none), [100, 101], [102, 103]
//! ```
//!
//! Block 100 arriving is what unblocks 101; 102 arriving releases both 102 and the
//! 103 that had been waiting since the start.

use std::collections::BTreeMap;

/// Buffers out-of-order completions and releases them in height order.
///
/// `T` is whatever a completed block carries — typically its processed operations.
///
/// ## Generations
///
/// Height alone cannot tell two branches apart. After a reorg rewinds to 99 and
/// indexing resumes at 100, a block 101 still in flight *from the abandoned
/// branch* has a perfectly plausible height — buffering it would commit state from
/// a chain that no longer exists.
///
/// So every reset bumps a generation counter. Work carries the generation it was
/// started under, and completions from an older one are discarded. That makes the
/// guarantee a property of this type rather than something the dispatcher has to
/// achieve by draining its queues perfectly.
#[derive(Debug)]
pub struct OrderedCommitBuffer<T> {
    next_expected: u64,
    pending: BTreeMap<u64, T>,
    capacity: usize,
    generation: u64,
}

impl<T> OrderedCommitBuffer<T> {
    /// Create a buffer expecting `start_height` first.
    ///
    /// `capacity` bounds how many out-of-order blocks may be held. It is a memory
    /// guard, not a correctness one — the fetcher's backpressure should keep the
    /// buffer well below it.
    pub fn new(start_height: u64, capacity: usize) -> Self {
        Self {
            next_expected: start_height,
            pending: BTreeMap::new(),
            capacity: capacity.max(1),
            generation: 0,
        }
    }

    /// The height that must arrive before anything can be released.
    pub fn next_expected(&self) -> u64 {
        self.next_expected
    }

    /// The current generation.
    ///
    /// Workers capture this when they pick up a block and hand it back to
    /// [`OrderedCommitBuffer::complete_from`].
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Completions held but not yet releasable.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Whether the buffer is at capacity.
    pub fn is_full(&self) -> bool {
        self.pending.len() >= self.capacity
    }

    /// Record a completed block from the current generation.
    ///
    /// Convenience for callers that cannot have crossed a reset — tests, and
    /// single-threaded paths. Concurrent workers should use
    /// [`OrderedCommitBuffer::complete_from`].
    pub fn complete(&mut self, height: u64, value: T) -> Vec<(u64, T)> {
        self.complete_from(self.generation, height, value)
    }

    /// Record a completed block and return everything now releasable, in ascending
    /// height order.
    ///
    /// Returns empty when the block cannot commit yet — either its predecessors
    /// have not arrived, or it is stale (already committed, or from a generation
    /// abandoned by a reorg).
    pub fn complete_from(&mut self, generation: u64, height: u64, value: T) -> Vec<(u64, T)> {
        if generation != self.generation {
            // From a branch that has been rewound away. Its writes were undone;
            // committing them now would resurrect state that no chain contains.
            return Vec::new();
        }
        if height < self.next_expected {
            // Already committed — a duplicate delivery.
            return Vec::new();
        }

        self.pending.insert(height, value);

        let mut released = Vec::new();
        while let Some(value) = self.pending.remove(&self.next_expected) {
            released.push((self.next_expected, value));
            self.next_expected += 1;
        }
        released
    }

    /// Discard everything buffered, resume at `height`, and start a new
    /// generation.
    ///
    /// Called on a reorg. Bumping the generation is what makes in-flight work from
    /// the abandoned branch inert even if it completes after this returns.
    pub fn reset_to(&mut self, height: u64) -> usize {
        let discarded = self.pending.len();
        self.pending.clear();
        self.next_expected = height;
        self.generation += 1;
        discarded
    }

    /// Heights currently buffered, ascending. Diagnostic.
    pub fn pending_heights(&self) -> Vec<u64> {
        self.pending.keys().copied().collect()
    }

    /// The gap blocking progress: the expected height, and the lowest height
    /// waiting on it.
    ///
    /// `None` when nothing is buffered. Useful when the pipeline stalls — it names
    /// the block that has not come back.
    pub fn stall_gap(&self) -> Option<(u64, u64)> {
        self.pending
            .keys()
            .next()
            .map(|lowest| (self.next_expected, *lowest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_order_completions_pass_straight_through() {
        let mut buf = OrderedCommitBuffer::new(100, 16);
        assert_eq!(buf.complete(100, "a"), vec![(100, "a")]);
        assert_eq!(buf.complete(101, "b"), vec![(101, "b")]);
        assert_eq!(buf.pending_count(), 0);
    }

    #[test]
    fn out_of_order_completions_are_held_until_the_gap_fills() {
        let mut buf = OrderedCommitBuffer::new(100, 16);

        // 103 and 101 arrive first; neither may commit.
        assert!(buf.complete(103, "d").is_empty());
        assert!(buf.complete(101, "b").is_empty());
        assert_eq!(buf.pending_count(), 2);

        // 100 arrives and unblocks 101 with it.
        assert_eq!(buf.complete(100, "a"), vec![(100, "a"), (101, "b")]);

        // 102 arrives and releases the 103 that has been waiting.
        assert_eq!(buf.complete(102, "c"), vec![(102, "c"), (103, "d")]);
        assert_eq!(buf.pending_count(), 0);
    }

    #[test]
    fn every_arrival_order_yields_the_same_commit_order() {
        // Guide Milestone 7's acceptance criterion, exhaustively over 4 blocks.
        let heights = [100u64, 101, 102, 103];
        let mut orderings = Vec::new();
        permutations(&heights, &mut Vec::new(), &mut orderings);
        assert_eq!(orderings.len(), 24);

        for arrival in orderings {
            let mut buf = OrderedCommitBuffer::new(100, 16);
            let mut committed = Vec::new();
            for h in &arrival {
                committed.extend(buf.complete(*h, *h).into_iter().map(|(h, _)| h));
            }
            assert_eq!(
                committed,
                vec![100, 101, 102, 103],
                "arrival order {arrival:?} produced a different commit order"
            );
            assert_eq!(buf.pending_count(), 0);
        }
    }

    fn permutations(remaining: &[u64], acc: &mut Vec<u64>, out: &mut Vec<Vec<u64>>) {
        if remaining.is_empty() {
            out.push(acc.clone());
            return;
        }
        for (i, h) in remaining.iter().enumerate() {
            let mut rest = remaining.to_vec();
            rest.remove(i);
            acc.push(*h);
            permutations(&rest, acc, out);
            acc.pop();
        }
    }

    #[test]
    fn already_committed_heights_are_dropped() {
        let mut buf = OrderedCommitBuffer::new(100, 16);
        buf.complete(100, "a");
        // A straggler from before the cursor must not commit twice.
        assert!(buf.complete(100, "a-again").is_empty());
        assert!(buf.complete(99, "old").is_empty());
        assert_eq!(buf.next_expected(), 101);
    }

    #[test]
    fn reset_discards_the_abandoned_branch() {
        let mut buf = OrderedCommitBuffer::new(100, 16);
        let before = buf.generation();
        buf.complete(102, "c");
        buf.complete(103, "d");
        assert_eq!(buf.pending_count(), 2);

        // Reorg: rewind to 99, resume at 100.
        assert_eq!(buf.reset_to(100), 2);
        assert_eq!(buf.pending_count(), 0);
        assert_eq!(buf.next_expected(), 100);
        assert_ne!(
            buf.generation(),
            before,
            "a reset must start a new generation"
        );

        // Work started before the reset is inert even though its height is
        // plausible on the new branch.
        assert!(buf.complete_from(before, 102, "stale").is_empty());
        assert_eq!(buf.pending_count(), 0);
    }

    #[test]
    fn a_stale_block_cannot_commit_on_the_new_branch() {
        // The hazard height alone cannot catch: after rewinding to 99 and
        // resuming at 100, block 101 from the *old* branch is still in flight.
        let mut buf = OrderedCommitBuffer::new(100, 16);
        let old = buf.generation();
        buf.reset_to(100);

        // The stale 101 arrives first and must be ignored...
        assert!(buf.complete_from(old, 101, "old-101").is_empty());
        // ...so committing 100 releases only 100, not the abandoned 101.
        assert_eq!(buf.complete(100, "new-100"), vec![(100, "new-100")]);

        // The canonical 101 then commits normally.
        assert_eq!(buf.complete(101, "new-101"), vec![(101, "new-101")]);
    }

    #[test]
    fn work_from_the_current_generation_still_commits_after_a_reset() {
        // A reset must not wedge the buffer: work started after it is fine.
        let mut buf = OrderedCommitBuffer::new(100, 16);
        buf.reset_to(200);
        let current = buf.generation();
        assert_eq!(buf.complete_from(current, 200, "a"), vec![(200, "a")]);
    }

    #[test]
    fn capacity_is_reported_but_never_silently_drops() {
        let mut buf = OrderedCommitBuffer::new(100, 2);
        buf.complete(102, "c");
        buf.complete(103, "d");
        assert!(buf.is_full());
        // Over-capacity is the fetcher's problem to avoid; the buffer must not
        // lose a block, because a dropped block would be a permanent gap.
        buf.complete(104, "e");
        assert_eq!(buf.pending_count(), 3);
    }

    #[test]
    fn stall_gap_names_the_missing_block() {
        let mut buf = OrderedCommitBuffer::new(100, 16);
        assert_eq!(buf.stall_gap(), None);

        buf.complete(105, "x");
        // Waiting on 100; the lowest thing buffered is 105.
        assert_eq!(buf.stall_gap(), Some((100, 105)));
        assert_eq!(buf.pending_heights(), vec![105]);
    }
}
