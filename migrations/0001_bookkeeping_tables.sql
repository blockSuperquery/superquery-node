-- 0001: the node's bookkeeping tables.
--
-- Created inside the project's schema, alongside its generated entity tables.
-- `{{schema}}` is substituted with the validated project schema name at apply
-- time (identifiers cannot be bound parameters in DDL).
--
-- Guide §3.4.

-- Project identity and the current checkpoint, one key per fact.
--
-- Key/value rather than a single wide row so adding a fact needs no migration,
-- and so a partially-written row cannot exist.
CREATE TABLE IF NOT EXISTS "{{schema}}"."_superquery_metadata" (
  key        text PRIMARY KEY,
  value      text NOT NULL,
  updated_at timestamptz NOT NULL DEFAULT now()
);

-- The recent header chain, which is what makes reorg detection possible.
--
-- Without `parent_hash` a fork can be noticed but its common ancestor cannot be
-- located, so a rewind would have no defensible target.
CREATE TABLE IF NOT EXISTS "{{schema}}"."_superquery_blocks" (
  height         bigint PRIMARY KEY,
  hash           text NOT NULL,
  parent_hash    text,
  finality_state text NOT NULL,
  indexed_at     timestamptz NOT NULL DEFAULT now()
);

-- Reorg handling looks up in both directions: "what did I store at height H"
-- (the primary key) and "do I know this hash" (this index).
CREATE INDEX IF NOT EXISTS "_superquery_blocks_hash_idx"
  ON "{{schema}}"."_superquery_blocks" (hash);
