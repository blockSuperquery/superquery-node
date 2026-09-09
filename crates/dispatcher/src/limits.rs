//! Bounds on the dispatch pipeline.
//!
//! Every limit here exists to stop one specific unbounded growth. They are grouped
//! so a deployment can be tuned without hunting for constants.

/// Pipeline limits.
#[derive(Debug, Clone, Copy)]
pub struct DispatchLimits {
    /// Blocks that may sit fetched and awaiting processing.
    pub queue_capacity: usize,
    /// Concurrent mapping workers.
    pub workers: usize,
    /// Out-of-order completions the reorder buffer may hold.
    pub reorder_capacity: usize,
}

impl DispatchLimits {
    /// Build limits, forcing each to at least 1 so the pipeline can always make
    /// progress.
    pub fn new(queue_capacity: usize, workers: usize, reorder_capacity: usize) -> Self {
        Self {
            queue_capacity: queue_capacity.max(1),
            workers: workers.max(1),
            reorder_capacity: reorder_capacity.max(1),
        }
    }

    /// Sensible defaults derived from a batch size and worker count.
    ///
    /// The reorder buffer is sized to the worker count because that is the most
    /// blocks that can be in flight through mapping at once — a smaller buffer
    /// would report full during normal operation.
    pub fn from_config(batch_size: u32, workers: usize) -> Self {
        Self::new(batch_size as usize * 2, workers, workers * 2)
    }
}

impl Default for DispatchLimits {
    fn default() -> Self {
        Self::new(200, 4, 8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_limits_are_raised_so_the_pipeline_can_progress() {
        let l = DispatchLimits::new(0, 0, 0);
        assert_eq!((l.queue_capacity, l.workers, l.reorder_capacity), (1, 1, 1));
    }

    #[test]
    fn derived_limits_leave_room_for_every_worker() {
        let l = DispatchLimits::from_config(100, 8);
        assert_eq!(l.queue_capacity, 200);
        assert_eq!(l.workers, 8);
        // At least one slot per in-flight worker, or the buffer reports full
        // during ordinary operation.
        assert!(l.reorder_capacity >= l.workers);
    }
}
