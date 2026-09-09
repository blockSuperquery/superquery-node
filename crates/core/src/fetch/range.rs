//! Turning "index from A to B" into the batches the scheduler actually fetches.
//!
//! Three constraints have to hold simultaneously, and they interact:
//!
//! - never fetch past the safe head (the finalized height, or the chain head when
//!   indexing unfinalized blocks);
//! - never exceed the configured batch size;
//! - skip heights the project has told us to bypass.
//!
//! Getting this wrong is expensive in a way that is hard to see: an off-by-one at
//! the head means re-fetching the same block forever, and a bypass range applied
//! after batching means fetching blocks only to discard them. So the maths lives
//! here, pure and tested, rather than inline in the scheduler loop.
//!
//! Upstream analogue: the range calculation in
//! `node-core/src/indexer/fetch.service.ts`.

use std::ops::RangeInclusive;

/// A contiguous span of heights, inclusive at both ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRange {
    /// First height in the span.
    pub start: u64,
    /// Last height in the span.
    pub end: u64,
}

impl BlockRange {
    /// Build a span. Returns `None` when `end < start`.
    pub fn new(start: u64, end: u64) -> Option<Self> {
        (start <= end).then_some(Self { start, end })
    }

    /// Number of heights in the span.
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }

    /// Always `false` — a `BlockRange` is inclusive, so it holds at least one
    /// height. Present because clippy asks for it alongside `len`.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Whether `height` falls inside the span.
    pub fn contains(&self, height: u64) -> bool {
        self.start <= height && height <= self.end
    }

    /// The span as a Rust range.
    pub fn heights(&self) -> RangeInclusive<u64> {
        self.start..=self.end
    }
}

/// What to index and what to skip.
#[derive(Debug, Clone, Default)]
pub struct RangePlan {
    /// Heights to skip entirely, as inclusive spans.
    pub bypass: Vec<BlockRange>,
}

impl RangePlan {
    /// A plan with no bypasses.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a bypass span.
    pub fn bypass(mut self, start: u64, end: u64) -> Self {
        if let Some(r) = BlockRange::new(start, end) {
            self.bypass.push(r);
        }
        self
    }

    /// Whether `height` should be skipped.
    pub fn is_bypassed(&self, height: u64) -> bool {
        self.bypass.iter().any(|r| r.contains(height))
    }

    /// The next batch to fetch, given where we are and how far it is safe to go.
    ///
    /// Returns `None` when there is nothing to do — already at the safe head, or
    /// past the configured end height.
    ///
    /// `next_height` is the first height *not yet* indexed.
    pub fn next_batch(
        &self,
        next_height: u64,
        safe_head: u64,
        batch_size: u32,
        end_height: Option<u64>,
    ) -> Option<BlockRange> {
        let ceiling = match end_height {
            Some(end) => safe_head.min(end),
            None => safe_head,
        };
        if next_height > ceiling {
            return None;
        }

        // Skip forward over any bypassed span we are sitting on, so a bypass at
        // the cursor cannot stall the scheduler.
        let start = self.advance_past_bypass(next_height, ceiling)?;

        let span_end = start.saturating_add(batch_size as u64 - 1).min(ceiling);

        // Stop the batch before the next bypassed span rather than fetching
        // blocks we would immediately drop.
        let end = self
            .bypass
            .iter()
            .filter(|r| r.start > start && r.start <= span_end)
            .map(|r| r.start - 1)
            .min()
            .unwrap_or(span_end);

        BlockRange::new(start, end)
    }

    /// Move `height` forward past any bypassed span covering it.
    fn advance_past_bypass(&self, mut height: u64, ceiling: u64) -> Option<u64> {
        // Bypass spans may be adjacent, so loop until a height survives. Bounded
        // by the span count: each pass leaves at least one span behind.
        for _ in 0..=self.bypass.len() {
            match self.bypass.iter().find(|r| r.contains(height)) {
                Some(r) => height = r.end.checked_add(1)?,
                None => return (height <= ceiling).then_some(height),
            }
        }
        None
    }

    /// Expand a batch into the heights actually worth fetching.
    pub fn heights_in(&self, range: &BlockRange) -> Vec<u64> {
        range.heights().filter(|h| !self.is_bypassed(*h)).collect()
    }
}

