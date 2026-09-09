//! The EVM [`ChainAdapter`] implementation.
//!
//! Guide Milestone 4, grant issue #2. Built on
//! [alloy](https://github.com/alloy-rs/alloy) — the crate that must never appear
//! outside this directory.
//!
//! Upstream analogue: `subql-ethereum/packages/node/src/blockchain.service.ts`.

use std::sync::Arc;

use async_trait::async_trait;
use superquery_chain_api::{
    ChainAdapter, ChainError, Filter, GenericBlock, HandlerKind, Header, Result,
};

use crate::block::{EvmBlock, EvmEvent};
use crate::filter::{EvmLogFilter, EvmTransactionFilter};

/// Connection settings for an EVM endpoint.
#[derive(Debug, Clone)]
pub struct EvmAdapterConfig {
    /// RPC endpoints. The first is primary; the rest are failover.
    pub endpoints: Vec<String>,
    /// Expected chain id. Verified at startup.
    pub chain_id: String,
    /// Per-request timeout, in seconds.
    pub timeout_secs: u64,
    /// Attempts before a request is failed.
    pub max_retries: u32,
    /// Depth treated as final. Only consulted when the endpoint reports no
    /// finalized block, which some chains and some sync states do.
    pub finality_confirmations: u64,
}

impl Default for EvmAdapterConfig {
    fn default() -> Self {
        Self {
            endpoints: Vec::new(),
            chain_id: "1".to_string(),
            timeout_secs: 30,
            max_retries: 5,
            finality_confirmations: 200,
        }
    }
}

/// Indexes an EVM chain.
pub struct EvmAdapter {
    config: EvmAdapterConfig,
    chain_name: String,
}

impl EvmAdapter {
    /// Build an adapter. Does not connect; call
    /// [`ChainAdapter::validate_network`] at startup to prove reachability and
    /// chain identity.
    pub fn new(config: EvmAdapterConfig) -> Self {
        let chain_name = well_known_chain_name(&config.chain_id)
            .unwrap_or("evm")
            .to_string();
        Self { config, chain_name }
    }

    /// The adapter's configuration.
    pub fn config(&self) -> &EvmAdapterConfig {
        &self.config
    }
}

/// Whether an EVM event matches a manifest filter.
///
/// Split out of the trait method so it can be exercised without an adapter or a
/// live endpoint — this is the hot path that runs on every log of every block.
pub fn event_matches_filter(event: &EvmEvent, filter: &Filter) -> bool {
    match filter.handler {
        HandlerKind::Event => match filter.parse_params::<EvmLogFilter>() {
            Ok(log_filter) => log_filter.matches(&event.log.view()),
            // A malformed filter is rejected when the manifest loads. Reaching
            // here means a filter changed underneath us; matching nothing is the
            // safe answer, and it is loud in the data rather than silently
            // over-broad.
            Err(_) => false,
        },
        HandlerKind::Transaction => match (
            &event.transaction,
            filter.parse_params::<EvmTransactionFilter>(),
        ) {
            (Some(tx), Ok(tx_filter)) => tx_filter.matches(&tx.view()),
            _ => false,
        },
        // Block handlers run per block, not per event.
        HandlerKind::Block => false,
    }
}

/// Map a well-known chain id to a display name.
fn well_known_chain_name(chain_id: &str) -> Option<&'static str> {
    Some(match chain_id {
        "1" => "ethereum",
        "10" => "optimism",
        "56" => "bsc",
        "137" => "polygon",
        "8453" => "base",
        "42161" => "arbitrum",
        "43114" => "avalanche",
        "11155111" => "sepolia",
        _ => return None,
    })
}

#[async_trait]
impl ChainAdapter for EvmAdapter {
    type Block = EvmBlock;
    type Event = EvmEvent;
    type FetchedBlock = GenericBlock<EvmBlock>;

    fn network_id(&self) -> &str {
        &self.config.chain_id
    }

    fn chain_name(&self) -> &str {
        &self.chain_name
    }

    async fn validate_network(&self, _expected: &str) -> Result<()> {
        // Milestone: eth_chainId round-trip. Guide Milestone 4, task plan phase B2.
        Err(ChainError::Other(
            "EVM RPC not yet implemented: guide Milestone 4, task plan phase B2".into(),
        ))
    }

    async fn latest_height(&self) -> Result<u64> {
        Err(ChainError::Other(
            "EVM RPC not yet implemented: guide Milestone 4, task plan phase B2".into(),
        ))
    }

    async fn finalized_height(&self) -> Result<u64> {
        Err(ChainError::Other(
            "EVM RPC not yet implemented: guide Milestone 4, task plan phase B2".into(),
        ))
    }

    async fn fetch_block(&self, _height: u64) -> Result<Self::FetchedBlock> {
        Err(ChainError::Other(
            "EVM RPC not yet implemented: guide Milestone 4, task plan phase B2".into(),
        ))
    }

    async fn header_at(&self, _height: u64) -> Result<Header> {
        Err(ChainError::Other(
            "EVM RPC not yet implemented: guide Milestone 4, task plan phase B2".into(),
        ))
    }

    async fn decode_events(&self, block: &Self::Block) -> Result<Vec<Self::Event>> {
        // Pure decoding over an already-fetched block, so this works today.
        // Transactions are indexed once rather than scanned per log: a block with
        // 200 transactions and 2,000 logs would otherwise cost 400,000
        // comparisons.
        let by_index: std::collections::HashMap<u64, &crate::block::EvmTransaction> = block
            .transactions
            .iter()
            .map(|tx| (tx.transaction_index, tx))
            .collect();

        Ok(block
            .logs
            .iter()
            .map(|log| EvmEvent {
                transaction: by_index.get(&log.transaction_index).map(|tx| (*tx).clone()),
                log: log.clone(),
                block_number: block.number,
            })
            .collect())
    }

