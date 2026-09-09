//! EVM block, transaction and log types as the indexer sees them.
//!
//! Deliberately *not* alloy's RPC types re-exported. Two reasons: mappings receive
//! JSON shaped by this module, so its field names are part of the SDK's public
//! contract and must not drift when alloy changes; and the indexer needs
//! logs already associated with their transactions, which the raw
//! `eth_getBlockByNumber` response does not provide.

use serde::{Deserialize, Serialize};
use superquery_chain_api::Header;

use crate::filter::{LogView, TransactionView};

/// A fetched EVM block with everything handlers may need.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvmBlock {
    /// Block height.
    pub number: u64,
    /// Block hash.
    pub hash: String,
    /// Parent block hash.
    pub parent_hash: String,
    /// Unix timestamp in seconds.
    pub timestamp: u64,
    /// Transactions, in block order.
    pub transactions: Vec<EvmTransaction>,
    /// Logs across the whole block, in emission order.
    pub logs: Vec<EvmLog>,
}

impl EvmBlock {
    /// The chain-agnostic header the engine tracks.
    pub fn header(&self) -> Header {
        Header {
            height: self.number,
            hash: self.hash.clone(),
            parent_hash: Some(self.parent_hash.clone()),
            timestamp: chrono::DateTime::from_timestamp(self.timestamp as i64, 0),
        }
    }
}

/// An EVM transaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvmTransaction {
    /// Transaction hash.
    pub hash: String,
    /// Index within the block.
    pub transaction_index: u64,
    /// Sender.
    pub from: String,
    /// Recipient, or `None` for a contract creation.
    pub to: Option<String>,
    /// Value transferred, as a decimal string. A string because `u256` does not
    /// fit any Rust integer, and JSON numbers lose precision above 2^53.
    pub value: String,
    /// Call data.
    pub input: String,
}

impl EvmTransaction {
    /// A borrowed view for filtering.
    pub fn view(&self) -> TransactionView<'_> {
        TransactionView {
            to: self.to.as_deref(),
            from: &self.from,
            input: &self.input,
        }
    }
}

/// An EVM log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvmLog {
    /// Emitting contract.
    pub address: String,
    /// Indexed topics, `topic0` first.
    pub topics: Vec<String>,
    /// ABI-encoded non-indexed data.
    pub data: String,
    /// Index within the block.
    pub log_index: u64,
    /// Hash of the transaction that emitted it.
    pub transaction_hash: String,
    /// Index of that transaction within the block.
    pub transaction_index: u64,
}

impl EvmLog {
    /// A borrowed view for filtering.
    pub fn view(&self) -> LogView<'_> {
        LogView {
            address: &self.address,
            topics: &self.topics,
        }
    }

    /// The event signature hash, if the log has one.
    ///
    /// Anonymous events have no `topic0`, so this is an `Option` rather than an
    /// index into `topics`.
    pub fn topic0(&self) -> Option<&str> {
        self.topics.first().map(String::as_str)
    }
}

/// An event handed to a mapping: a log plus the transaction that produced it.
///
/// Paired at decode time because mappings routinely need the sender or the
/// transaction hash while handling a log, and re-scanning the block for it at
/// every handler invocation would be wasteful.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvmEvent {
    /// The log.
    pub log: EvmLog,
    /// The transaction it came from, when present in the block.
    pub transaction: Option<EvmTransaction>,
    /// Height of the block it was emitted in.
    pub block_number: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block() -> EvmBlock {
        EvmBlock {
            number: 18_000_000,
            hash: "0xaaa".into(),
            parent_hash: "0xbbb".into(),
            timestamp: 1_692_100_800,
            transactions: vec![EvmTransaction {
                hash: "0xtx1".into(),
                transaction_index: 0,
                from: "0xalice".into(),
                to: Some("0xusdc".into()),
                value: "0".into(),
                input: "0xa9059cbb".into(),
            }],
            logs: vec![EvmLog {
                address: "0xusdc".into(),
                topics: vec!["0xddf252ad".into(), "0xalice".into()],
                data: "0x00".into(),
                log_index: 0,
                transaction_hash: "0xtx1".into(),
                transaction_index: 0,
            }],
        }
    }

    #[test]
    fn header_carries_lineage_and_time() {
        let h = block().header();
        assert_eq!(h.height, 18_000_000);
        assert_eq!(h.hash, "0xaaa");
        // The parent hash is what makes reorg detection possible.
        assert_eq!(h.parent_hash.as_deref(), Some("0xbbb"));
        assert!(h.timestamp.is_some());
    }

    #[test]
    fn views_borrow_rather_than_clone() {
        let b = block();
        let log_view = b.logs[0].view();
        assert_eq!(log_view.address, "0xusdc");
        assert_eq!(log_view.topics.len(), 2);

        let tx_view = b.transactions[0].view();
        assert_eq!(tx_view.to, Some("0xusdc"));
        assert_eq!(tx_view.from, "0xalice");
    }

    #[test]
    fn anonymous_logs_have_no_topic0() {
        let mut log = block().logs[0].clone();
        assert_eq!(log.topic0(), Some("0xddf252ad"));
        log.topics.clear();
        assert_eq!(log.topic0(), None);
    }

    #[test]
    fn value_survives_a_json_round_trip_at_u256_scale() {
        // A u256 value exceeds f64 precision; as a string it stays exact.
        let mut b = block();
        b.transactions[0].value =
            "115792089237316195423570985008687907853269984665640564039457584007913129639935".into();

        let json = serde_json::to_string(&b).unwrap();
        let back: EvmBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(back.transactions[0].value, b.transactions[0].value);
    }

    #[test]
    fn json_field_names_are_camel_case() {
        // These names reach mapping authors through the SDK, so they are a public
        // contract and must not drift.
        let json = serde_json::to_value(block()).unwrap();
        assert!(json["parentHash"].is_string());
        assert!(json["transactions"][0]["transactionIndex"].is_number());
        assert!(json["logs"][0]["logIndex"].is_number());
        assert!(json["logs"][0]["transactionHash"].is_string());
    }
}
