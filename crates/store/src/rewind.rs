//! Rewind: undoing indexed state after a reorg.
//!
//! Guide Milestone 11. The store's half of the job is to make "return every table
//! to how it looked at height H" a real operation. Two strategies, chosen by
//! `--historical`:
//!
//! - **Versioned entities** (`HistoricalMode::Height`): each row carries the
//!   height range it was valid for. Rewinding deletes rows created above H and
//!   reopens rows that were closed above H. No re-indexing of surviving state.
//! - **Replay** (`HistoricalMode::Disabled`): no version information exists, so
//!   the only sound recovery is to drop entity data and re-index from a known-good
//!   height. Correct, but expensive — which is why history defaults to on.
//!
//! Upstream analogue: `node-core/src/indexer/multiChainRewind.service.ts`.

use superquery_chain_api::BlockPtr;

use crate::error::Result;

/// The column holding an entity version's validity range.
///
/// Name and `int8range` type match SubQuery's `sync-helper.ts`, so a database
/// written by either engine is readable by the other.
pub const BLOCK_RANGE_COLUMN: &str = "_block_range";

/// What a rewind did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewindOutcome {
    /// The block state was returned to.
    pub target: BlockPtr,
    /// Entity rows deleted (created above the target).
    pub rows_deleted: u64,
    /// Entity rows reopened (closed above the target).
    pub rows_reopened: u64,
    /// Checkpoint rows removed.
    pub headers_removed: u64,
}

/// Rewind entity state to `target`.
///
/// # Milestone
///
/// Not yet implemented — needs the versioned-entity write path
/// (`_block_range` on every entity table), which lands with guide Milestone 11.
/// The signature is fixed now so [`crate::checkpoint`] and the reorg detector in
/// `superquery-core` can be written against it.
pub async fn rewind_to(
    _db: &crate::postgres::Database,
    _target: &BlockPtr,
) -> Result<RewindOutcome> {
    unimplemented!(
        "rewind requires versioned entity writes (_block_range); guide Milestone 11, \
         task plan phase C2"
    )
}

/// SQL deleting entity versions created strictly above `height`.
///
/// Pure string generation, so the range algebra is testable before the write path
/// that produces these rows exists.
pub fn delete_versions_above_sql(schema: &str, table: &str, height: u64) -> String {
    format!(
        "DELETE FROM \"{schema}\".\"{table}\" \
         WHERE lower(\"{BLOCK_RANGE_COLUMN}\") > {height}"
    )
}

/// SQL reopening entity versions that were closed above `height`.
///
/// A row closed at height 105 was still current at 100, so rewinding to 100 must
/// make it current again — reopening the range rather than deleting the row.
pub fn reopen_versions_sql(schema: &str, table: &str, height: u64) -> String {
    format!(
        "UPDATE \"{schema}\".\"{table}\" \
         SET \"{BLOCK_RANGE_COLUMN}\" = int8range(lower(\"{BLOCK_RANGE_COLUMN}\"), NULL) \
         WHERE upper(\"{BLOCK_RANGE_COLUMN}\") > {height} \
           AND lower(\"{BLOCK_RANGE_COLUMN}\") <= {height}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_targets_versions_created_after_the_target() {
        let sql = delete_versions_above_sql("app", "transfers", 100);
        assert!(sql.contains("DELETE FROM \"app\".\"transfers\""));
        assert!(sql.contains("lower(\"_block_range\") > 100"));
    }

    #[test]
    fn reopen_only_touches_rows_that_were_current_at_the_target() {
        let sql = reopen_versions_sql("app", "transfers", 100);
        // Closed after the target...
        assert!(sql.contains("upper(\"_block_range\") > 100"));
        // ...but already open at it. Without this second clause the statement
        // would resurrect rows that were never current at height 100.
        assert!(sql.contains("lower(\"_block_range\") <= 100"));
        assert!(sql.contains("int8range(lower(\"_block_range\"), NULL)"));
    }

    #[test]
    fn block_range_column_matches_upstream() {
        // Compatibility with SubQuery's sync-helper.ts.
        assert_eq!(BLOCK_RANGE_COLUMN, "_block_range");
    }
}
