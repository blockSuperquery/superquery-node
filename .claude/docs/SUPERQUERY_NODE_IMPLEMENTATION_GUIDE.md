# SuperQuery Node — Rust Implementation Guide

> Repository: `blockSuperquery/superquery-node`
> Role: The blockchain ingestion and indexing engine.
> Upstream reference: SubQuery's chain-agnostic node core + chain-specific node implementations.
>
> **Source:** provided by the project owner, 2026-09-09. This is the authoritative
> architecture brief for the Rust rewrite. Diagrams re-rendered from the original.

---

## 1. What this repository owns

`superquery-node` starts with blockchain RPC data and ends with durable indexed state in PostgreSQL.

```text
RPC
 |
 v
Chain adapter
 |
 v
Block fetcher / scheduler
 |
 v
Decoder + filters
 |
 v
Dispatcher / workers
 |
 v
Mapping runtime
 |
 v
Store / transaction
 |
 +-- entity tables
 +-- metadata
 +-- checkpoints
 +-- reorg history
```

It should own:

- blockchain RPC connectivity
- block/range fetching
- block, transaction, log/event decoding
- handler/filter matching
- block dispatch and worker scheduling
- deterministic mapping execution
- entity writes
- transaction boundaries
- checkpoints
- chain finality
- reorg detection and rewind
- dynamic data sources
- optional dictionary/accelerated fetching
- Proof-of-Index later
- health, metrics and tracing
- chain adapters for EVM, Stellar, Solana, etc.

It should **not** own:

- the developer CLI and project scaffolding: `superquery-sdk`
- public GraphQL reads: `superquery-query`
- frontend/dashboard code: keep under `superquery-sdk/web` while staying at exactly three repositories

---

## 2. Closest SubQuery equivalents

There is not one single upstream repository that equals `superquery-node`. The closest mapping is:

