# Migrations

Versioned SQL for the node's **own** bookkeeping tables — `_superquery_metadata`
and `_superquery_blocks`.

Entity tables are **not** here. They are generated at runtime from each project's
GraphQL schema (`crates/store/src/ddl.rs`), because their shape is the project's,
not ours.

## Naming

```text
NNNN_short_description.sql
```

Four-digit zero-padded sequence, applied in ascending order. Never renumber or
edit a migration that has shipped — write a new one.

## Why plain SQL

The set is small, the ordering is obvious, and anyone auditing what this node does
to their database can read it without knowing a migration framework. See the task
plan §4.1 for the surrounding decision.

## Applying

`superquery-store`'s `migration::initialize_schema` runs these against the
project's schema at startup. Every statement must be idempotent (`IF NOT EXISTS`,
`ON CONFLICT DO NOTHING`) so a restart mid-upgrade is safe.