/// The highest height it is safe to index right now.
///
/// With `unfinalized` set the node indexes up to the chain head and accepts that
/// reorgs must be handled. Without it, indexing stops at the finalized height —
/// falling back to a depth-based estimate when the chain reports finality of 0,
/// which some endpoints do before they have synced.
pub fn safe_head(latest: u64, finalized: u64, unfinalized: bool, confirmations: u64) -> u64 {
    if unfinalized {
        latest
    } else if finalized > 0 {
        finalized.min(latest)
    } else {
        latest.saturating_sub(confirmations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_are_capped_by_size() {
        let plan = RangePlan::new();
        let b = plan.next_batch(100, 1_000, 10, None).unwrap();
        assert_eq!(
            b,
            BlockRange {
                start: 100,
                end: 109
            }
        );
        assert_eq!(b.len(), 10);
    }

    #[test]
    fn batches_are_capped_by_the_safe_head() {
        let plan = RangePlan::new();
        // Only 3 blocks available, batch size 100.
        let b = plan.next_batch(100, 102, 100, None).unwrap();
        assert_eq!(
            b,
            BlockRange {
                start: 100,
                end: 102
            }
        );
    }

    #[test]
    fn nothing_to_do_at_the_head() {
        let plan = RangePlan::new();
        assert_eq!(plan.next_batch(101, 100, 10, None), None);
        // Exactly at the head there is still one block to fetch.
        assert_eq!(
            plan.next_batch(100, 100, 10, None),
            Some(BlockRange {
                start: 100,
                end: 100
            })
        );
    }

    #[test]
    fn end_height_caps_the_run() {
        let plan = RangePlan::new();
        let b = plan.next_batch(100, 1_000, 100, Some(150)).unwrap();
        assert_eq!(b.end, 150);
        // Past the end height, there is nothing left.
        assert_eq!(plan.next_batch(151, 1_000, 100, Some(150)), None);
    }

    #[test]
    fn a_batch_stops_before_a_bypassed_span() {
        let plan = RangePlan::new().bypass(105, 110);
        let b = plan.next_batch(100, 1_000, 50, None).unwrap();
        // Would have run to 149; stops at 104 instead of fetching 105..110.
        assert_eq!(
            b,
            BlockRange {
                start: 100,
                end: 104
            }
        );
    }

    #[test]
    fn a_bypass_at_the_cursor_is_skipped_not_stalled() {
        let plan = RangePlan::new().bypass(100, 110);
        let b = plan.next_batch(100, 1_000, 10, None).unwrap();
        assert_eq!(
            b,
            BlockRange {
                start: 111,
                end: 120
            }
        );
    }

    #[test]
    fn adjacent_bypasses_are_skipped_together() {
        // Two touching spans must not leave the cursor stuck between them.
        let plan = RangePlan::new().bypass(100, 105).bypass(106, 110);
        let b = plan.next_batch(100, 1_000, 5, None).unwrap();
        assert_eq!(b.start, 111);
    }

    #[test]
    fn a_bypass_covering_everything_left_yields_nothing() {
        let plan = RangePlan::new().bypass(100, 200);
        assert_eq!(plan.next_batch(100, 150, 10, None), None);
    }

    #[test]
    fn heights_in_drops_bypassed_blocks() {
        let plan = RangePlan::new().bypass(103, 104);
        let range = BlockRange::new(100, 106).unwrap();
        assert_eq!(plan.heights_in(&range), vec![100, 101, 102, 105, 106]);
    }

    #[test]
    fn safe_head_respects_finality_mode() {
        // Unfinalized indexing goes to the head.
        assert_eq!(safe_head(1_000, 900, true, 200), 1_000);
        // Otherwise it stops at the finalized height.
        assert_eq!(safe_head(1_000, 900, false, 200), 900);
        // A chain reporting no finality falls back to a depth estimate.
        assert_eq!(safe_head(1_000, 0, false, 200), 800);
        // Finality is never allowed to exceed the head.
        assert_eq!(safe_head(500, 900, false, 200), 500);
        // Depth deeper than the chain saturates at genesis.
        assert_eq!(safe_head(50, 0, false, 200), 0);
    }

    #[test]
    fn batch_size_of_one_advances_by_one() {
        let plan = RangePlan::new();
        let b = plan.next_batch(100, 1_000, 1, None).unwrap();
        assert_eq!(
            b,
            BlockRange {
                start: 100,
                end: 100
            }
        );
    }

    #[test]
    fn ranges_reject_inverted_bounds() {
        assert_eq!(BlockRange::new(10, 9), None);
        assert_eq!(BlockRange::new(10, 10).unwrap().len(), 1);
    }
}