| SuperQuery | SubQuery reference |
|---|---|
| `superquery-node` core | [`subquery/subql/packages/node-core`](https://github.com/subquery/subql/tree/main/packages/node-core) |
| generic Substrate node wiring | [`subquery/subql/packages/node`](https://github.com/subquery/subql/tree/main/packages/node) |
| EVM node | [`subquery/subql-ethereum/packages/node`](https://github.com/subquery/subql-ethereum/tree/main/packages/node) |
| EVM common/config types | [`subquery/subql-ethereum/packages/common-ethereum`](https://github.com/subquery/subql-ethereum/tree/main/packages/common-ethereum) |
| EVM public types | [`subquery/subql-ethereum/packages/types`](https://github.com/subquery/subql-ethereum/tree/main/packages/types) |
| Stellar implementation | [`subquery/subql-stellar`](https://github.com/subquery/subql-stellar) |
| Solana implementation | [`subquery/subql-solana`](https://github.com/subquery/subql-solana) |

The architecture to imitate is:

```text
node-core                    chain-specific node
---------                    -------------------
scheduler        <---------- RPC/block implementation
dispatcher       <---------- chain block/event types
store            <---------- chain-specific decoding
sandbox/runtime  <---------- handler execution
reorg/finality   <---------- chain finality rules
```

For SuperQuery, make the interface explicit with a Rust `ChainAdapter` trait rather than coupling core logic to one blockchain.

---

## 3. Upstream source map

### 3.1 `packages/node-core/src`

Study: [`packages/node-core/src`](https://github.com/subquery/subql/tree/main/packages/node-core/src)

Important upstream directories:

- [`admin`](https://github.com/subquery/subql/tree/main/packages/node-core/src/admin)
- [`configure`](https://github.com/subquery/subql/tree/main/packages/node-core/src/configure)
- [`db`](https://github.com/subquery/subql/tree/main/packages/node-core/src/db)
- [`indexer`](https://github.com/subquery/subql/tree/main/packages/node-core/src/indexer)
- [`meta`](https://github.com/subquery/subql/tree/main/packages/node-core/src/meta)
- [`subcommands`](https://github.com/subquery/subql/tree/main/packages/node-core/src/subcommands)
- [`utils`](https://github.com/subquery/subql/tree/main/packages/node-core/src/utils)

Important top-level files:

- [`api.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/api.service.ts)
- [`blockchain.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/blockchain.service.ts)
- [`events.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/events.ts)
- [`process.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/process.ts)
- [`profiler.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/profiler.ts)

#### What to learn, not literally port

`node-core` is where SubQuery keeps chain-agnostic orchestration. For SuperQuery this becomes Rust traits/services around:

```rust
ChainAdapter
BlockFetcher
BlockDispatcher
MappingRuntime
EntityStore
CheckpointStore
FinalityTracker
ReorgManager
```

Avoid trying to reproduce NestJS dependency injection. Rust structs + traits + explicit constructors are cleaner.

---

### 3.2 Fetching and orchestration

Primary file: [`fetch.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/fetch.service.ts)

Also study:

- [`indexer.manager.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/indexer.manager.ts)
- [`project.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/project.service.ts)
- [`benchmark.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/benchmark.service.ts)

#### SuperQuery equivalent

```text
crates/core/src/
├── fetch/
│   ├── scheduler.rs
│   ├── range.rs
│   └── backpressure.rs
├── indexer/
│   ├── manager.rs
│   └── project.rs
└── metrics/
```

Build a scheduler that works in ranges rather than a naive single-block loop:

```text
latest safe height
       |
       v
calculate [start..end]
       |
       v
fetch concurrently
       |
       v
ordered dispatch
```

Rust stack: `tokio`, `futures`, `tokio::sync::mpsc`, `tokio::sync::Semaphore`, `tracing`.

---

### 3.3 Block dispatch

Upstream directory: [`packages/node-core/src/indexer/blockDispatcher`](https://github.com/subquery/subql/tree/main/packages/node-core/src/indexer/blockDispatcher)

Files:

- [`base-block-dispatcher.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/blockDispatcher/base-block-dispatcher.ts)
- [`block-dispatcher.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/blockDispatcher/block-dispatcher.ts)
- [`worker-block-dispatcher.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/blockDispatcher/worker-block-dispatcher.ts)
- [`factory.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/blockDispatcher/factory.ts)

Worker directory: [`packages/node-core/src/indexer/worker`](https://github.com/subquery/subql/tree/main/packages/node-core/src/indexer/worker)

#### SuperQuery equivalent

```text
crates/dispatcher/src/
├── dispatcher.rs
├── worker.rs
├── ordered_commit.rs
├── queue.rs
└── limits.rs
```

Important rule:

> Fetching may be concurrent, but database commits that affect deterministic
> indexed state must respect the ordering guarantees required by your project model.

Possible design:

```text
Fetcher 1 --+
Fetcher 2 --+--> bounded queue --> workers --> ordered commit
Fetcher N --+
```

Do not begin with distributed workers. First get a correct single-process Tokio implementation.

---

### 3.4 Store and persistence

Upstream: [`packages/node-core/src/indexer/store`](https://github.com/subquery/subql/tree/main/packages/node-core/src/indexer/store)

Files:

- [`entity.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/store/entity.ts)
- [`store.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/store/store.ts)
- [`store.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/store.service.ts)
- [`StoreOperations.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/StoreOperations.ts)

Database layer: [`packages/node-core/src/db`](https://github.com/subquery/subql/tree/main/packages/node-core/src/db)

- [`db.module.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/db/db.module.ts)
- [`sequelizeUtil.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/db/sequelizeUtil.ts)
- [`sync-helper.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/db/sync-helper.ts)

#### SuperQuery equivalent

Use a raw-SQL Postgres layer, not a Sequelize clone.

```text
crates/store/src/
├── postgres.rs
├── entity.rs
├── transaction.rs
├── metadata.rs
├── checkpoint.rs
├── migration.rs
└── rewind.rs
```

Core traits:

```rust
pub trait EntityStore {
    async fn get(&self, entity: &str, id: &str) -> Result<Option<EntityValue>>;
    async fn set(&self, entity: &str, id: &str, value: EntityValue) -> Result<()>;
    async fn remove(&self, entity: &str, id: &str) -> Result<()>;
}
```

Recommended internal metadata:

```text
_superquery_metadata
├── project_id
├── schema_version
├── manifest_hash
├── chain_id
├── indexed_height
├── finalized_height
├── indexed_block_hash
└── updated_at
```

Each block's entity mutations + checkpoint update should commit transactionally.

---

### 3.5 Mapping sandbox/runtime

SubQuery references:

- [`sandbox.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/sandbox.ts)
- [`sandbox.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/sandbox.service.ts)
- [`ds-processor.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/ds-processor.service.ts)

#### SuperQuery design

Do **not** reproduce a JavaScript VM merely to be compatible with SubQuery internals.

Use WebAssembly as a first-class SuperQuery design:

```text
Rust mapping project
      |
cargo build --target wasm32-wasip1
      |
      v
 mapping.wasm
      |
      v
  Wasmtime
      |
      +-- store_get
      +-- store_set
      +-- store_remove
      +-- log
      +-- controlled chain_call
```

```text
crates/runtime/src/
├── runtime.rs
├── wasmtime.rs
├── host.rs
├── memory.rs
├── limits.rs
└── ABI.md
```

Default-deny capabilities: filesystem, arbitrary network access, system clock,
uncontrolled randomness, environment variables.

Add: fuel/instruction limits, memory limit, execution timeout, deterministic serialization.

This is one of the strongest places where SuperQuery should be an independent Rust
design rather than a line-for-line port.

---

### 3.6 Unfinalized blocks and reorgs

Upstream:

- [`unfinalizedBlocks.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/unfinalizedBlocks.service.ts)
- [`multiChainRewind.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/multiChainRewind.service.ts)

EVM also has: [`subql-ethereum/packages/node/src/indexer/unfinalizedBlocks.service.ts`](https://github.com/subquery/subql-ethereum/blob/main/packages/node/src/indexer/unfinalizedBlocks.service.ts)

#### SuperQuery equivalent

```text
crates/core/src/finality/
├── tracker.rs
├── reorg.rs
└── rewind.rs
```

Store at least: `height`, `block_hash`, `parent_hash`, `finality_state`.

On a mismatch:

```text
stored hash for H  !=  canonical hash for H
       |
       v
find common ancestor
       |
       v
rewind entity mutations
       |
       v
re-index canonical branch
```

Build this before claiming production-grade indexing.

---

### 3.7 Dynamic data sources

Upstream: [`dynamic-ds.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/dynamic-ds.service.ts)

A project may discover a new contract while indexing and start watching it from that block onward.

```rust
pub struct DynamicDataSource {
    pub template: String,
    pub start_height: u64,
    pub parameters: serde_json::Value,
}
```

Persist dynamic data sources so restart/replay is deterministic.

---

### 3.8 Dictionary / optimized fetching

Upstream: [`packages/node-core/src/indexer/dictionary`](https://github.com/subquery/subql/tree/main/packages/node-core/src/indexer/dictionary)

Do not make this a v0 blocker. Start with RPC-only correctness. Then add an optional
`BlockCandidateProvider` abstraction:

```rust
trait BlockCandidateProvider {
    async fn candidate_heights(
        &self,
        from: u64,
        to: u64,
        filters: &[Filter],
    ) -> Result<Vec<u64>>;
}
```

---

### 3.9 Proof of Index

Upstream: [`packages/node-core/src/indexer/poi`](https://github.com/subquery/subql/tree/main/packages/node-core/src/indexer/poi)

Treat PoI as a later milestone after deterministic mapping + transactional store +
rewind are stable. Design the core so an `IndexingProofSink` can observe committed
block operations without coupling the entire store to PoI.

---

## 4. EVM implementation reference

SubQuery's EVM repository: [`subquery/subql-ethereum`](https://github.com/subquery/subql-ethereum)

Node source: [`packages/node/src`](https://github.com/subquery/subql-ethereum/tree/main/packages/node/src)

- [`blockchain.service.ts`](https://github.com/subquery/subql-ethereum/blob/main/packages/node/src/blockchain.service.ts)
- [`indexer`](https://github.com/subquery/subql-ethereum/tree/main/packages/node/src/indexer)
- [`indexer.manager.ts`](https://github.com/subquery/subql-ethereum/blob/main/packages/node/src/indexer/indexer.manager.ts)
- [`ethereum`](https://github.com/subquery/subql-ethereum/tree/main/packages/node/src/ethereum)
- [`configure`](https://github.com/subquery/subql-ethereum/tree/main/packages/node/src/configure)

### SuperQuery EVM adapter — use Alloy

```rust
#[async_trait::async_trait]
pub trait ChainAdapter: Send + Sync {
    type Block: Send + Sync;
    type Event: Send + Sync;

    fn network_id(&self) -> &str;

    async fn latest_height(&self) -> Result<u64>;
    async fn finalized_height(&self) -> Result<u64>;
    async fn fetch_block(&self, height: u64) -> Result<Self::Block>;
    async fn block_hash(&self, height: u64) -> Result<BlockHash>;
    async fn decode_events(&self, block: &Self::Block) -> Result<Vec<Self::Event>>;
}
```

For EVM: `alloy-provider`, `alloy-rpc-types`, `alloy-primitives`, `alloy-sol-types`, `alloy-json-abi`.

Do EVM first. Do not implement three chains concurrently.

---

## 5. Recommended final repository layout

```text
superquery-node/
├── Cargo.toml
├── README.md
├── crates/
│   ├── core/
│   │   └── src/
│   │       ├── fetch/
│   │       ├── indexer/
│   │       ├── finality/
│   │       └── project/
│   ├── chain-api/
│   │   └── src/
│   │       ├── adapter.rs
│   │       ├── block.rs
│   │       └── error.rs
│   ├── dispatcher/
│   ├── store/
│   ├── runtime/
│   ├── config/
│   └── chains/
│       ├── evm/
│       ├── stellar/
│       └── solana/
├── bins/
│   └── superquery-node/
├── migrations/
├── tests/
│   ├── fixtures/
│   ├── integration/
│   └── reorg/
└── docker/
```

---

## 6. Step-by-step implementation

### Milestone 0 — Define cross-repository contracts

Minimum stable model:

```rust
ProjectManifest
NetworkConfig
DataSource
Handler
HandlerKind
Filter
SchemaMetadata
EntityDefinition
FieldDefinition
```

Do not let node invent a second manifest representation.

Acceptance: a valid SDK manifest can be deserialized by node; incompatible manifest
versions fail with a clear message.

### Milestone 1 — Bootstrap node and configuration

```bash
superquery-node \
  --project ./project \
  --database-url postgres://... \
  --rpc-url https://...
```

Study: [`configure`](https://github.com/subquery/subql/tree/main/packages/node-core/src/configure), [`process.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/process.ts)

Acceptance: parse config; connect Postgres; connect RPC; load project; print
chain/project info; shutdown cleanly on SIGINT/SIGTERM.

### Milestone 2 — PostgreSQL project store

Implement: project schema creation, `_superquery_metadata`, entity persistence
abstraction, transaction per committed block, schema version check.

Acceptance: start with clean DB; initialize schema; set/get/delete one generated
entity; restart without losing indexed height.

### Milestone 3 — `ChainAdapter`

Acceptance: fake chain adapter passes core unit tests; core crate has no Alloy
imports; EVM details live only under `chains/evm`.

### Milestone 4 — EVM RPC and block ingestion

Implement: chain ID validation, latest/finalized height, fetch block, fetch
receipts/logs as required, bounded retry, RPC timeout.

Acceptance: index a selected Ethereum-compatible RPC range; record height/hash/parent hash.

### Milestone 5 — Fetch scheduler

Implement: start block, target/safe block, configurable batch size, bounded
in-flight requests, retry/backoff, graceful pause.

Acceptance: catch up 1,000 test blocks without unbounded RAM growth; RPC failures resume safely.

### Milestone 6 — Handler filtering and decoding

Implement EVM block/transaction/log handlers, address filter, topic/signature filter.
Do filtering before invoking user mappings.

Acceptance: an ERC-20 Transfer project invokes its handler only for matching logs.

### Milestone 7 — Dispatcher + ordered commit

Implement: bounded channel, worker pool, deterministic block lifecycle, ordered state commit.

Acceptance: varying fetch completion order yields the same final DB state.

### Milestone 8 — Mapping ABI

```text
sq_store_get
sq_store_set
sq_store_remove
sq_log
sq_chain_call
```

Version it: `superquery_mapping_abi = 1`

Acceptance: minimal hand-written WASM guest can save an entity through host functions.

### Milestone 9 — Wasmtime runtime

Implement: module loading, host functions, memory/fuel limits, timeout,
deterministic payload serialization, per-handler error boundaries.

Acceptance: SDK-generated Rust mapping WASM handles an ERC-20 Transfer and writes to
DB; infinite-loop mapping is terminated.

### Milestone 10 — Atomic block commits/checkpoints

```text
BEGIN
  run handlers
  persist entity mutations
  persist dynamic DS changes
  persist block hash/checkpoint
  update indexed height
COMMIT
```

Acceptance: process kill during handler execution never leaves a half-indexed block.

### Milestone 11 — Reorg handling

Implement: canonical block hash check, common ancestor search, historical mutation
log/versioned entities, rewind, replay.

Acceptance: synthetic 3-block fork produces same state as indexing canonical branch from scratch.

### Milestone 12 — Dynamic data sources

Acceptance: factory event creates a new contract datasource; datasource survives
restart; replay creates it at the same deterministic height.

### Milestone 13 — Metrics/admin

Add `/health`, `/ready`, `/metrics`. Track indexed height, chain target height,
blocks/sec, handler time, DB commit time, RPC errors, queue depth, reorg count.

### Milestone 14 — Dictionary optimization

Acceptance: dictionary-assisted run and raw-RPC run yield identical DB state.

### Milestone 15 — Additional chains

Order: EVM, Stellar, Solana. Each new chain implements `ChainAdapter` and
chain-specific filters/decoders without changing dispatcher/store/runtime fundamentals.

---

## 7. V0.1 definition of done

```text
EVM RPC -> ERC-20 Transfer filtering -> Rust WASM mapping
        -> PostgreSQL entity -> checkpoint/restart
```

```bash
superquery init --chain evm erc20-indexer
superquery build

superquery-node \
  --project ./erc20-indexer/dist \
  --database-url "$DATABASE_URL"
```

Then query the result through `superquery-query`.

---

## 8. Rust dependencies

```toml
tokio
futures
async-trait
serde
serde_json
serde_yaml
thiserror
anyhow
tracing
tracing-subscriber
sqlx
alloy
wasmtime
axum
tower
prometheus-client
```

Avoid adding distributed systems dependencies until the single-node engine is correct.

---

## 9. What not to port blindly

Do not blindly reproduce: NestJS module structure, Sequelize patterns, the
JavaScript sandbox implementation, package-level dependency wiring, the worker
model merely because upstream uses it, PostGraphile assumptions that belong to the
query service.

Prefer:

```text
SubQuery behavior/specification -> explicit Rust trait -> idiomatic Rust implementation
```

---

## 10. Grant-friendly issue sequence

1. Define `ChainAdapter` abstraction
2. Implement Alloy EVM adapter
3. Add bounded block-range scheduler
4. Add address/topic log filters
5. Add ordered block dispatcher
6. Add PostgreSQL checkpoint store
7. Define Mapping ABI v1
8. Add Wasmtime host runtime
9. Add runtime fuel/memory limits
10. Add atomic block transaction commits
11. Add canonical hash/reorg detector
12. Add rewind mutation log
13. Add dynamic data sources
14. Add Prometheus metrics
15. Add dictionary candidate provider
16. Add Stellar adapter
17. Add Solana adapter

---

## 11. Cross-repository contract

```text
superquery-sdk
  |
  +-- manifest types
  +-- schema IR
  +-- mapping ABI version
          |
          v
superquery-node --writes--> PostgreSQL --> superquery-query
```

Rules:

- SDK owns the public project specification.
- Node owns indexed-state writes and migration/checkpoint semantics.
- Query is read-only toward indexed project data.
- Mapping ABI and manifest versions are explicit.
- Changes that break a contract require a version bump.

---

## 12. Source-license note

Use the linked SubQuery code primarily as an architectural and behavioral reference.
Before copying source text or substantial implementation code, verify the license and
notices that apply to the exact upstream repository/package/version you are using. An
independent Rust implementation is cleaner technically and legally than a mechanical
translation.
