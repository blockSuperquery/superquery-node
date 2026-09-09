//! Per-block indexing: turning a fetched block into entity writes.

pub mod manager;

pub use manager::{IndexerManager, ProcessedBlock};
