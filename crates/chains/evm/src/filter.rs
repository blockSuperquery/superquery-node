//! EVM log and transaction filtering.
//!
//! Guide Milestone 6: *do filtering before invoking user mappings*. This is where
//! that happens, and it is worth doing carefully — an ERC-20 project on mainnet
//! sees a few hundred relevant logs per block out of tens of thousands, so a
//! filter that is wrong in the permissive direction costs orders of magnitude in
//! mapping time, and one wrong in the restrictive direction silently drops data.
//!
//! Topic matching follows the `eth_getLogs` convention, which has two subtleties
//! that are easy to get wrong:
//!
//! - a `null` in position *i* matches **any** value there, and
//! - a **list** in position *i* matches if any of its entries do (an OR),
//!
//! while positions are ANDed together. A filter shorter than the log's topic list
//! constrains only the positions it names.
//!
//! Upstream analogue: `filterLogsProcessor` in
//! `subql-ethereum/packages/node/src/ethereum/block.ethereum.ts`.

use serde::{Deserialize, Serialize};

/// An EVM log filter, as written in a manifest.
///
/// ```yaml
/// filter:
///   address: "0xa0b8...eb48"
///   topics:
///     - "Transfer(address,address,uint256)"
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvmLogFilter {
    /// Contract address to match. `None` matches any address.
    pub address: Option<String>,
    /// Positional topic constraints. An entry may be:
    /// - absent/`null` — match anything at that position;
    /// - a single value;
    /// - a list of alternatives (OR).
    #[serde(default)]
    pub topics: Vec<TopicFilter>,
}

/// One position's constraint in a topic filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TopicFilter {
    /// Match any value at this position.
    Any,
    /// Match exactly this value.
    One(String),
    /// Match any of these values.
    Any0f(Vec<String>),
}

impl TopicFilter {
    /// Whether `topic` satisfies this constraint.
    pub fn matches(&self, topic: &str) -> bool {
        match self {
            TopicFilter::Any => true,
            TopicFilter::One(expected) => eq_hex(expected, topic),
            TopicFilter::Any0f(options) => options.iter().any(|o| eq_hex(o, topic)),
        }
    }
}

/// An EVM transaction filter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvmTransactionFilter {
    /// Recipient address. `None` matches any, including contract creations.
    pub to: Option<String>,
    /// Sender address.
    pub from: Option<String>,
    /// Function selector or signature the input data must start with.
    pub function: Option<String>,
}

/// The parts of a log that filtering examines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogView<'a> {
    /// Emitting contract address.
    pub address: &'a str,
    /// Topics, `topic0` first.
    pub topics: &'a [String],
}

/// The parts of a transaction that filtering examines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionView<'a> {
    /// Recipient, or `None` for a contract creation.
    pub to: Option<&'a str>,
    /// Sender.
    pub from: &'a str,
    /// Call data.
    pub input: &'a str,
}

