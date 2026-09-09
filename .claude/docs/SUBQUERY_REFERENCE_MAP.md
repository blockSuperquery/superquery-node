# SubQuery Reference Map

Per-module pointers into the upstream SubQuery GitHub sources. Use these as
**behavioural specification**, not as code to translate line-for-line (see
[the guide](./SUPERQUERY_NODE_IMPLEMENTATION_GUIDE.md) §9 and §12).

Upstream accounts and repositories:

| Repo | URL | Licence |
|---|---|---|
| `subquery/subql` | https://github.com/subquery/subql | GPL-3.0 |
| `subquery/subql-ethereum` | https://github.com/subquery/subql-ethereum | GPL-3.0 |
| `subquery/subql-stellar` | https://github.com/subquery/subql-stellar | GPL-3.0 |
| `subquery/subql-solana` | https://github.com/subquery/subql-solana | GPL-3.0 |
| `subquery/subql-cosmos` | https://github.com/subquery/subql-cosmos | GPL-3.0 |
| `subquery/subql-dictionary` | https://github.com/subquery/subql-dictionary | Apache-2.0 |

> **Licence note.** Upstream is GPL-3.0. This repository is also GPL-3.0, so
> reference is unproblematic — but prefer independent Rust implementations of the
> *behaviour* over transliterated code. Where a constant, string, or algorithm is
> load-bearing for compatibility (hash inputs, table naming, metadata keys), copy the
> value and cite the upstream file in a comment.

---

## Crate → upstream mapping

### `crates/chain-api` — `superquery-chain-api`

| Our item | Upstream |
|---|---|
| `ChainAdapter` trait | [`blockchain.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/blockchain.service.ts) (`IBlockchainService`) |
| `Header`, `BlockPtr` | [`indexer/types.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/types.ts) (`Header`, `IBlock<B>`) |
| `ChainError` | [`api.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/api.service.ts) |
| `BlockCandidateProvider` | [`indexer/dictionary`](https://github.com/subquery/subql/tree/main/packages/node-core/src/indexer/dictionary) |

### `crates/config` — `superquery-config`

| Our item | Upstream |
|---|---|
| `NodeConfig` fields/defaults | [`configure/NodeConfig.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/configure/NodeConfig.ts) (`IConfig`, `DEFAULT_CONFIG`) |
| `DbConfig` + env vars | [`db/db.module.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/db/db.module.ts) |
| CLI argument surface | [`packages/node/src/yargs.ts`](https://github.com/subquery/subql/blob/main/packages/node/src/yargs.ts) |
| Process lifecycle / signals | [`process.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/process.ts) |

### `crates/store` — `superquery-store`

