//! Block fetching: what to fetch, how fast, and from where.
//!
//! - [`range`] decides *which* heights, given the chain head and the project's
//!   bypass rules.
//! - [`backpressure`] decides *how many* at once, so a fast fetcher cannot
//!   outrun a slower mapping pipeline.
//! - [`scheduler`] puts the two together and drives the loop.

pub mod backpressure;
pub mod range;
pub mod scheduler;

pub use backpressure::Backpressure;
pub use range::{safe_head, BlockRange, RangePlan};
pub use scheduler::{FetchScheduler, SchedulerConfig};
