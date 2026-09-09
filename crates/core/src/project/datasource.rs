//! Data sources: what a project watches, and from which height.
//!
//! A **static** data source comes from the manifest and is active from a fixed
//! start height. A **dynamic** one is created by a handler at runtime — the
//! factory-contract pattern, where indexing a `PairCreated` event means starting
//! to watch the new pair (guide §3.7).
//!
//! Dynamic sources must be persisted, because they are part of the answer to
//! "what was being indexed at height N". Recomputing them by replaying handlers
//! would work, but only if replay is already correct — and replay is what they are
//! needed for. So they are written to the database in the same transaction as the
//! block that created them (guide Milestone 10).

use superquery_chain_api::{Filter, HandlerKind};

/// One thing the project watches.
#[derive(Debug, Clone, PartialEq)]
pub struct DataSource {
    /// Manifest name, or the template name for dynamic sources.
    pub name: String,
    /// First height this source is active from.
    pub start_height: u64,
    /// Last height it is active for, if bounded.
    pub end_height: Option<u64>,
    /// Handlers and their filters.
    pub handlers: Vec<Handler>,
    /// How this source came to exist.
    pub origin: DataSourceOrigin,
}

/// Where a data source came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataSourceOrigin {
    /// Declared in the project manifest.
    Static,
    /// Created by a handler while indexing.
    Dynamic {
        /// Manifest template it was instantiated from.
        template: String,
        /// Height at which the creating block was indexed.
        created_at: u64,
    },
}

/// A handler and the filter deciding when it runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Handler {
    /// Exported function name in the mapping module.
    pub function: String,
    /// What the handler is invoked for.
    pub kind: HandlerKind,
    /// Criteria the input must satisfy.
    pub filter: Filter,
}

impl DataSource {
    /// Whether this source is active at `height`.
    pub fn is_active_at(&self, height: u64) -> bool {
        height >= self.start_height && self.end_height.is_none_or(|end| height <= end)
    }

    /// Filters for handlers of one kind. Used to build the fetch-time filter set,
    /// so blocks are screened before any mapping runs (guide Milestone 6).
    pub fn filters_for(&self, kind: HandlerKind) -> Vec<&Filter> {
        self.handlers
            .iter()
            .filter(|h| h.kind == kind)
            .map(|h| &h.filter)
            .collect()
    }
}

/// A dynamic data source as persisted, matching the guide's §3.7 shape.
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicDataSource {
    /// Manifest template to instantiate.
    pub template: String,
    /// Height indexing of this source begins at.
    pub start_height: u64,
    /// Template parameters, e.g. the discovered contract address.
    pub parameters: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use superquery_chain_api::Filter;

    fn source(start: u64, end: Option<u64>) -> DataSource {
        DataSource {
            name: "Erc20".into(),
            start_height: start,
            end_height: end,
            handlers: vec![
                Handler {
                    function: "handleTransfer".into(),
                    kind: HandlerKind::Event,
                    filter: Filter::new(HandlerKind::Event),
                },
                Handler {
                    function: "handleBlock".into(),
                    kind: HandlerKind::Block,
                    filter: Filter::new(HandlerKind::Block),
                },
            ],
            origin: DataSourceOrigin::Static,
        }
    }

    #[test]
    fn open_ended_sources_stay_active() {
        let ds = source(100, None);
        assert!(!ds.is_active_at(99));
        assert!(ds.is_active_at(100));
        assert!(ds.is_active_at(u64::MAX));
    }

    #[test]
    fn bounded_sources_stop_at_their_end_height() {
        let ds = source(100, Some(200));
        assert!(ds.is_active_at(200));
        assert!(!ds.is_active_at(201));
    }

    #[test]
    fn filters_are_selected_by_handler_kind() {
        let ds = source(0, None);
        assert_eq!(ds.filters_for(HandlerKind::Event).len(), 1);
        assert_eq!(ds.filters_for(HandlerKind::Block).len(), 1);
        assert_eq!(ds.filters_for(HandlerKind::Transaction).len(), 0);
    }

    #[test]
    fn dynamic_sources_record_where_they_came_from() {
        let ds = DataSource {
            origin: DataSourceOrigin::Dynamic {
                template: "Pair".into(),
                created_at: 5_000,
            },
            ..source(5_000, None)
        };
        // Replay must recreate it at exactly this height to stay deterministic.
        assert!(matches!(
            ds.origin,
            DataSourceOrigin::Dynamic {
                created_at: 5_000,
                ..
            }
        ));
    }
}
