//! End-to-end ordering: fetch planning through to commit order.
//!
//! Guide Milestone 7's acceptance — *varying fetch completion order yields the
//! same final DB state* — is a property of two crates working together, so it is
//! tested here rather than inside either one.
//!
//! The dispatcher's own tests cover the reorder buffer exhaustively. These check
//! the pieces compose: what the scheduler plans is what the buffer can commit.

use superquery_core::fetch::{Backpressure, RangePlan};
use superquery_dispatcher::OrderedCommitBuffer;

/// Push heights through the reorder buffer in `arrival` order and return the
/// order they were released for commit.
fn commit_order(start: u64, arrival: &[u64]) -> Vec<u64> {
    let mut buffer = OrderedCommitBuffer::new(start, 64);
    let mut committed = Vec::new();
    for &height in arrival {
        committed.extend(buffer.complete(height, height).into_iter().map(|(h, _)| h));
    }
    committed
}

#[test]
fn a_planned_batch_commits_in_height_order_however_it_arrives() {
    let plan = RangePlan::new();
    let batch = plan
        .next_batch(100, 1_000, 8, None)
        .expect("a batch should be available");
    let heights = plan.heights_in(&batch);
    assert_eq!(heights, (100..=107).collect::<Vec<_>>());

    // Sequential arrival.
    assert_eq!(commit_order(100, &heights), heights);

    // Reversed arrival — the worst case for a reorder buffer.
    let mut reversed = heights.clone();
    reversed.reverse();
    assert_eq!(commit_order(100, &reversed), heights);

    // An interleaved order.
    let shuffled = vec![103, 100, 107, 101, 105, 102, 106, 104];
    assert_eq!(commit_order(100, &shuffled), heights);
}

#[test]
fn bypassed_heights_are_never_planned_and_never_stall_the_commit_buffer() {
    // A project skipping a known-bad span must still commit contiguously: the
    // buffer expects the next height it is told about, so a planned gap would
    // deadlock if the two disagreed.
    let plan = RangePlan::new().bypass(103, 104);
    let batch = plan.next_batch(100, 1_000, 8, None).unwrap();

    // Planning stops before the bypass rather than emitting a gap.
    assert_eq!(batch.start, 100);
    assert_eq!(batch.end, 102);

    let heights = plan.heights_in(&batch);
    assert_eq!(heights, vec![100, 101, 102]);
    assert_eq!(commit_order(100, &heights), heights);

    // The next batch resumes past the bypassed span.
    let next = plan.next_batch(103, 1_000, 8, None).unwrap();
    assert_eq!(next.start, 105);
}

#[test]
fn backpressure_never_plans_more_than_the_buffer_can_hold() {
    let capacity = 16;
    let bp = Backpressure::new(8, capacity);
    let plan = RangePlan::new();

    let mut buffer = OrderedCommitBuffer::new(100, capacity as usize);
    let mut next_height = 100;
    let mut in_flight = 0u32;

    // Fetch without ever committing: the pessimal case for memory growth.
    for _ in 0..20 {
        let budget = bp.clamp_batch(100, in_flight, buffer.pending_count() as u32);
        if budget == 0 {
            break;
        }
        let Some(batch) = plan.next_batch(next_height, 10_000, budget, None) else {
            break;
        };
        for height in plan.heights_in(&batch) {
            // Deliberately never supply the height the buffer is waiting for, so
            // nothing is ever released.
            if height != 100 {
                buffer.complete(height, height);
            } else {
                in_flight += 1;
            }
        }
        next_height = batch.end + 1;
    }

    assert!(
        buffer.pending_count() <= capacity as usize,
        "backpressure let the buffer exceed its capacity: {} > {capacity}",
        buffer.pending_count()
    );
    // And the stall is diagnosable: it names the block everything waits on.
    assert_eq!(buffer.stall_gap().map(|(expected, _)| expected), Some(100));
}

#[test]
fn a_reorg_reset_discards_planned_work_from_the_abandoned_branch() {
    let mut buffer = OrderedCommitBuffer::new(100, 64);
    let abandoned = buffer.generation();
    buffer.complete(101, 101);
    buffer.complete(102, 102);

    // Rewind to 99: everything above it is from the abandoned branch.
    assert_eq!(buffer.reset_to(100), 2);

    // A block still in flight when the reorg happened lands afterwards. Its
    // height is plausible on the new branch, so only the generation distinguishes
    // it — and it must not commit.
    assert!(buffer.complete_from(abandoned, 101, 101).is_empty());

    // The canonical branch commits from the resume point, uncontaminated.
    assert_eq!(
        buffer
            .complete(100, 100)
            .into_iter()
            .map(|(h, _)| h)
            .collect::<Vec<_>>(),
        vec![100]
    );
}
