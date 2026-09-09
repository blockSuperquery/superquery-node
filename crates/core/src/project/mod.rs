//! The project being indexed: its data sources, and how they change with height.
//!
//! Upstream analogue: `node-core/src/indexer/project.service.ts`.

pub mod block_height_map;
pub mod datasource;

pub use block_height_map::{BlockHeightMap, EntryNotFoundError, GetRange};
pub use datasource::{DataSource, DataSourceOrigin, DynamicDataSource, Handler};
