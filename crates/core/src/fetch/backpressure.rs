//! Admission control for the fetch loop.
//!
//! Guide Milestone 5's acceptance is "catch up 1,000 test blocks without unbounded
//! RAM growth". That is a statement about backpressure: fetching is far faster
//! than mapping execution, so without a limiter the fetcher will happily pull
//! thousands of decoded blocks into memory while the dispatcher works through the
//! first few.
//!
//! Two independent limits, because they bound different things:
//!
//! - **in-flight requests** bound concurrent load on the RPC endpoint;
//! - **queue depth** bounds memory held by blocks fetched but not yet committed.
//!
//! Either can stop the fetcher. This type answers "how many more blocks may I ask
//! for right now?", and the scheduler simply obeys it.

/// Limits governing how far ahead the fetcher may run.
#[derive(Debug, Clone, Copy)]
pub struct Backpressure {
    /// Maximum concurrent block fetches.
    pub max_in_flight: u32,
    /// Maximum blocks fetched but not yet committed.
    pub queue_capacity: u32,
}

impl Backpressure {
    /// Build from the node's configured limits.
    pub fn new(max_in_flight: u32, queue_capacity: u32) -> Self {
        Self {
            max_in_flight: max_in_flight.max(1),
            queue_capacity: queue_capacity.max(1),
        }
    }

    /// How many more blocks may be requested, given current occupancy.
    ///
    /// Returns 0 when either limit is reached — the caller should wait rather
    /// than spin.
    pub fn available(&self, in_flight: u32, queued: u32) -> u32 {
        let by_flight = self.max_in_flight.saturating_sub(in_flight);
        // Queued and in-flight blocks both end up in memory, so they share the
        // queue budget. Counting only `queued` would let in-flight blocks
        // overshoot the capacity on arrival.
        let by_queue = self
            .queue_capacity
            .saturating_sub(queued.saturating_add(in_flight));
        by_flight.min(by_queue)
    }

    /// Whether the fetcher must wait.
    pub fn should_pause(&self, in_flight: u32, queued: u32) -> bool {
        self.available(in_flight, queued) == 0
    }

    /// The batch size to actually use, capped by what is available.
    pub fn clamp_batch(&self, desired: u32, in_flight: u32, queued: u32) -> u32 {
        desired.min(self.available(in_flight, queued))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_allows_a_full_batch() {
        let bp = Backpressure::new(8, 200);
        assert_eq!(bp.available(0, 0), 8);
        assert!(!bp.should_pause(0, 0));
    }

    #[test]
    fn in_flight_requests_are_the_binding_limit_when_the_queue_is_empty() {
        let bp = Backpressure::new(8, 200);
        assert_eq!(bp.available(5, 0), 3);
        assert_eq!(bp.available(8, 0), 0);
        assert!(bp.should_pause(8, 0));
    }

    #[test]
    fn a_full_queue_stops_fetching_even_with_spare_concurrency() {
        let bp = Backpressure::new(8, 100);
        // No requests in flight, but the queue is full: memory, not the endpoint,
        // is the constraint.
        assert_eq!(bp.available(0, 100), 0);
        assert!(bp.should_pause(0, 100));
    }

    #[test]
    fn in_flight_blocks_count_against_the_queue_budget() {
        let bp = Backpressure::new(50, 100);
        // 90 queued + 10 arriving already fills the queue; asking for more would
        // overshoot capacity the moment those 10 land.
        assert_eq!(bp.available(10, 90), 0);
        assert_eq!(bp.available(10, 80), 10);
    }

    #[test]
    fn over_occupancy_saturates_instead_of_underflowing() {
        let bp = Backpressure::new(8, 100);
        assert_eq!(bp.available(20, 200), 0);
    }

    #[test]
    fn batches_are_clamped_to_what_fits() {
        let bp = Backpressure::new(8, 100);
        assert_eq!(bp.clamp_batch(100, 0, 0), 8);
        assert_eq!(bp.clamp_batch(2, 0, 0), 2);
        assert_eq!(bp.clamp_batch(100, 0, 95), 5);
    }

    #[test]
    fn zero_limits_are_raised_to_one_so_progress_is_always_possible() {
        let bp = Backpressure::new(0, 0);
        assert_eq!(bp.max_in_flight, 1);
        assert_eq!(bp.queue_capacity, 1);
        assert_eq!(bp.available(0, 0), 1);
    }
}
