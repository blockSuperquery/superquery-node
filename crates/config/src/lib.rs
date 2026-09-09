//! # superquery-config
//!
//! Configuration for the indexer node: the CLI surface, environment variables and
//! their defaults.
//!
//! Kept in its own crate so the store, core and binary all agree on settings
//! without depending on each other.
//!
//! Upstream analogues: `node-core/src/configure/NodeConfig.ts` and
//! `node-core/src/db/db.module.ts`.

pub mod db;
pub mod node;

pub use db::{DbConfig, DbConfigError};
pub use node::{HistoricalMode, NodeConfig};
