# Task: SuperQuery Node — Rust rewrite scaffold

**Status:** scaffold complete (Milestones 0–3 partially landed), see §7 Progress Log
**Owner:** @0xMegie
**Created:** 2026-09-09
**Reference:** [`.claude/docs/SUPERQUERY_NODE_IMPLEMENTATION_GUIDE.md`](../docs/SUPERQUERY_NODE_IMPLEMENTATION_GUIDE.md),
[`.claude/docs/SUBQUERY_REFERENCE_MAP.md`](../docs/SUBQUERY_REFERENCE_MAP.md)

---

## 1. Objective

Replace the inherited SubQuery TypeScript monorepo with a pure-Rust Cargo workspace
matching the architecture in the implementation guide: a chain-agnostic indexing
engine (`core` + `chain-api` + `dispatcher` + `store` + `runtime`) with chain support
supplied by adapter crates, EVM first.

**In scope for this task:** the workspace skeleton, crate boundaries, the trait seams,
and every foundation that can be built and tested without a live chain or database.

**Out of scope:** the full indexing pipeline. Milestones 4–15 are follow-on work.

---

## 2. Starting position

The repository contained two overlapping things:

1. The **SubQuery TypeScript monorepo** (`packages/`, 764 files) — the upstream fork.
2. A **partial Rust port** (`crates/subql-*`, ~2.4k lines) structured as a
   *strangler-fig port of the TS packages* — crate-per-TS-package, Substrate-first.

The guide asks for a different shape: SuperQuery-native crates organised by
*engine responsibility*, not by upstream package, and EVM-first rather than
Substrate-first. So (1) is deleted and (2) is **restructured, not discarded** —
roughly 1.9k lines of it is directly reusable and already has passing tests.

### Reuse decision

| Existing code | Decision | Destination |
|---|---|---|
| `subql-store/naming.rs` (inflection port) | **keep** — compatibility-critical, tested | `store/naming.rs` |
| `subql-store/schema.rs` (SDL → entities) | **keep** | `store/schema.rs` |
| `subql-store/ddl.rs` (`CREATE TABLE`) | **keep**, extend for `_block_range` later | `store/ddl.rs` |
| `subql-store/introspect.rs` | **keep** — schema-parity differ | `store/introspect.rs` |
| `subql-store/model.rs` (`PlainModel`) | **keep** | `store/entity.rs` |
| `subql-store/db.rs` (pool) | **keep**, rename | `store/postgres.rs` |
| `subql-config/*` | **keep**, re-scope to guide's CLI surface | `config/` |
| `subql-node-core/types/*` | **keep** (`Header`, `BlockHeightMap`, operations) | `chain-api/`, `core/project/` |
| `subql-node-core/traits/*` | **rewrite** into `ChainAdapter` + `EntityStore` seams | `chain-api/`, `store/` |
| `subql-node-core/rpc.rs` | **drop** — superseded by the Alloy EVM adapter |  |
| `subql-{cli,query,types,utils,common*}` | **drop** — those responsibilities belong to `superquery-sdk` / `superquery-query` | |

Everything deleted stays recoverable in git history at `91ebf6c`.

---

## 3. Target layout

Follows guide §5. Directory names are as the guide specifies; Cargo package names
are prefixed `superquery-` so they are unambiguous on crates.io.

```text
superquery-node/
├── Cargo.toml                     # workspace, shared dep pins, lints
├── rust-toolchain.toml
├── crates/
│   ├── chain-api/                 # superquery-chain-api  (no chain deps)
│   ├── config/                    # superquery-config
│   ├── store/                     # superquery-store
│   ├── core/                      # superquery-core
│   ├── dispatcher/                # superquery-dispatcher
│   ├── runtime/                   # superquery-runtime
│   └── chains/
│       └── evm/                   # superquery-chain-evm (alloy lives ONLY here)
├── bins/superquery-node/          # the binary
├── migrations/
├── tests/{fixtures,integration,reorg}/
└── docker/
```

### Dependency graph (enforced by Cargo)

```text
chain-api  <- core, dispatcher, chains/evm, bins
config     <- store, core, bins
store      <- core, dispatcher, bins
runtime    <- core, bins
core       <- dispatcher, bins
chains/evm <- bins            (nothing else may depend on a chain crate)
```

**Invariant (guide Milestone 3):** `core` must never import `alloy`. Enforced by a
test in `tests/integration/` that greps the manifests.

---

## 4. Deviations from the guide — with reasoning

Three deliberate departures. Each is a judgement call, flagged for review.

### 4.1 `tokio-postgres` + `deadpool` instead of `sqlx`

The guide's §8 dependency list suggests `sqlx`. We keep the existing
`tokio-postgres`/`deadpool-postgres` layer instead:

