//! # superquery-dispatcher
//!
//! Moves fetched blocks through mapping to committed state, allowing concurrency
//! everywhere it is safe and forbidding it at the one place it is not.
//!
//! ```text
//! Fetcher 1 --+
//! Fetcher 2 --+--> bounded queue --> workers --> ordered commit --> Postgres
//! Fetcher N --+
//! ```
//!
//! ## The rule this crate exists to enforce
//!
//! Handlers read state that earlier blocks wrote, so committing block 101 before
//! block 100 yields a different database than indexing the two in sequence. Guide
//! Milestone 7's acceptance is exactly this: *varying fetch completion order must
//! yield the same final DB state*.
//!
//! [`ordered_commit::OrderedCommitBuffer`] is where that guarantee lives, and its
//! tests check it exhaustively over every arrival permutation.

pub mod dispatcher;
pub mod error;
pub mod limits;
pub mod ordered_commit;
pub mod queue;
pub mod worker;

pub use dispatcher::{BlockCommitter, BlockDispatcher, DispatchStatus, TokioDispatcher};
pub use error::{DispatchError, Result};
pub use limits::DispatchLimits;
pub use ordered_commit::OrderedCommitBuffer;
pub use queue::{queue, QueueReceiver, QueueSender, QueuedBlock};
pub use worker::{WorkerOutput, WorkerResult};
