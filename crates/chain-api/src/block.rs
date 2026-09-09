//! Chain-agnostic block identity types.
//!
//! Derived from SubQuery's `Header` / `IBlock<B>` (`node-core/src/indexer/types.ts`),
//! reshaped for Rust: a header carries only what the *engine* needs — identity,
//! lineage and time — while the chain-specific payload stays behind an associated
//! type so `superquery-core` never has to know what a block contains.

use chrono::{DateTime, Utc};

/// A block's identity: its height and hash.
///
/// Used wherever the engine needs to name a block without carrying its payload —
/// checkpoints, reorg comparisons, log lines.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockPtr {
    /// Block height (number).
    pub height: u64,
    /// Canonical block hash, in the chain's own display encoding.
    pub hash: String,
}

impl BlockPtr {
    /// Construct a pointer.
    pub fn new(height: u64, hash: impl Into<String>) -> Self {
        Self {
            height,
            hash: hash.into(),
        }
    }
}

impl std::fmt::Display for BlockPtr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{} ({})", self.height, self.hash)
    }
}

/// A block header: identity plus the lineage and timestamp the engine relies on.
///
/// `parent_hash` is what makes reorg detection possible (guide §3.6) — an adapter
/// that cannot supply it forfeits common-ancestor search. `timestamp` is optional
/// because not every chain exposes one on every block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Block height.
    pub height: u64,
    /// This block's hash.
    pub hash: String,
    /// The parent block's hash. `None` only for genesis, or chains that do not
    /// expose it.
    pub parent_hash: Option<String>,
    /// Block timestamp, when the chain provides one.
    pub timestamp: Option<DateTime<Utc>>,
}

impl Header {
    /// The header's identity as a [`BlockPtr`].
    pub fn ptr(&self) -> BlockPtr {
        BlockPtr::new(self.height, self.hash.clone())
    }

    /// Whether `self` is a direct child of `parent`.
    ///
    /// Returns `false` when the parent hash is unknown — an unverifiable link is
    /// not a valid one, and treating it as valid would mask reorgs.
    pub fn is_child_of(&self, parent: &Header) -> bool {
        self.height == parent.height + 1
            && self.parent_hash.as_deref() == Some(parent.hash.as_str())
    }
}

/// How final a block is believed to be.
///
/// Chains differ in what they can promise: some have explicit finality gadgets,
/// others only probabilistic depth. The engine treats [`FinalityState::Final`] as
/// "safe to compact history past this point".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalityState {
    /// Indexed but still reorg-able.
    Unfinalized,
    /// The chain considers this block final.
    Final,
}

/// A fetched block: its header plus the chain's own payload.
///
/// A trait rather than a struct so adapters can wrap native block types without
/// copying them. [`GenericBlock`] is the obvious implementation when no wrapping
/// is needed.
pub trait IBlock: Send + Sync {
    /// The chain-specific block payload.
    type Inner;

    /// The block's header.
    fn header(&self) -> &Header;
    /// The chain-specific payload.
    fn inner(&self) -> &Self::Inner;
    /// Consume the wrapper, yielding the payload.
    fn into_inner(self) -> Self::Inner;
}

/// The straightforward `(Header, B)` implementation of [`IBlock`].
#[derive(Debug, Clone)]
pub struct GenericBlock<B> {
    /// The block header.
    pub header: Header,
    /// The chain-specific payload.
    pub inner: B,
}

impl<B> GenericBlock<B> {
    /// Pair a header with its payload.
    pub fn new(header: Header, inner: B) -> Self {
        Self { header, inner }
    }
}

impl<B: Send + Sync> IBlock for GenericBlock<B> {
    type Inner = B;

    fn header(&self) -> &Header {
        &self.header
    }
    fn inner(&self) -> &B {
        &self.inner
    }
    fn into_inner(self) -> B {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(height: u64, hash: &str, parent: Option<&str>) -> Header {
        Header {
            height,
            hash: hash.to_string(),
            parent_hash: parent.map(str::to_string),
            timestamp: None,
        }
    }

    #[test]
    fn child_link_requires_height_and_hash() {
        let a = header(10, "0xa", None);
        let b = header(11, "0xb", Some("0xa"));
        assert!(b.is_child_of(&a));

        // Right height, wrong parent — a fork.
        let fork = header(11, "0xb2", Some("0xother"));
        assert!(!fork.is_child_of(&a));

        // Right parent, wrong height.
        let skipped = header(12, "0xc", Some("0xa"));
        assert!(!skipped.is_child_of(&a));
    }

    #[test]
    fn unknown_parent_is_not_a_valid_link() {
        let a = header(10, "0xa", None);
        let orphan = header(11, "0xb", None);
        assert!(!orphan.is_child_of(&a));
    }

    #[test]
    fn ptr_and_display() {
        let h = header(42, "0xdead", None);
        assert_eq!(h.ptr(), BlockPtr::new(42, "0xdead"));
        assert_eq!(h.ptr().to_string(), "#42 (0xdead)");
    }
}
