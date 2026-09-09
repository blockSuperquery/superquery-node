//! Handler kinds and the filter representation shared between the manifest, the
//! engine, and chain adapters.
//!
//! The engine must decide *which blocks are worth fetching* without knowing what a
//! log or an extrinsic is. So a [`Filter`] carries two parts:
//!
//! - a [`HandlerKind`] and an optional [`BlockFilter`], which are genuinely
//!   chain-agnostic and which `superquery-core` acts on directly;
//! - `params`, an opaque JSON object whose schema each adapter defines and
//!   validates (EVM: `address` + `topics`; Substrate: `module` + `method`; …).
//!
//! This mirrors how the manifest is actually written, and keeps chain vocabulary
//! out of the core crate (guide Milestone 3).

use serde::{Deserialize, Serialize};

use crate::error::{ChainError, Result};

/// What a handler is invoked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HandlerKind {
    /// Once per block.
    Block,
    /// Once per matching transaction.
    Transaction,
    /// Once per matching event/log.
    Event,
}

impl HandlerKind {
    /// The manifest spelling of this kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            HandlerKind::Block => "block",
            HandlerKind::Transaction => "transaction",
            HandlerKind::Event => "event",
        }
    }
}

/// Block-level constraints the engine can evaluate on its own, before fetching
/// any payload.
///
/// Upstream analogue: the `modulo` / `timestamp` block filters in
/// `node-core/src/indexer/fetch.service.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockFilter {
    /// Run only on heights where `height % modulo == 0`.
    pub modulo: Option<u64>,
    /// Cron-like timestamp filter, kept as written in the manifest. Interpreted by
    /// the scheduler, not here.
    pub timestamp: Option<String>,
}

impl BlockFilter {
    /// Whether `height` satisfies the modulo constraint. A filter with no modulo
    /// matches every height.
    pub fn matches_height(&self, height: u64) -> bool {
        match self.modulo {
            Some(0) | None => true,
            Some(m) => height.is_multiple_of(m),
        }
    }
}

/// One handler's filter: kind, block-level constraints, and opaque chain params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    /// The kind of handler this filter belongs to.
    pub handler: HandlerKind,
    /// Constraints the engine evaluates itself.
    #[serde(default)]
    pub block: BlockFilter,
    /// Chain-specific criteria. Shape is defined and validated by the adapter.
    #[serde(default)]
    pub params: serde_json::Value,
}

impl Filter {
    /// A filter of `handler` kind with no constraints — matches everything.
    pub fn new(handler: HandlerKind) -> Self {
        Self {
            handler,
            block: BlockFilter::default(),
            params: serde_json::Value::Null,
        }
    }

    /// Attach chain-specific params.
    pub fn with_params(mut self, params: serde_json::Value) -> Self {
        self.params = params;
        self
    }

    /// Attach block-level constraints.
    pub fn with_block(mut self, block: BlockFilter) -> Self {
        self.block = block;
        self
    }

    /// Deserialize `params` into an adapter's own filter type.
    ///
    /// Adapters call this once when loading the manifest so a malformed filter is
    /// a startup error rather than a per-block surprise.
    pub fn parse_params<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_value(self.params.clone())
            .map_err(|e| ChainError::Decode(format!("invalid filter params: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn modulo_gating() {
        let every = BlockFilter::default();
        assert!(every.matches_height(1));
        assert!(every.matches_height(7));

        let tenth = BlockFilter {
            modulo: Some(10),
            timestamp: None,
        };
        assert!(tenth.matches_height(0));
        assert!(tenth.matches_height(20));
        assert!(!tenth.matches_height(21));

        // A zero modulo would panic on `%`; treat it as "no constraint".
        let zero = BlockFilter {
            modulo: Some(0),
            timestamp: None,
        };
        assert!(zero.matches_height(3));
    }

    #[derive(Debug, serde::Deserialize, PartialEq)]
    struct EvmParams {
        address: String,
    }

    #[test]
    fn params_round_trip_into_adapter_type() {
        let f = Filter::new(HandlerKind::Event).with_params(json!({"address": "0xabc"}));
        let parsed: EvmParams = f.parse_params().unwrap();
        assert_eq!(parsed.address, "0xabc");
    }

    #[test]
    fn malformed_params_are_a_decode_error() {
        let f = Filter::new(HandlerKind::Event).with_params(json!({"address": 42}));
        assert!(matches!(
            f.parse_params::<EvmParams>(),
            Err(ChainError::Decode(_))
        ));
    }

    #[test]
    fn handler_kind_manifest_spelling() {
        assert_eq!(HandlerKind::Block.as_str(), "block");
        assert_eq!(HandlerKind::Event.as_str(), "event");
        // Serde spelling must match the manifest too.
        assert_eq!(
            serde_json::to_value(HandlerKind::Transaction).unwrap(),
            json!("transaction")
        );
    }
}