- sqlx's headline feature is **compile-time-checked queries**. Our entity SQL is
  generated at runtime from each project's GraphQL schema — there are no static
  queries to check, so the feature does not apply.
- The existing store code (~1.4k lines, with integration tests) already works on
  `tokio-postgres`. Rewriting it for sqlx buys nothing functional.
- `tokio-postgres` gives direct access to `COPY` and pipelining, which matter for
  bulk block ingestion.

Migrations use plain versioned `.sql` files under `migrations/` applied by our own
runner, rather than `sqlx::migrate!`.

**Revisit if:** we want a second SQL backend, or static query checking for the
non-entity (metadata/checkpoint) tables becomes valuable.

### 4.2 Directory `crates/chains/evm`, package `superquery-chain-evm`

The guide's tree shows `chains/evm`. Kept, but the *package* name is prefixed so
crates.io names never collide with a generic `evm`.

### 4.3 `wasmtime` pinned to 48.0.1

Latest on crates.io is `49.0.0-rc.1`. Pinned to the newest **stable** release.

---

## 5. Dependency pins (verified on crates.io 2026-09-09)

| Crate | Version | Notes |
|---|---|---|
| `tokio` | 1.53 | `full` features |
| `alloy` | 2.4 | EVM crate only |
| `wasmtime` | 48.0.1 | latest stable (49 is RC) |
| `axum` | 0.8 | admin/metrics server |
| `prometheus-client` | 0.25 | metrics |
| `tokio-postgres` | 0.7 | |
| `deadpool-postgres` | 0.14 | |
| `sqlx` | — | not used, see §4.1 |
| `graphql-parser` | 0.4 | SDL parsing |
| `thiserror` | 2.0 | |
| `clap` | 4.6 | derive + env |

---

## 6. Task breakdown

Mapped to guide §6 milestones and the §10 grant issue sequence.

### Phase A — scaffold (this task)

- [x] **A1** Delete the TypeScript monorepo and its toolchain
- [x] **A2** Workspace `Cargo.toml` with shared pins + lints; `rust-toolchain.toml`
- [x] **A3** `chain-api`: `ChainAdapter`, `Header`, `BlockPtr`, `ChainError`,
      `GenericBlock`/`IBlock`, `BlockCandidateProvider` — *guide Milestone 3, issue #1*
- [x] **A4** `config`: `NodeConfig` with the guide's CLI surface
      (`--project/--database-url/--rpc-url`), `DbConfig` incl. URL parsing — *Milestone 1*
- [x] **A5** `store`: port naming/schema/ddl/introspect/entity/postgres; add
      `metadata` (`_superquery_metadata`) and `checkpoint` — *Milestone 2, issue #6*
- [x] **A6** `core`: `fetch/range.rs` (real batch-range maths + tests),
      `fetch/backpressure.rs`, module seams for scheduler/indexer/finality/project — *Milestone 5, issue #3*
- [x] **A7** `dispatcher`: `ordered_commit.rs` reorder buffer (real logic + tests),
      queue/limits/worker seams — *Milestone 7, issue #5*
- [x] **A8** `runtime`: ABI v1 definition (`ABI.md`) + host-function signatures +
      `limits.rs` — *Milestone 8, issue #7*
- [x] **A9** `chains/evm`: adapter skeleton on alloy + **log filter matching**
      (address/topic, real logic + tests) — *Milestone 6, issues #2/#4*
- [x] **A10** `bins/superquery-node`: config parse, tracing, graceful SIGINT/SIGTERM
      shutdown — *Milestone 1 acceptance*
- [x] **A11** `migrations/`, `docker/`, `tests/` skeletons, README, CI workflow
- [x] **A12** `cargo build` + `cargo test` + `cargo clippy` green

### Phase B — first indexing path (next)

- [ ] **B1** Manifest types shared with `superquery-sdk` — *Milestone 0*
- [ ] **B2** Alloy EVM adapter: real `latest/finalized_height`, `fetch_block`,
      receipts/logs, bounded retry — *Milestone 4*
- [ ] **B3** Scheduler driving the adapter through the dispatcher — *Milestone 5*
- [ ] **B4** Entity DDL applied from project schema at startup — *Milestone 2*
- [ ] **B5** Atomic per-block commit (entities + checkpoint in one tx) — *Milestone 10*

### Phase C — runtime + correctness

- [ ] **C1** Wasmtime host: module load, host fns, fuel/memory/timeout — *Milestone 9*
- [ ] **C2** Reorg: canonical hash check, ancestor search, rewind, replay — *Milestone 11*
- [ ] **C3** Dynamic data sources, persisted — *Milestone 12*
- [ ] **C4** `/health`, `/ready`, `/metrics` — *Milestone 13*

