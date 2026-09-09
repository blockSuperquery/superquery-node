//! Indexing one block: filter, run the matching handlers, collect the writes.
//!
//! The lifecycle, in order, because the order is what makes results deterministic:
//!
//! ```text
//! resolve active data sources for the height   <- project::BlockHeightMap
//!   -> filter block/tx/event inputs            <- ChainAdapter::event_matches
//!   -> run matching handlers in manifest order <- superquery-runtime
//!   -> collect entity operations
//!   -> hand them to the dispatcher for ordered commit
//! ```
//!
//! Handlers run in a fixed order and writes are collected rather than applied
//! directly, so two runs over the same blocks produce the same database — which is
//! what makes replay after a reorg, and Proof of Index later, meaningful.
//!
//! Upstream analogue: `node-core/src/indexer/indexer.manager.ts`.

use superquery_chain_api::Header;
use superquery_store::Operation;

use crate::error::Result;
use crate::project::DynamicDataSource;

/// The result of indexing one block.
#[derive(Debug, Default)]
pub struct ProcessedBlock {
    /// Entity mutations the handlers produced, in execution order.
    pub operations: Vec<Operation>,
    /// Data sources created while indexing this block.
    pub dynamic_sources: Vec<DynamicDataSource>,
    /// Handler invocations that ran. Reported as a metric.
    pub handlers_run: u32,
}

impl ProcessedBlock {
    /// Whether the block changed anything.
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty() && self.dynamic_sources.is_empty()
    }
}

/// Runs a project's handlers against blocks.
#[async_trait::async_trait]
pub trait IndexerManager: Send + Sync {
    /// The block type this manager indexes.
    type Block: Send + Sync;

    /// Index one block, returning the writes it produced.
    ///
    /// Must not touch the database: the dispatcher decides when the returned
    /// operations are committed, and in what order (guide Milestone 7).
    async fn index_block(&self, header: &Header, block: &Self::Block) -> Result<ProcessedBlock>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untouched_block_reports_empty() {
        assert!(ProcessedBlock::default().is_empty());
    }

    #[test]
    fn a_block_creating_only_a_datasource_is_not_empty() {
        // A factory event may write no entities but still change what is indexed
        // from here on, so it must still be committed.
        let processed = ProcessedBlock {
            dynamic_sources: vec![DynamicDataSource {
                template: "Pair".into(),
                start_height: 100,
                parameters: serde_json::Value::Null,
            }],
            ..Default::default()
        };
        assert!(!processed.is_empty());
    }
}
