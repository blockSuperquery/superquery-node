//! Metrics the node reports.
//!
//! Guide Milestone 13 lists what has to be observable. Defining the names here
//! rather than at each call site keeps a metric from being renamed by accident and
//! quietly breaking a dashboard.
//!
//! The Prometheus registry and the `/metrics` endpoint are wired up in the binary.

/// Metric names, all prefixed `superquery_`.
pub mod names {
    /// Highest fully-committed block height.
    pub const INDEXED_HEIGHT: &str = "superquery_indexed_height";
    /// Chain head height.
    pub const TARGET_HEIGHT: &str = "superquery_target_height";
    /// Highest height known final.
    pub const FINALIZED_HEIGHT: &str = "superquery_finalized_height";
    /// Blocks committed per second.
    pub const BLOCKS_PER_SECOND: &str = "superquery_blocks_per_second";
    /// Handler execution time.
    pub const HANDLER_DURATION: &str = "superquery_handler_duration_seconds";
    /// Database commit time.
    pub const COMMIT_DURATION: &str = "superquery_commit_duration_seconds";
    /// RPC requests that failed.
    pub const RPC_ERRORS: &str = "superquery_rpc_errors_total";
    /// Blocks fetched but not yet committed.
    pub const QUEUE_DEPTH: &str = "superquery_queue_depth";
    /// Reorgs handled.
    pub const REORGS: &str = "superquery_reorgs_total";
    /// Blocks discarded by rewinds.
    pub const REORG_DEPTH: &str = "superquery_reorg_blocks_discarded_total";
}

/// A point-in-time snapshot of indexing progress, for `/health` and logs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProgressSnapshot {
    /// Highest committed height.
    pub indexed_height: u64,
    /// Chain head.
    pub target_height: u64,
    /// Highest final height.
    pub finalized_height: u64,
    /// Blocks fetched but not committed.
    pub queue_depth: u32,
}

impl ProgressSnapshot {
    /// Blocks still to index before reaching the head.
    pub fn blocks_behind(&self) -> u64 {
        self.target_height.saturating_sub(self.indexed_height)
    }

    /// Whether the node has caught up to within `tolerance` blocks of the head.
    ///
    /// Drives `/ready`: a node still catching up is healthy but not yet serving
    /// current data.
    pub fn is_caught_up(&self, tolerance: u64) -> bool {
        self.blocks_behind() <= tolerance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_how_far_behind_the_head_it_is() {
        let s = ProgressSnapshot {
            indexed_height: 900,
            target_height: 1_000,
            ..Default::default()
        };
        assert_eq!(s.blocks_behind(), 100);
        assert!(!s.is_caught_up(10));
        assert!(s.is_caught_up(100));
    }

    #[test]
    fn running_ahead_of_a_stale_head_reads_as_caught_up() {
        // The target height can lag behind reality between refreshes; that must
        // not underflow into a huge backlog.
        let s = ProgressSnapshot {
            indexed_height: 1_000,
            target_height: 900,
            ..Default::default()
        };
        assert_eq!(s.blocks_behind(), 0);
        assert!(s.is_caught_up(0));
    }
}