### Phase D — scale

- [ ] **D1** Dictionary `BlockCandidateProvider` — *Milestone 14*
- [ ] **D2** Worker-pool dispatcher
- [ ] **D3** Proof of Index
- [ ] **D4** Stellar, then Solana adapters — *Milestone 15*

---

## 7. Progress log

### 2026-09-09 — Phase A: scaffold

Repository converted from the SubQuery TypeScript fork to a pure-Rust workspace.

**Removed.** `packages/` (764 files), `.yarn/`, `yarn.lock`, `package.json`,
`tsconfig*.json`, `jest.config.js`, `eslint.config.js`, `.prettier*`, `.husky/`,
`crowdin.yaml`, `report-main.json` (3 MB build artefact), `test/` (JS harness),
`scripts/` (JS fixture generators), `.gitpod*`, TS-oriented GitHub workflows, and the
eleven `crates/subql-*` crates. All recoverable from git history at `91ebf6c`.

**Added.** Eight crates under the guide's §5 layout plus the binary. Notable content
beyond plain stubs — the pieces that carry real, tested logic:

- `store/naming.rs` — Sequelize `inflection` port (`underscored`, `pluralize`,
  `model_to_table_name`). Compatibility-critical; carried over intact.
- `store/schema.rs` — GraphQL SDL → `EntityModel`.
- `store/ddl.rs` — entity `CREATE TABLE` generation.
- `store/introspect.rs` — canonical schema introspection (the parity differ).
- `store/entity.rs` — `PlainModel` upsert/get/query/dump against Postgres.
- `store/metadata.rs` — `_superquery_metadata` contract from guide §3.4, typed
  accessors for `indexed_height` / `finalized_height` / `indexed_block_hash`.
- `store/checkpoint.rs` — `Checkpoint` type + the atomic commit boundary (guide §3.4).
- `core/fetch/range.rs` — batch-range calculation with bypass-range subtraction.
- `core/fetch/backpressure.rs` — in-flight/queue-depth admission control.
- `core/project/block_height_map.rs` — `BlockHeightMap` (carried over).
- `core/finality/` — `FinalityTracker`, reorg detection with common-ancestor search.
- `dispatcher/ordered_commit.rs` — reorder buffer turning out-of-order completions
  into a contiguous in-order commit sequence.
- `chains/evm/filter.rs` — address + topic/signature log matching.
- `runtime/ABI.md` + `runtime/host.rs` — mapping ABI v1 host-function surface.

**Verification.** All green as of 2026-09-09:

| Check | Result |
|---|---|
| `cargo build --workspace` | 0 warnings |
| `cargo test --workspace` | **182 passed, 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps` | clean |
| `superquery-node --help` | renders the Milestone 1 CLI |

The six Postgres integration tests were run against a **live database**, not
skipped — DDL generation, entity round-trip at u256 scale, metadata mismatch
detection, atomic commit *and rollback*, and the descending reorg header walk all
verified end-to-end. Both paths were checked: with an unreachable database they
skip with a notice instead of failing.

Startup validation was exercised by hand: a missing project path, an RPC URL with
no scheme, and `--end-height` below `--start-height` each fail with a message
naming the problem.

**Design changes made during implementation.**

- **Generation counters on the reorder buffer.** An integration test exposed a
  hazard the original design missed: after a rewind to 99 and a resume at 100, a
  block 101 still in flight *from the abandoned branch* has a perfectly plausible
  height, so height alone cannot reject it. `OrderedCommitBuffer` now bumps a
  generation on every `reset_to`, and `complete_from` discards output from an
  abandoned one. This makes reorg safety a property of the type rather than
  something the dispatcher must achieve by draining its queues perfectly.
- **Wasm relaxed-SIMD disabled explicitly.** Wasmtime refuses to disable the SIMD
  proposal while relaxed SIMD stays enabled. Relaxed SIMD is implementation-defined
  by design, so it had to go regardless — two nodes could otherwise index the same
  block differently.

**Known gaps (intentional).** The EVM adapter's RPC methods, the scheduler loop, the
Wasmtime host, and rewind execution are `unimplemented!()` or return an error naming
their milestone. They are seams, not silently-broken code — phases B and C fill them.

---

## 8. Acceptance for this task

- [x] `cargo build --workspace` succeeds
- [x] `cargo test --workspace` passes
- [x] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [x] No TypeScript, npm, or yarn artefacts remain
- [x] `core` does not depend on `alloy` (asserted by a test)
- [x] `superquery-node --help` renders the guide's CLI surface
- [x] Binary connects/reports and shuts down cleanly on SIGINT