| Our item | Upstream |
|---|---|
| `naming.rs` (`underscored`, `pluralize`) | [`db/sequelizeUtil.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/db/sequelizeUtil.ts) (`modelToTableName`) + Sequelize's `inflection` |
| `schema.rs` (SDL → entities) | [`packages/utils/src/graphql`](https://github.com/subquery/subql/tree/main/packages/utils/src/graphql) (`getAllEntitiesRelations`) |
| `ddl.rs` (`CREATE TABLE`) | [`db/sync-helper.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/db/sync-helper.ts) + Sequelize `sync()` |
| `entity.rs` / `PlainModel` | [`storeModelProvider/model/model.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/storeModelProvider/model/model.ts) |
| `EntityStore` trait | [`indexer/store/store.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/store/store.ts) |
| `metadata.rs` | [`storeModelProvider/metadata`](https://github.com/subquery/subql/tree/main/packages/node-core/src/indexer/storeModelProvider/metadata) |
| `transaction.rs` | [`indexer/store.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/store.service.ts) |
| `rewind.rs` | [`indexer/multiChainRewind.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/multiChainRewind.service.ts) |
| Store operation types (PoI input) | [`indexer/StoreOperations.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/StoreOperations.ts) |

### `crates/core` — `superquery-core`

| Our item | Upstream |
|---|---|
| `fetch/scheduler.rs` | [`indexer/fetch.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/fetch.service.ts) |
| `fetch/range.rs` | `fetch.service.ts` (`getModulos`, batch-range calculation) |
| `fetch/backpressure.rs` | `fetch.service.ts` (`fetchLoop` / queue-size guards) |
| `indexer/manager.rs` | [`indexer/indexer.manager.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/indexer.manager.ts) |
| `project/` (`BlockHeightMap`) | [`utils/blockHeightMap.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/utils/blockHeightMap.ts), [`indexer/project.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/project.service.ts) |
| `finality/tracker.rs`, `reorg.rs` | [`indexer/unfinalizedBlocks.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/unfinalizedBlocks.service.ts) |
| `finality/rewind.rs` | [`indexer/multiChainRewind.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/multiChainRewind.service.ts) |
| dynamic data sources | [`indexer/dynamic-ds.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/dynamic-ds.service.ts) |
| metrics / admin | [`meta`](https://github.com/subquery/subql/tree/main/packages/node-core/src/meta), [`admin`](https://github.com/subquery/subql/tree/main/packages/node-core/src/admin) |
| PoI | [`indexer/poi`](https://github.com/subquery/subql/tree/main/packages/node-core/src/indexer/poi) |

### `crates/dispatcher` — `superquery-dispatcher`

| Our item | Upstream |
|---|---|
| `dispatcher.rs` | [`blockDispatcher/base-block-dispatcher.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/blockDispatcher/base-block-dispatcher.ts), [`block-dispatcher.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/blockDispatcher/block-dispatcher.ts) |
| `worker.rs` | [`indexer/worker`](https://github.com/subquery/subql/tree/main/packages/node-core/src/indexer/worker), [`worker-block-dispatcher.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/blockDispatcher/worker-block-dispatcher.ts) |
| `queue.rs` | [`utils/queues`](https://github.com/subquery/subql/tree/main/packages/node-core/src/utils) (`AutoQueue`, `BlockSizeBuffer`) |
| `ordered_commit.rs` | SuperQuery-specific; behavioural rule from guide §3.3 |

### `crates/runtime` — `superquery-runtime`

| Our item | Upstream (concept only) |
|---|---|
| `runtime.rs`, `host.rs` | [`indexer/sandbox.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/sandbox.ts), [`sandbox.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/sandbox.service.ts) |
| `limits.rs` | `sandbox.ts` (`timeout`, memory options) |
| datasource processors | [`indexer/ds-processor.service.ts`](https://github.com/subquery/subql/blob/main/packages/node-core/src/indexer/ds-processor.service.ts) |

> The SubQuery sandbox is a **JS VM** (`vm2`/`node:vm`). SuperQuery deliberately
> diverges to WASM/Wasmtime — take the *lifecycle and capability model* from
> upstream, not the implementation.

### `crates/chains/evm` — `superquery-chain-evm`

| Our item | Upstream |
|---|---|
| adapter | [`subql-ethereum .../blockchain.service.ts`](https://github.com/subquery/subql-ethereum/blob/main/packages/node/src/blockchain.service.ts) |
| RPC / block fetch | [`subql-ethereum .../ethereum`](https://github.com/subquery/subql-ethereum/tree/main/packages/node/src/ethereum) |
| log/tx filters | [`subql-ethereum .../ethereum/block.ethereum.ts`](https://github.com/subquery/subql-ethereum/blob/main/packages/node/src/ethereum/block.ethereum.ts) (`filterLogsProcessor`, `filterTransactionsProcessor`) |
| finality rules | [`subql-ethereum .../indexer/unfinalizedBlocks.service.ts`](https://github.com/subquery/subql-ethereum/blob/main/packages/node/src/indexer/unfinalizedBlocks.service.ts) |
| manifest/datasource types | [`subql-ethereum/packages/common-ethereum`](https://github.com/subquery/subql-ethereum/tree/main/packages/common-ethereum), [`packages/types`](https://github.com/subquery/subql-ethereum/tree/main/packages/types) |

### Future chains

| Chain | Upstream |
|---|---|
| Stellar | [`subquery/subql-stellar`](https://github.com/subquery/subql-stellar) |
| Solana | [`subquery/subql-solana`](https://github.com/subquery/subql-solana) |

---

## Compatibility-critical values

Values that must match upstream **exactly** if we want schema/state parity with a
SubQuery-indexed database. Each is cited at its use site in code.

| Value | Where | Upstream source |
|---|---|---|
| `underscored(pluralize(Name))` table naming | `store/naming.rs` | `sequelizeUtil.ts#modelToTableName` |
| GraphQL scalar → Postgres type map | `store/ddl.rs` | `sync-helper.ts` `sequelizeToPostgresTypeMap` |
| `"Set"` / `"Remove"` operation strings | `store/operation.rs` | `StoreOperations.ts` (hashed into PoI leaves) |
| `_block_range` historical column | `store/` (later milestone) | `sync-helper.ts` |
| `DB_HOST`/`DB_PORT`/`DB_USER`/`DB_PASS`/`DB_DATABASE` | `config/db.rs` | `db.module.ts` |

> `_superquery_metadata` intentionally **diverges** from upstream `_metadata`: it is
> our own checkpoint contract (guide §3.4), not a compatibility surface.
