//! Reorg detection: deciding whether what we indexed is still the canonical chain,
//! and if not, how far back to go.
//!
//! ```text
//! stored hash for H  !=  canonical hash for H
//!        |
//!        v
//! find common ancestor
//!        |
//!        v
//! rewind entity mutations
//!        |
//!        v
//! re-index canonical branch
//! ```
//!
//! The search is the delicate part, so it is written here as pure functions over
//! stored-vs-canonical header pairs, testable without a chain or a database. The
//! I/O that supplies those pairs lives in [`super::tracker`].
//!
//! Upstream analogue: `node-core/src/indexer/unfinalizedBlocks.service.ts`.

use superquery_chain_api::{BlockPtr, Header};

/// One height's stored hash next to the chain's current answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeightComparison {
    /// The height compared.
    pub height: u64,
    /// What we recorded when we indexed it.
    pub stored_hash: String,
    /// What the chain reports now. `None` when the chain no longer has a block at
    /// this height at all.
    pub canonical_hash: Option<String>,
}

impl HeightComparison {
    /// Whether our record still matches the chain.
    pub fn matches(&self) -> bool {
        self.canonical_hash.as_deref() == Some(self.stored_hash.as_str())
    }
}

/// The outcome of checking indexed state against the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReorgDecision {
    /// Indexed state is on the canonical chain; carry on.
    Canonical,
    /// A fork was found. Rewind to `ancestor`, then re-index from the next height.
    Rewind {
        /// The deepest block still shared with the canonical chain.
        ancestor: BlockPtr,
        /// How many indexed blocks are discarded.
        depth: u64,
    },
    /// A fork exists deeper than the retained header history, so the common
    /// ancestor cannot be located.
    ///
    /// Escalated rather than guessed: rewinding to an unverified height would
    /// leave state that looks committed but never existed on any chain.
    BeyondHistory {
        /// Oldest height still in the header table.
        oldest_known: u64,
    },
}

/// Find the common ancestor from headers ordered **highest first**.
///
/// Returns the deepest height whose stored hash still matches the chain. The
/// input must be contiguous and descending — [`super::tracker`] reads it that way
/// out of `_superquery_blocks`.
///
/// Walking down from the tip rather than up from the base matters in the common
/// case: a shallow reorg is found after one or two comparisons, whereas an
/// ascending scan would read the whole retained history every time.
pub fn find_common_ancestor(descending: &[HeightComparison]) -> ReorgDecision {
    let Some(tip) = descending.first() else {
        // Nothing indexed yet, so nothing can be wrong.
        return ReorgDecision::Canonical;
    };

    if tip.matches() {
        return ReorgDecision::Canonical;
    }

    for (index, comparison) in descending.iter().enumerate() {
        if comparison.matches() {
            return ReorgDecision::Rewind {
                ancestor: BlockPtr::new(comparison.height, comparison.stored_hash.clone()),
                // Everything scanned before this point diverged.
                depth: index as u64,
            };
        }
    }

    // Every retained header diverged: the fork is older than our history.
    ReorgDecision::BeyondHistory {
        oldest_known: descending.last().map(|c| c.height).unwrap_or(tip.height),
    }
}

/// Whether a newly fetched header continues the chain we have indexed.
///
/// The cheap check run on every block, before the expensive comparison walk: if
/// the incoming block's parent is the block we last committed, no reorg has
/// happened.
pub fn continues_chain(last_indexed: &BlockPtr, incoming: &Header) -> bool {
    incoming.height == last_indexed.height + 1
        && incoming.parent_hash.as_deref() == Some(last_indexed.hash.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmp(height: u64, stored: &str, canonical: Option<&str>) -> HeightComparison {
        HeightComparison {
            height,
            stored_hash: stored.to_string(),
            canonical_hash: canonical.map(str::to_string),
        }
    }

    #[test]
    fn a_matching_tip_needs_no_further_work() {
        let history = [
            cmp(100, "0x64", Some("0x64")),
            cmp(99, "0x63", Some("0x63")),
        ];
        assert_eq!(find_common_ancestor(&history), ReorgDecision::Canonical);
    }

    #[test]
    fn finds_a_one_block_reorg() {
        let history = [
            cmp(100, "0xbad", Some("0xgood")),
            cmp(99, "0x63", Some("0x63")),
        ];
        assert_eq!(
            find_common_ancestor(&history),
            ReorgDecision::Rewind {
                ancestor: BlockPtr::new(99, "0x63"),
                depth: 1,
            }
        );
    }

    #[test]
    fn finds_a_three_block_reorg() {
        // The synthetic fork from guide Milestone 11's acceptance test.
        let history = [
            cmp(103, "0xf3", Some("0xc3")),
            cmp(102, "0xf2", Some("0xc2")),
            cmp(101, "0xf1", Some("0xc1")),
            cmp(100, "0x64", Some("0x64")),
        ];
        assert_eq!(
            find_common_ancestor(&history),
            ReorgDecision::Rewind {
                ancestor: BlockPtr::new(100, "0x64"),
                depth: 3,
            }
        );
    }

    #[test]
    fn a_vanished_block_counts_as_diverged() {
        // The chain is shorter than our record — reorg to a shorter branch.
        let history = [cmp(100, "0x64", None), cmp(99, "0x63", Some("0x63"))];
        assert_eq!(
            find_common_ancestor(&history),
            ReorgDecision::Rewind {
                ancestor: BlockPtr::new(99, "0x63"),
                depth: 1,
            }
        );
    }

    #[test]
    fn a_fork_older_than_our_history_is_escalated_not_guessed() {
        let history = [
            cmp(100, "0xf2", Some("0xc2")),
            cmp(99, "0xf1", Some("0xc1")),
            cmp(98, "0xf0", Some("0xc0")),
        ];
        assert_eq!(
            find_common_ancestor(&history),
            ReorgDecision::BeyondHistory { oldest_known: 98 }
        );
    }

    #[test]
    fn empty_history_is_canonical() {
        assert_eq!(find_common_ancestor(&[]), ReorgDecision::Canonical);
    }

    #[test]
    fn continuation_requires_both_height_and_parent() {
        let last = BlockPtr::new(100, "0x64");
        let good = Header {
            height: 101,
            hash: "0x65".into(),
            parent_hash: Some("0x64".into()),
            timestamp: None,
        };
        assert!(continues_chain(&last, &good));

        let forked = Header {
            parent_hash: Some("0xother".into()),
            ..good.clone()
        };
        assert!(!continues_chain(&last, &forked));

        let gap = Header {
            height: 105,
            ..good.clone()
        };
        assert!(!continues_chain(&last, &gap));

        // An adapter that cannot supply a parent hash cannot assert continuation.
        let unknown_parent = Header {
            parent_hash: None,
            ..good
        };
        assert!(!continues_chain(&last, &unknown_parent));
    }
}
