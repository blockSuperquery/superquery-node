//! Checkpoints: the record of exactly how far indexing has got, and on which
//! branch of the chain.
//!
//! Two things are stored, and the distinction matters:
//!
//! - **`_superquery_metadata`** holds the single current checkpoint — the answer
//!   to "where do I resume?" after a restart.
//! - **`_superquery_blocks`** holds the recent header chain (height, hash,
//!   parent hash, finality). This is what makes reorg detection possible: without
//!   the parent links, a fork can be *noticed* but its common ancestor cannot be
//!   *found* (guide §3.6).
//!
//! Rows below the finalized height can be pruned; they can no longer be reorged.

use superquery_chain_api::{BlockPtr, FinalityState, Header};

use crate::error::Result;
use crate::metadata::{keys, MetadataStore};
use crate::postgres::{validate_ident, Database};

/// Name of the header-chain table.
pub const BLOCKS_TABLE: &str = "_superquery_blocks";

/// Where indexing has reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// Last fully-committed block.
    pub indexed: BlockPtr,
    /// Highest height known to be final at the time of the write.
    pub finalized_height: u64,
}

/// Reads and writes checkpoints and the recent header chain.
pub struct CheckpointStore {
    schema: String,
    metadata: MetadataStore,
}

impl CheckpointStore {
    /// Bind to a schema.
    pub fn new(schema: impl Into<String>) -> Result<Self> {
        let schema = schema.into();
        validate_ident(&schema)?;
        let metadata = MetadataStore::new(&schema)?;
        Ok(Self { schema, metadata })
    }

    /// DDL for the header-chain table.
    ///
    /// Indexed by hash as well as height because reorg handling looks up both
    /// directions: "what did I store at height H" and "do I know this hash".
    pub fn create_table_sql(&self) -> String {
        format!(
            "CREATE TABLE IF NOT EXISTS \"{schema}\".\"{BLOCKS_TABLE}\" (\n  \
               height bigint PRIMARY KEY,\n  \
               hash text NOT NULL,\n  \
               parent_hash text,\n  \
               finality_state text NOT NULL,\n  \
               indexed_at timestamptz NOT NULL DEFAULT now()\n\
             );\n\
             CREATE INDEX IF NOT EXISTS \"{BLOCKS_TABLE}_hash_idx\" \
               ON \"{schema}\".\"{BLOCKS_TABLE}\" (hash);",
            schema = self.schema
        )
    }

    /// Create the checkpoint tables if absent.
    pub async fn ensure_tables(&self, db: &Database) -> Result<()> {
        self.metadata.ensure_table(db).await?;
        db.batch_execute(&self.create_table_sql()).await
    }

    /// The current checkpoint, or `None` on a fresh schema.
    pub async fn load(&self, db: &Database) -> Result<Option<Checkpoint>> {
        let height = self.metadata.get_u64(db, keys::INDEXED_HEIGHT).await?;
        let hash = self.metadata.get(db, keys::INDEXED_BLOCK_HASH).await?;
        let finalized = self
            .metadata
            .get_u64(db, keys::FINALIZED_HEIGHT)
            .await?
            .unwrap_or(0);

        Ok(match (height, hash) {
            (Some(height), Some(hash)) => Some(Checkpoint {
                indexed: BlockPtr::new(height, hash),
                finalized_height: finalized,
            }),
            // Height without hash means an interrupted write; treat as no
            // checkpoint and re-index from the project start rather than trusting
            // a half-written position.
            _ => None,
        })
    }

    /// Record a committed block **inside the caller's transaction**.
    ///
    /// Taking `&Transaction` rather than `&Database` is the point: this call sits
    /// in the same commit as the block's entity writes, so a crash between the two
    /// is impossible (guide Milestone 10).
    pub async fn commit_tx(
        &self,
        tx: &tokio_postgres::Transaction<'_>,
        header: &Header,
        finality: FinalityState,
        finalized_height: u64,
    ) -> Result<()> {
        let sql = format!(
            "INSERT INTO \"{}\".\"{BLOCKS_TABLE}\" (height, hash, parent_hash, finality_state) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (height) DO UPDATE SET \
               hash = EXCLUDED.hash, \
               parent_hash = EXCLUDED.parent_hash, \
               finality_state = EXCLUDED.finality_state, \
               indexed_at = now()",
            self.schema
        );
        let height = header.height as i64;
        tx.execute(
            &sql,
            &[
                &height,
                &header.hash,
                &header.parent_hash,
                &finality_to_str(finality),
            ],
        )
        .await?;

        self.metadata
            .set_tx(tx, keys::INDEXED_HEIGHT, &header.height.to_string())
            .await?;
        self.metadata
            .set_tx(tx, keys::INDEXED_BLOCK_HASH, &header.hash)
            .await?;
        self.metadata
            .set_tx(tx, keys::FINALIZED_HEIGHT, &finalized_height.to_string())
            .await?;
        Ok(())
    }

