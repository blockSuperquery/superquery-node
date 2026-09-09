//! Mapping workers.
//!
//! A worker takes a block off the queue, runs the project's handlers against it,
//! and reports the resulting operations back to the dispatcher — which decides
//! when they commit ([`crate::ordered_commit`]).
//!
//! Deliberately **in-process Tokio tasks**, not separate processes. Guide §3.3 is
//! explicit: "Do not begin with distributed workers. First get a correct
//! single-process Tokio implementation." SubQuery's worker threads exist to escape
//! JavaScript's single-threaded execution; Rust has no such constraint, so the
//! complexity would buy nothing until profiling says otherwise.

use superquery_core::ProcessedBlock;

/// What a worker produced for one block.
#[derive(Debug)]
pub struct WorkerOutput {
    /// Height processed.
    pub height: u64,
    /// The writes the handlers produced.
    pub processed: ProcessedBlock,
}

/// How a worker finished.
#[derive(Debug)]
pub enum WorkerResult {
    /// Handlers ran; here is what they produced.
    Completed(Box<WorkerOutput>),
    /// A handler failed. The pipeline must stop rather than skip the block:
    /// committing later blocks over a gap would produce state that no replay
    /// reproduces.
    Failed {
        /// Height that failed.
        height: u64,
        /// What went wrong.
        error: String,
    },
}

impl WorkerResult {
    /// The height this result concerns.
    pub fn height(&self) -> u64 {
        match self {
            WorkerResult::Completed(output) => output.height,
            WorkerResult::Failed { height, .. } => *height,
        }
    }

    /// Whether the block was processed successfully.
    pub fn is_success(&self) -> bool {
        matches!(self, WorkerResult::Completed(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_report_their_height_either_way() {
        let ok = WorkerResult::Completed(Box::new(WorkerOutput {
            height: 100,
            processed: ProcessedBlock::default(),
        }));
        assert_eq!(ok.height(), 100);
        assert!(ok.is_success());

        let failed = WorkerResult::Failed {
            height: 101,
            error: "handler panicked".into(),
        };
        assert_eq!(failed.height(), 101);
        assert!(!failed.is_success());
    }
}
