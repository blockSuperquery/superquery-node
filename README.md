<p align="center">
  <img src="docs/assets/superquery-wordmark.svg#gh-light-mode-only" alt="SuperQuery" height="64">
  <img src="docs/assets/superquery-wordmark-dark.svg#gh-dark-mode-only" alt="SuperQuery" height="64">
</p>

<p align="center"><strong>The SuperQuery blockchain indexing engine.</strong></p>

<p align="center">
  <a href="#status"><img src="https://img.shields.io/badge/status-pre--alpha-orange" alt="Status: pre-alpha"></a>
  <img src="https://img.shields.io/badge/rust-1.82%2B-93450a" alt="Rust 1.82+">
  <img src="https://img.shields.io/badge/license-GPL--3.0-blue" alt="GPL-3.0">
</p>

---

`superquery-node` takes blockchain RPC data at one end and produces durable
indexed state in PostgreSQL at the other.

```text
RPC → chain adapter → fetch scheduler → decode + filter → dispatcher
    → mapping runtime (WASM) → store → { entities, metadata, checkpoints, reorg history }
```

It is one of three repositories:

| Repository | Owns |
|---|---|
| `superquery-sdk` | the project specification, developer CLI, scaffolding |
| **`superquery-node`** | **ingestion, indexing, indexed-state writes** |
| `superquery-query` | the public GraphQL read API |

---

## Status

**Pre-alpha.** The workspace, the crate boundaries and the correctness-critical
logic are in place and tested; the pipeline that connects them is not yet wired.

| Area | State |
|---|---|
| Workspace, crate seams, `ChainAdapter` | done |
| Configuration + CLI, startup, graceful shutdown | done |
| Store: schema generation, entities, metadata, checkpoints | done |
| Fetch range planning, backpressure | done |
| Ordered commit (out-of-order → in-order) | done |
| Reorg detection, common-ancestor search | done |
| EVM log/transaction filtering | done |
| Mapping ABI v1 | specified |
| EVM RPC ingestion | **not yet** — Milestone 4 |
| Scheduler loop, worker pool | **not yet** — Milestones 5, 7 |
| Wasmtime host functions | **not yet** — Milestone 9 |
| Rewind execution, dynamic data sources | **not yet** — Milestones 11, 12 |

Unimplemented seams return a clear error naming their milestone rather than
failing obscurely. The roadmap is
[`.claude/tasks/superquery-node-rust-scaffold.md`](.claude/tasks/superquery-node-rust-scaffold.md).

---

## Layout

```text
crates/
├── chain-api/        the ChainAdapter trait — no chain SDKs, ever
├── config/           CLI surface, environment, defaults
├── store/            PostgreSQL: entities, metadata, checkpoints, rewind
├── core/             the engine: fetch, indexer, finality, project
├── dispatcher/       bounded queue, workers, ordered commit
├── runtime/          WebAssembly mapping sandbox (+ ABI.md)
└── chains/
    └── evm/          the EVM adapter — the only crate that may use alloy

bins/superquery-node/ the binary
migrations/           the node's own bookkeeping tables
tests/                cross-crate and workspace-invariant tests
docker/               local Postgres, and the release image
```

### The one architectural rule

`superquery-core` never depends on a chain SDK. Everything it knows about
blockchains arrives through `superquery-chain-api`, and a chain's specifics live
in a `crates/chains/*` adapter. Adding a chain means adding a crate, not editing
the engine.

This is enforced, not merely documented — see
[`tests/integration/crate_boundaries.rs`](tests/integration/crate_boundaries.rs).

---

## Building

Needs a recent stable Rust (MSRV 1.82).

```bash
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The store's integration tests skip when no database is reachable, so `cargo test`
passes on a bare checkout. To run them:

```bash
docker compose -f docker/docker-compose.yml up -d
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/superquery
cargo test --workspace
```

---

## Running

```bash
superquery-node \
  --project ./erc20-indexer/dist \
  --database-url "$DATABASE_URL" \
  --rpc-url https://eth-mainnet.example
```

`--help` lists every flag. The three above are the spine; everything else has a
default that lets that command work.

Startup is fail-fast: the project path, database configuration, connectivity and
RPC endpoints are all checked before a single block is fetched.

---

## Design notes

A few decisions that are not obvious from the code:

**WebAssembly, not a JavaScript VM, for mappings.** Deterministic execution,
enforceable fuel/memory limits, and default-deny capabilities — none of which a JS
sandbox gives without cooperation from the guest. See
[`crates/runtime/ABI.md`](crates/runtime/ABI.md).

**Raw SQL, not an ORM.** Entity tables are generated at runtime from each
project's GraphQL schema, so there is nothing static for a query builder to check.

**Concurrent fetching, serialised commits.** Handlers read what earlier blocks
wrote, so commit order must be block order regardless of network delivery order.
`crates/dispatcher/src/ordered_commit.rs` is where that guarantee lives, with
generation counters so a reorg cannot let an abandoned branch commit late.

---

## Relationship to SubQuery

SuperQuery's architecture follows [SubQuery](https://github.com/subquery/subql)'s,
which is a mature and well-shaped design for this problem. This is an independent
Rust implementation of that *behaviour*, not a translation of its TypeScript.

Per-module pointers into the upstream sources are in
[`.claude/docs/SUBQUERY_REFERENCE_MAP.md`](.claude/docs/SUBQUERY_REFERENCE_MAP.md).
Both projects are GPL-3.0.

---

## Contributing

See [contributing.md](contributing.md) and
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## License

[GPL-3.0](LICENSE).