    /// The stored header at `height`, if we indexed it.
    pub async fn header_at(&self, db: &Database, height: u64) -> Result<Option<StoredHeader>> {
        let sql = format!(
            "SELECT height, hash, parent_hash, finality_state \
             FROM \"{}\".\"{BLOCKS_TABLE}\" WHERE height = $1",
            self.schema
        );
        let rows = db.query(&sql, &[&(height as i64)]).await?;
        Ok(rows.first().map(row_to_stored_header))
    }

    /// Stored headers from `from` down to `to`, highest first.
    ///
    /// Reorg handling walks backwards from the tip looking for the last height
    /// whose stored hash still matches the canonical chain.
    pub async fn headers_descending(
        &self,
        db: &Database,
        from: u64,
        to: u64,
    ) -> Result<Vec<StoredHeader>> {
        let sql = format!(
            "SELECT height, hash, parent_hash, finality_state \
             FROM \"{}\".\"{BLOCKS_TABLE}\" \
             WHERE height <= $1 AND height >= $2 ORDER BY height DESC",
            self.schema
        );
        let rows = db.query(&sql, &[&(from as i64), &(to as i64)]).await?;
        Ok(rows.iter().map(row_to_stored_header).collect())
    }

    /// Delete stored headers above `height`, inside a transaction.
    ///
    /// Part of rewind: the discarded branch's headers go with its entity writes.
    pub async fn truncate_above_tx(
        &self,
        tx: &tokio_postgres::Transaction<'_>,
        height: u64,
    ) -> Result<u64> {
        let sql = format!(
            "DELETE FROM \"{}\".\"{BLOCKS_TABLE}\" WHERE height > $1",
            self.schema
        );
        Ok(tx.execute(&sql, &[&(height as i64)]).await?)
    }

    /// Drop finalized headers below `height`, which can no longer be reorged.
    pub async fn prune_below(&self, db: &Database, height: u64) -> Result<u64> {
        let sql = format!(
            "DELETE FROM \"{}\".\"{BLOCKS_TABLE}\" \
             WHERE height < $1 AND finality_state = 'final'",
            self.schema
        );
        db.execute(&sql, &[&(height as i64)]).await
    }
}

/// A header row read back from `_superquery_blocks`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredHeader {
    /// Block height.
    pub height: u64,
    /// Stored block hash.
    pub hash: String,
    /// Stored parent hash.
    pub parent_hash: Option<String>,
    /// Finality as recorded at index time.
    pub finality: FinalityState,
}

impl StoredHeader {
    /// This row as a [`BlockPtr`].
    pub fn ptr(&self) -> BlockPtr {
        BlockPtr::new(self.height, self.hash.clone())
    }
}

fn row_to_stored_header(row: &tokio_postgres::Row) -> StoredHeader {
    let height: i64 = row.get("height");
    let state: String = row.get("finality_state");
    StoredHeader {
        height: height as u64,
        hash: row.get("hash"),
        parent_hash: row.get("parent_hash"),
        finality: finality_from_str(&state),
    }
}

fn finality_to_str(state: FinalityState) -> &'static str {
    match state {
        FinalityState::Final => "final",
        FinalityState::Unfinalized => "unfinalized",
    }
}

fn finality_from_str(raw: &str) -> FinalityState {
    match raw {
        "final" => FinalityState::Final,
        // Anything unrecognised is treated as reorg-able. Assuming finality on a
        // value we do not understand would suppress a rewind that should happen.
        _ => FinalityState::Unfinalized,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_ddl_records_lineage_and_indexes_hash() {
        let c = CheckpointStore::new("app").unwrap();
        let sql = c.create_table_sql();
        assert!(sql.contains("\"app\".\"_superquery_blocks\""));
        // parent_hash is what makes common-ancestor search possible.
        assert!(sql.contains("parent_hash text"));
        assert!(sql.contains("height bigint PRIMARY KEY"));
        assert!(sql.contains("_superquery_blocks_hash_idx"));
    }

    #[test]
    fn finality_round_trips() {
        for state in [FinalityState::Final, FinalityState::Unfinalized] {
            assert_eq!(finality_from_str(finality_to_str(state)), state);
        }
    }

    #[test]
    fn unknown_finality_is_treated_as_reorgable() {
        // Fail safe: an unrecognised value must not suppress a rewind.
        assert_eq!(finality_from_str("garbage"), FinalityState::Unfinalized);
        assert_eq!(finality_from_str(""), FinalityState::Unfinalized);
    }

    #[test]
    fn stored_header_exposes_a_ptr() {
        let h = StoredHeader {
            height: 12,
            hash: "0xabc".into(),
            parent_hash: Some("0xaaa".into()),
            finality: FinalityState::Final,
        };
        assert_eq!(h.ptr(), BlockPtr::new(12, "0xabc"));
    }
}
