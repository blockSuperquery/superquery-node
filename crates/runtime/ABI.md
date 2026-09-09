# SuperQuery Mapping ABI v1

`superquery_mapping_abi = 1`

The contract between a compiled mapping module and the SuperQuery node. Guide
Milestone 8.

> **Stability.** The version is a single integer, checked at module load. A node
> refuses a module whose declared version it does not implement, rather than
> guessing — a mapping running against the wrong ABI would silently write wrong
> data. Additive changes bump the minor documentation here; anything that changes
> an existing signature's meaning bumps the integer.

---

## 1. Why WASM

SubQuery runs mappings in a JavaScript VM. SuperQuery does not reproduce that
(guide §3.5). WebAssembly gives three things a JS sandbox does not:

- **deterministic execution** — the same inputs produce the same writes, which is
  what makes replay after a reorg and Proof of Index meaningful;
- **enforceable resource limits** — fuel and memory caps are properties of the
  engine, not of cooperating guest code;
- **default-deny capabilities** — a module reaches the host *only* through the
  functions below.

Mappings are compiled with `cargo build --target wasm32-wasip1`.

## 2. Capability model

Denied unless a host function below grants it:

| Capability | Status |
|---|---|
| Filesystem | denied |
| Network | denied — chain reads go through `sq_chain_call` |
| System clock | denied — use the block timestamp |
| Randomness | denied — non-deterministic |
| Environment variables | denied |
| Threads | denied |

WASI is **not** linked. The module's only imports are the `superquery` module
below.

## 3. Memory and value passing

The guest exports an allocator so the host can place data in guest memory:

```wat
(export "sq_alloc" (func (param i32) (result i32)))   ;; size -> ptr
(export "sq_dealloc" (func (param i32 i32)))          ;; ptr, size
```

Values cross the boundary as **length-prefixed UTF-8 JSON** in linear memory. A
pointer/length pair identifies a region:

```text
(ptr: i32, len: i32)
```

Entities are JSON objects. JSON rather than a binary encoding because entity
shapes are defined by each project's GraphQL schema and are not known when the
host is compiled; serialization is canonical (keys sorted, no insignificant
whitespace) so identical logical values produce identical bytes.

Host functions returning variable-length data write a `(ptr, len)` pair into a
caller-provided out-pointer:

```text
sq_store_get(entity_ptr, entity_len, id_ptr, id_len, out_ptr) -> i32
```

The return value is a [status code](#6-status-codes). The guest owns the returned
region and frees it with `sq_dealloc`.

## 4. Host functions

Imported from module `"superquery"`.

### `sq_store_get`

```wat
(import "superquery" "sq_store_get"
  (func (param i32 i32 i32 i32 i32) (result i32)))
;;            entity_ptr entity_len id_ptr id_len out_ptr
```

Fetch one entity by id. Writes a `(ptr, len)` pair at `out_ptr` on success;
returns `STATUS_NOT_FOUND` with nothing written when the entity does not exist.

### `sq_store_set`

```wat
(import "superquery" "sq_store_set"
  (func (param i32 i32 i32 i32 i32 i32) (result i32)))
;;            entity_ptr entity_len id_ptr id_len data_ptr data_len
```

Insert or update. `data` is a JSON object that must carry an `id` matching the
`id` argument. Buffered, not written immediately — the dispatcher commits the
block's writes as one transaction (guide Milestone 10).

### `sq_store_remove`

```wat
(import "superquery" "sq_store_remove"
  (func (param i32 i32 i32 i32) (result i32)))
;;            entity_ptr entity_len id_ptr id_len
```

Delete one entity by id.

### `sq_log`

```wat
(import "superquery" "sq_log"
  (func (param i32 i32 i32) (result i32)))
;;            level msg_ptr msg_len
```

Levels: `0` trace, `1` debug, `2` info, `3` warn, `4` error. Emitted through the
node's `tracing` subscriber, tagged with the project and block height.

### `sq_chain_call`

```wat
(import "superquery" "sq_chain_call"
  (func (param i32 i32 i32) (result i32)))
;;            request_ptr request_len out_ptr
```

A **controlled** read of chain state — the one outward call a mapping may make.
The request is a JSON object naming the call and its arguments; the host pins it
to the block being indexed, so a mapping cannot read state from a different
height and break replay determinism.

Restrictions:

- reads only; no transaction may be submitted;
- pinned to the current block;
- subject to the same fuel budget as the rest of the handler.

## 5. Guest exports

```wat
(export "sq_abi_version" (func (result i32)))   ;; must return 1
(export "sq_alloc"   (func (param i32) (result i32)))
(export "sq_dealloc" (func (param i32 i32)))
```

Plus one export per handler named in the manifest, each taking the JSON-encoded
input:

```wat
(export "handleTransfer" (func (param i32 i32) (result i32)))
;;                              input_ptr input_len
```

The host calls `sq_abi_version` first and refuses the module on a mismatch.

## 6. Status codes

| Code | Meaning |
|---|---|
| `0` | success |
| `1` | not found (`sq_store_get` only; not an error) |
| `-1` | invalid arguments — bad pointer, length, or malformed JSON |
| `-2` | unknown entity type — not in the project schema |
| `-3` | store error — the write could not be buffered |
| `-4` | capability denied |
| `-5` | resource exhausted — fuel or memory |

Negative codes are host errors. A handler that ignores one and continues will have
its block failed by the host anyway: partial application of a block's writes is
never committed.

## 7. Limits

Per handler invocation, configurable on the node:

| Limit | Flag | Default |
|---|---|---|
| Fuel | `--mapping-fuel` | 10,000,000,000 |
| Wall clock | `--mapping-timeout-ms` | 5,000 ms |
| Memory | `--mapping-memory-mb` | 256 MiB |

Fuel is the deterministic bound — the same module on the same input consumes the
same fuel on any machine. The wall-clock timeout is a backstop for time spent
outside the guest (a slow `sq_chain_call`). Exhausting either terminates the
instance; guide Milestone 9's acceptance is that an infinite-loop mapping is
terminated rather than hanging the node.

## 8. Handler input

```jsonc
{
  "block": {
    "height": 18000000,
    "hash": "0x...",
    "parentHash": "0x...",
    "timestamp": "2023-08-15T12:00:00Z"
  },
  "kind": "event",          // "block" | "transaction" | "event"
  "payload": { /* chain-specific, shaped by the adapter */ },
  "dataSource": {
    "name": "Erc20",
    "parameters": { "address": "0x..." }
  }
}
```

`payload` is the only chain-specific part. For EVM events it is a decoded log;
`superquery-sdk` generates typed accessors over it, so mapping authors do not
handle the JSON directly.
