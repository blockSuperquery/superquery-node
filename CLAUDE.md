# CLAUDE.md

Guidance for Claude Code (claude.ai/code) working in this repository.

## Plan and review

### Before starting work

- Work in planning mode first and produce a plan.
- Write the plan to `./.claude/tasks/TASK_NAME.md`.
- The plan should be a detailed implementation with the reasoning behind it and
  the tasks broken down.
- If the task needs external knowledge or current dependency versions, research
  it rather than guessing.
- Don't over-plan; design an MVP.
- Ask for review of the plan before continuing. Do not proceed until approved.

### While implementing

- Keep the plan updated as work proceeds.
- After completing tasks, append a description of the changes to the plan so the
  work can be handed to another engineer.

## Project

`superquery-node` is the SuperQuery blockchain indexing engine: blockchain RPC in,
indexed state in PostgreSQL out. Pure Rust, Cargo workspace.

It is one of three repositories — `superquery-sdk` owns the project
specification and developer CLI, `superquery-query` owns the public GraphQL read
API, and this repository owns ingestion and indexed-state writes.

**Read these before making architectural decisions:**

- [`.claude/docs/SUPERQUERY_NODE_IMPLEMENTATION_GUIDE.md`](.claude/docs/SUPERQUERY_NODE_IMPLEMENTATION_GUIDE.md)
  — the authoritative architecture brief and milestone sequence.
- [`.claude/docs/SUBQUERY_REFERENCE_MAP.md`](.claude/docs/SUBQUERY_REFERENCE_MAP.md)
  — per-module pointers into the upstream SubQuery sources.
- [`.claude/tasks/superquery-node-rust-scaffold.md`](.claude/tasks/superquery-node-rust-scaffold.md)
  — the current plan, deviations from the guide, and progress log.

## Commands

```bash
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace --all-targets -- -D warnings   # must stay clean
cargo fmt --all
cargo doc --workspace --no-deps                          # intra-doc links must resolve
```

Store integration tests skip when no database is reachable. To run them:

```bash
docker compose -f docker/docker-compose.yml up -d
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/superquery
cargo test --workspace
```

Run the node:

```bash
cargo run --bin superquery-node -- \
  --project ./path/to/project --database-url "$DATABASE_URL" --rpc-url https://...
```

## Architecture

```text
crates/
├── chain-api/        ChainAdapter trait, Header, Filter — no chain SDKs
├── config/           CLI surface, environment, defaults
├── store/            PostgreSQL: entities, metadata, checkpoints, rewind
├── core/             engine: fetch, indexer, finality, project
├── dispatcher/       bounded queue, workers, ordered commit
├── runtime/          WebAssembly mapping sandbox (+ ABI.md)
└── chains/evm/       the EVM adapter — the only crate that may use alloy

bins/superquery-node/ the binary and its composition root
migrations/           the node's own bookkeeping tables
tests/                cross-crate and workspace-invariant tests
```

Dependency direction:

```text
chain-api  <- core, dispatcher, chains/*, bins
config     <- store, core, bins
store      <- core, dispatcher, bins
runtime    <- core, bins
core       <- dispatcher, bins
chains/*   <- bins only
```

### Invariants

These are enforced by tests, not just convention. Breaking one fails the build.

1. **`superquery-core` never depends on a chain SDK.** Everything it knows about
   blockchains comes through `superquery-chain-api`. Adding a chain means adding a
   `crates/chains/*` crate, never editing the engine.
   (`tests/integration/crate_boundaries.rs`)

2. **Commits are ordered by height, whatever order blocks arrive in.** Handlers
   read what earlier blocks wrote, so out-of-order commits produce a different
   database. `crates/dispatcher/src/ordered_commit.rs`.

3. **A block's entity writes and its checkpoint commit in one transaction.**
   A killed process must never leave a half-indexed block.
   `Database::transaction` + `CheckpointStore::commit_tx`.

4. **Generations, not heights, distinguish branches.** After a reorg, work in
   flight from the abandoned branch has plausible heights. `reset_to` bumps a
   generation counter so stale output cannot commit.

## Conventions

- **Milestone-honest stubs.** Unimplemented seams `unimplemented!()` or return an
  error naming their guide milestone and task-plan phase. Never silently return a
  wrong-but-plausible value.
- **Doc comments explain *why*.** `missing_docs` is a warning and clippy runs with
  `-D warnings`, so every public item is documented. Say what a reader cannot
  infer from the signature.
- **Tests name the behaviour they protect.** `a_stale_block_cannot_commit_on_the_new_branch`,
  not `test_reset`. Comment the hazard a non-obvious assertion guards against.
- **Compatibility-critical values are cited.** Where a constant must match
  upstream (table naming, `Set`/`Remove` PoI strings, `_block_range`), cite the
  SubQuery file at the use site.
- **SQL identifiers are validated, never interpolated blind.** Identifiers cannot
  be bound parameters, so everything reaching SQL text goes through
  `postgres::validate_ident`.

## Relationship to SubQuery

SuperQuery's architecture follows [SubQuery](https://github.com/subquery/subql).
This is an independent Rust implementation of that *behaviour*, not a translation
of its TypeScript — see guide §9 and §12. Both projects are GPL-3.0.

Use upstream as a specification. Do not reproduce NestJS module structure,
Sequelize patterns, the JavaScript sandbox, or the worker model merely because
upstream has them.