impl EvmLogFilter {
    /// Whether `log` matches.
    ///
    /// An empty filter matches everything, which is what an unfiltered handler in
    /// a manifest means.
    pub fn matches(&self, log: &LogView<'_>) -> bool {
        if let Some(expected) = &self.address {
            if !eq_hex(expected, log.address) {
                return false;
            }
        }

        // More constraints than the log has topics cannot be satisfied — except
        // where the surplus constraints are `Any`, which assert nothing.
        for (position, constraint) in self.topics.iter().enumerate() {
            match log.topics.get(position) {
                Some(topic) => {
                    if !constraint.matches(topic) {
                        return false;
                    }
                }
                None => {
                    if !matches!(constraint, TopicFilter::Any) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// The `topic0` this filter pins, if it pins exactly one.
    ///
    /// Lets the fetcher push the constraint down into `eth_getLogs`, or into a
    /// dictionary query, instead of fetching every log and discarding most.
    pub fn pinned_topic0(&self) -> Option<&str> {
        match self.topics.first() {
            Some(TopicFilter::One(topic)) => Some(topic.as_str()),
            _ => None,
        }
    }
}

impl EvmTransactionFilter {
    /// Whether `tx` matches.
    pub fn matches(&self, tx: &TransactionView<'_>) -> bool {
        if let Some(expected) = &self.to {
            // A contract creation has no recipient, so a `to` constraint cannot
            // match it.
            match tx.to {
                Some(actual) if eq_hex(expected, actual) => {}
                _ => return false,
            }
        }
        if let Some(expected) = &self.from {
            if !eq_hex(expected, tx.from) {
                return false;
            }
        }
        if let Some(selector) = &self.function {
            if !input_starts_with_selector(tx.input, selector) {
                return false;
            }
        }
        true
    }
}

/// Compare two hex strings, ignoring `0x` prefixes and case.
///
/// EVM addresses appear checksummed (EIP-55) in manifests and lowercase in RPC
/// responses, so a plain string comparison would miss almost every match.
fn eq_hex(a: &str, b: &str) -> bool {
    let a = a.strip_prefix("0x").unwrap_or(a);
    let b = b.strip_prefix("0x").unwrap_or(b);
    a.len() == b.len() && a.eq_ignore_ascii_case(b)
}

/// Whether call data begins with `selector` (the 4-byte function selector).
fn input_starts_with_selector(input: &str, selector: &str) -> bool {
    let input = input.strip_prefix("0x").unwrap_or(input);
    let selector = selector.strip_prefix("0x").unwrap_or(selector);
    if selector.is_empty() || input.len() < selector.len() {
        return false;
    }
    input[..selector.len()].eq_ignore_ascii_case(selector)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Transfer(address,address,uint256)`.
    const TRANSFER_SIG: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
    /// `Approval(address,address,uint256)`.
    const APPROVAL_SIG: &str = "0x8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925";

    const USDC: &str = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
    const ALICE: &str = "0x000000000000000000000000aaaa000000000000000000000000000000000000";
    const BOB: &str = "0x000000000000000000000000bbbb000000000000000000000000000000000000";

    fn transfer_log<'a>(topics: &'a [String]) -> LogView<'a> {
        LogView {
            address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
            topics,
        }
    }

    fn topics(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn an_empty_filter_matches_everything() {
        let t = topics(&[TRANSFER_SIG]);
        assert!(EvmLogFilter::default().matches(&transfer_log(&t)));
    }

    #[test]
    fn checksummed_and_lowercase_addresses_are_the_same_address() {
        // The manifest carries EIP-55 checksummed case; the RPC returns
        // lowercase. A case-sensitive compare here would match nothing.
        let filter = EvmLogFilter {
            address: Some(USDC.to_string()),
            topics: vec![],
        };
        let t = topics(&[TRANSFER_SIG]);
        assert!(filter.matches(&transfer_log(&t)));
    }

    #[test]
    fn a_different_contract_does_not_match() {
        let filter = EvmLogFilter {
            address: Some("0x1111111111111111111111111111111111111111".to_string()),
            topics: vec![],
        };
        let t = topics(&[TRANSFER_SIG]);
        assert!(!filter.matches(&transfer_log(&t)));
    }

    #[test]
    fn the_erc20_transfer_case() {
        // Guide Milestone 6's acceptance: the handler runs only for matching logs.
        let filter = EvmLogFilter {
            address: Some(USDC.to_string()),
            topics: vec![TopicFilter::One(TRANSFER_SIG.to_string())],
        };

        let transfer = topics(&[TRANSFER_SIG, ALICE, BOB]);
        assert!(filter.matches(&transfer_log(&transfer)));

        let approval = topics(&[APPROVAL_SIG, ALICE, BOB]);
        assert!(!filter.matches(&transfer_log(&approval)));
    }

    #[test]
    fn null_positions_match_anything() {
        // Transfers *to* Bob, from anyone.
        let filter = EvmLogFilter {
            address: None,
            topics: vec![
                TopicFilter::One(TRANSFER_SIG.to_string()),
                TopicFilter::Any,
                TopicFilter::One(BOB.to_string()),
            ],
        };

        assert!(filter.matches(&transfer_log(&topics(&[TRANSFER_SIG, ALICE, BOB]))));
        assert!(filter.matches(&transfer_log(&topics(&[TRANSFER_SIG, BOB, BOB]))));
        // ...but not transfers to Alice.
        assert!(!filter.matches(&transfer_log(&topics(&[TRANSFER_SIG, BOB, ALICE]))));
    }

    #[test]
    fn a_list_at_one_position_is_an_or() {
        let filter = EvmLogFilter {
            address: None,
            topics: vec![TopicFilter::Any0f(vec![
                TRANSFER_SIG.to_string(),
                APPROVAL_SIG.to_string(),
            ])],
        };
        assert!(filter.matches(&transfer_log(&topics(&[TRANSFER_SIG]))));
        assert!(filter.matches(&transfer_log(&topics(&[APPROVAL_SIG]))));
        assert!(!filter.matches(&transfer_log(&topics(&[
            "0xdeadbeef00000000000000000000000000000000000000000000000000000000"
        ]))));
    }

    #[test]
    fn positions_are_anded_together() {
        let filter = EvmLogFilter {
            address: None,
            topics: vec![
                TopicFilter::One(TRANSFER_SIG.to_string()),
                TopicFilter::One(ALICE.to_string()),
            ],
        };
        // Right signature, wrong sender: both must hold.
        assert!(!filter.matches(&transfer_log(&topics(&[TRANSFER_SIG, BOB]))));
        assert!(filter.matches(&transfer_log(&topics(&[TRANSFER_SIG, ALICE]))));
    }

    #[test]
    fn a_filter_longer_than_the_log_cannot_match() {
        let filter = EvmLogFilter {
            address: None,
            topics: vec![
                TopicFilter::One(TRANSFER_SIG.to_string()),
                TopicFilter::One(ALICE.to_string()),
            ],
        };
        // An anonymous or single-topic log has nothing at position 1.
        assert!(!filter.matches(&transfer_log(&topics(&[TRANSFER_SIG]))));
    }

    #[test]
    fn surplus_any_constraints_assert_nothing() {
        let filter = EvmLogFilter {
            address: None,
            topics: vec![
                TopicFilter::One(TRANSFER_SIG.to_string()),
                TopicFilter::Any,
                TopicFilter::Any,
            ],
        };
        // Trailing `Any` positions must not exclude a shorter log.
        assert!(filter.matches(&transfer_log(&topics(&[TRANSFER_SIG]))));
    }

    #[test]
    fn a_filter_shorter_than_the_log_constrains_only_what_it_names() {
        let filter = EvmLogFilter {
            address: None,
            topics: vec![TopicFilter::One(TRANSFER_SIG.to_string())],
        };
        assert!(filter.matches(&transfer_log(&topics(&[TRANSFER_SIG, ALICE, BOB]))));
    }

    #[test]
    fn a_pinned_topic0_can_be_pushed_down_to_the_rpc() {
        let pinned = EvmLogFilter {
            address: None,
            topics: vec![TopicFilter::One(TRANSFER_SIG.to_string())],
        };
        assert_eq!(pinned.pinned_topic0(), Some(TRANSFER_SIG));

        // An OR or a wildcard is not a single pinned value.
        let either = EvmLogFilter {
            address: None,
            topics: vec![TopicFilter::Any0f(vec![TRANSFER_SIG.to_string()])],
        };
        assert_eq!(either.pinned_topic0(), None);
        assert_eq!(EvmLogFilter::default().pinned_topic0(), None);
    }

    #[test]
    fn transaction_filters_match_on_participants_and_selector() {
        let filter = EvmTransactionFilter {
            to: Some(USDC.to_string()),
            from: None,
            function: Some("0xa9059cbb".to_string()), // transfer(address,uint256)
        };

        let tx = TransactionView {
            to: Some("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"),
            from: "0xaaaa000000000000000000000000000000000000",
            input: "0xa9059cbb000000000000000000000000bbbb",
        };
        assert!(filter.matches(&tx));

        let wrong_selector = TransactionView {
            input: "0x095ea7b3000000000000000000000000bbbb", // approve
            ..tx.clone()
        };
        assert!(!filter.matches(&wrong_selector));
    }

    #[test]
    fn a_to_constraint_excludes_contract_creations() {
        let filter = EvmTransactionFilter {
            to: Some(USDC.to_string()),
            ..Default::default()
        };
        let creation = TransactionView {
            to: None,
            from: "0xaaaa000000000000000000000000000000000000",
            input: "0x6080604052",
        };
        assert!(!filter.matches(&creation));
    }

    #[test]
    fn an_empty_transaction_filter_matches_everything() {
        let creation = TransactionView {
            to: None,
            from: "0xaaaa000000000000000000000000000000000000",
            input: "0x",
        };
        assert!(EvmTransactionFilter::default().matches(&creation));
    }

    #[test]
    fn selector_matching_needs_enough_input_to_compare() {
        let tx = TransactionView {
            to: None,
            from: "0xaaaa000000000000000000000000000000000000",
            input: "0xa905", // truncated
        };
        let filter = EvmTransactionFilter {
            function: Some("0xa9059cbb".to_string()),
            ..Default::default()
        };
        assert!(!filter.matches(&tx));
    }

    #[test]
    fn hex_comparison_ignores_prefix_and_case_but_not_length() {
        assert!(eq_hex("0xABCD", "abcd"));
        assert!(eq_hex("abcd", "0xABCD"));
        // A prefix must not pass as a match.
        assert!(!eq_hex("0xabcd", "0xabcdef"));
        assert!(!eq_hex("0xabcd", "0xabce"));
    }

    #[test]
    fn topic_filters_deserialize_from_manifest_shapes() {
        // A manifest may give a string, a list, or null at each position.
        let json = serde_json::json!({
            "address": USDC,
            "topics": [TRANSFER_SIG, null, [ALICE, BOB]]
        });
        let filter: EvmLogFilter = serde_json::from_value(json).unwrap();

        assert_eq!(filter.address.as_deref(), Some(USDC));
        assert!(matches!(filter.topics[0], TopicFilter::One(_)));
        assert!(matches!(filter.topics[1], TopicFilter::Any));
        assert!(matches!(filter.topics[2], TopicFilter::Any0f(ref v) if v.len() == 2));
    }
}
