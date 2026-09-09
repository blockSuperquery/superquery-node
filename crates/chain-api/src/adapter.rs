//! The [`ChainAdapter`] trait — the single seam between the chain-agnostic engine
//! and any specific blockchain.
//!
//! Upstream analogue: `IBlockchainService` in
//! [`node-core/src/blockchain.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/blockchain.service.ts).
//! SubQuery threads ~9 generic parameters through that interface; here the
//! chain-specific types are associated types, so the engine names none of them.
//!
//! **Invariant:** nothing above this trait may reference a chain SDK. `alloy`
//! belongs to `superquery-chain-evm` alone (guide Milestone 3).

use async_trait::async_trait;

use crate::block::{Header, IBlock};
use crate::error::Result;
use crate::filter::Filter;

/// Everything the engine needs from a blockchain.
///
/// Implementations are expected to be cheap to clone or to be used behind an
/// `Arc`; the scheduler calls these concurrently from many tasks.
#[async_trait]
pub trait ChainAdapter: Send + Sync + 'static {
    /// The chain's block payload.
    type Block: Send + Sync;

    /// A decoded, filterable event (an EVM log, a Substrate event, …).
    type Event: Send + Sync;

    /// The fetched-block wrapper carrying [`Self::Block`].
    type FetchedBlock: IBlock<Inner = Self::Block> + Send + Sync;

    /// Stable identifier for the network being indexed (EVM chain id, Substrate
    /// genesis hash, …). Persisted in `_superquery_metadata` and checked on
    /// restart so a project cannot silently switch chains.
    fn network_id(&self) -> &str;

    /// Human-readable chain name, for logs and the admin API.
    fn chain_name(&self) -> &str {
        self.network_id()
    }

    /// Verify the endpoint actually serves `expected` — call once at startup.
    ///
    /// Returns [`ChainError::ChainIdMismatch`](crate::ChainError::ChainIdMismatch)
    /// when it does not.
    async fn validate_network(&self, expected: &str) -> Result<()>;

    /// Height of the chain head, including unfinalized blocks.
    async fn latest_height(&self) -> Result<u64>;

    /// Height of the most recent block the chain considers final.
    ///
    /// Chains without a finality gadget should return
    /// `latest_height().saturating_sub(confirmations)`.
    async fn finalized_height(&self) -> Result<u64>;

    /// Fetch one block with everything the project's handlers may need
    /// (transactions, receipts, logs).
    async fn fetch_block(&self, height: u64) -> Result<Self::FetchedBlock>;

    /// Fetch a contiguous range. The default fetches sequentially; adapters with
    /// batch RPC support should override it.
    ///
    /// Returns blocks in ascending height order.
    async fn fetch_blocks(&self, heights: &[u64]) -> Result<Vec<Self::FetchedBlock>> {
        let mut out = Vec::with_capacity(heights.len());
        for &h in heights {
            out.push(self.fetch_block(h).await?);
        }
        Ok(out)
    }

    /// The header at `height` on the canonical chain, without the payload.
    ///
    /// Reorg detection calls this constantly, so it should be the cheapest call
    /// the adapter offers.
    async fn header_at(&self, height: u64) -> Result<Header>;

    /// Decode the events a block contains, in chain order.
    async fn decode_events(&self, block: &Self::Block) -> Result<Vec<Self::Event>>;

    /// Whether `event` matches `filter`.
    ///
    /// Runs before any mapping is invoked (guide Milestone 6), so it must be pure
    /// and allocation-light.
    fn event_matches(&self, event: &Self::Event, filter: &Filter) -> bool;

    /// Approximate block interval in milliseconds, used to pace polling when the
    /// indexer has caught up to the head.
    fn block_interval_ms(&self) -> u64 {
        12_000
    }
}

/// Optional accelerated block discovery — a dictionary or pre-built index that can
/// answer "which heights in this range could possibly match these filters?".
///
/// Guide §3.8: strictly an optimisation. A dictionary-assisted run and a raw-RPC
/// run must produce identical state, so the engine treats a `None` provider and a
/// provider that returns every height as equivalent.
#[async_trait]
pub trait BlockCandidateProvider: Send + Sync {
    /// Heights within `from..=to` that may contain a match, ascending.
    ///
    /// May over-approximate (extra heights cost time, not correctness) but must
    /// never under-approximate.
    async fn candidate_heights(&self, from: u64, to: u64, filters: &[Filter]) -> Result<Vec<u64>>;

    /// Highest height this provider has indexed. The engine falls back to raw RPC
    /// beyond it.
    async fn provider_head(&self) -> Result<u64>;
}