    fn event_matches(&self, event: &Self::Event, filter: &Filter) -> bool {
        event_matches_filter(event, filter)
    }

    fn block_interval_ms(&self) -> u64 {
        match self.config.chain_id.as_str() {
            "1" => 12_000,
            "10" | "8453" => 2_000,
            "56" => 3_000,
            "137" => 2_000,
            "42161" => 250,
            _ => 12_000,
        }
    }
}

/// Shorthand for an adapter behind an `Arc`, which is how the engine holds it.
pub type SharedEvmAdapter = Arc<EvmAdapter>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{EvmLog, EvmTransaction};

    const TRANSFER_SIG: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

    fn adapter() -> EvmAdapter {
        EvmAdapter::new(EvmAdapterConfig {
            endpoints: vec!["https://eth.example".into()],
            chain_id: "1".into(),
            ..Default::default()
        })
    }

    fn block_with(logs: Vec<EvmLog>, transactions: Vec<EvmTransaction>) -> EvmBlock {
        EvmBlock {
            number: 100,
            hash: "0xaaa".into(),
            parent_hash: "0xbbb".into(),
            timestamp: 1_692_100_800,
            transactions,
            logs,
        }
    }

    fn log(log_index: u64, tx_index: u64) -> EvmLog {
        EvmLog {
            address: "0xusdc".into(),
            topics: vec![TRANSFER_SIG.into()],
            data: "0x".into(),
            log_index,
            transaction_hash: format!("0xtx{tx_index}"),
            transaction_index: tx_index,
        }
    }

    fn tx(index: u64) -> EvmTransaction {
        EvmTransaction {
            hash: format!("0xtx{index}"),
            transaction_index: index,
            from: "0xalice".into(),
            to: Some("0xusdc".into()),
            value: "0".into(),
            input: "0xa9059cbb".into(),
        }
    }

    #[test]
    fn chain_ids_resolve_to_names_and_block_times() {
        assert_eq!(adapter().chain_name(), "ethereum");
        assert_eq!(adapter().block_interval_ms(), 12_000);

        let base = EvmAdapter::new(EvmAdapterConfig {
            chain_id: "8453".into(),
            ..Default::default()
        });
        assert_eq!(base.chain_name(), "base");
        assert_eq!(base.block_interval_ms(), 2_000);

        // An unknown chain still works, just without a friendly name.
        let unknown = EvmAdapter::new(EvmAdapterConfig {
            chain_id: "999999".into(),
            ..Default::default()
        });
        assert_eq!(unknown.chain_name(), "evm");
        assert_eq!(unknown.network_id(), "999999");
    }

    #[tokio::test]
    async fn decoding_pairs_each_log_with_its_transaction() {
        let block = block_with(vec![log(0, 0), log(1, 1)], vec![tx(0), tx(1)]);
        let events = adapter().decode_events(&block).await.unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].transaction.as_ref().unwrap().hash, "0xtx0");
        assert_eq!(events[1].transaction.as_ref().unwrap().hash, "0xtx1");
        assert_eq!(events[0].block_number, 100);
    }

    #[tokio::test]
    async fn a_log_without_its_transaction_still_decodes() {
        // Some endpoints return blocks with transaction hashes only.
        let block = block_with(vec![log(0, 7)], vec![]);
        let events = adapter().decode_events(&block).await.unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].transaction.is_none());
    }

    #[tokio::test]
    async fn event_filtering_selects_only_matching_logs() {
        let block = block_with(vec![log(0, 0), log(1, 1)], vec![tx(0), tx(1)]);
        let mut events = adapter().decode_events(&block).await.unwrap();
        // Point the second log at a different contract.
        events[1].log.address = "0xdai".into();

        let filter = Filter::new(HandlerKind::Event).with_params(serde_json::json!({
            "address": "0xusdc",
            "topics": [TRANSFER_SIG]
        }));

        let matched: Vec<_> = events
            .iter()
            .filter(|e| adapter().event_matches(e, &filter))
            .collect();
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].log.address, "0xusdc");
    }

    #[test]
    fn block_handlers_never_match_individual_events() {
        let event = EvmEvent {
            log: log(0, 0),
            transaction: Some(tx(0)),
            block_number: 100,
        };
        let block_filter = Filter::new(HandlerKind::Block);
        assert!(!event_matches_filter(&event, &block_filter));
    }

    #[test]
    fn a_malformed_filter_matches_nothing_rather_than_everything() {
        let event = EvmEvent {
            log: log(0, 0),
            transaction: None,
            block_number: 100,
        };
        // `address` should be a string; a number cannot be parsed.
        let broken =
            Filter::new(HandlerKind::Event).with_params(serde_json::json!({"address": 42}));
        assert!(!event_matches_filter(&event, &broken));
    }

    #[tokio::test]
    async fn rpc_backed_calls_report_their_milestone_rather_than_panicking() {
        // Until phase B2 lands these must fail cleanly, so a premature run gives
        // a readable message instead of a panic.
        let err = adapter().latest_height().await.unwrap_err();
        assert!(err.to_string().contains("Milestone 4"));
        assert!(!err.is_retryable());
    }
}
