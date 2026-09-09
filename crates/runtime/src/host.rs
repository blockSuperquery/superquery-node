//! Host functions: everything a mapping is allowed to do.
//!
//! This module is the entire attack surface of the sandbox. A mapping's only way
//! to affect the outside world is a call defined here, so the set is kept small
//! and each entry is justified in `ABI.md`.
//!
//! Writes are **buffered, not applied**. A handler calling `sq_store_set` adds to
//! [`HostContext::operations`]; the dispatcher commits the block's accumulated
//! operations in one transaction (guide Milestone 10). That is what makes a
//! half-executed block impossible to observe.

use serde_json::Value;
use superquery_store::{Operation, OperationType};

use crate::abi::{BlockContext, LogLevel, Status};

/// State a mapping invocation may reach.
///
/// One per block, shared by every handler that runs for it — so a later handler
/// sees what an earlier one wrote, matching sequential execution.
pub struct HostContext {
    /// The block being indexed. Pins `sq_chain_call` to this height.
    pub block: BlockContext,
    /// Entity mutations accumulated so far, in call order.
    pub operations: Vec<Operation>,
    /// Entity types in the project schema. Anything else is rejected, so a typo
    /// in a mapping surfaces as an error rather than a silently discarded write.
    pub known_entities: Vec<String>,
}

impl HostContext {
    /// Create a context for one block.
    pub fn new(block: BlockContext, known_entities: Vec<String>) -> Self {
        Self {
            block,
            operations: Vec::new(),
            known_entities,
        }
    }

    /// Whether `entity` is part of the project schema.
    pub fn knows_entity(&self, entity: &str) -> bool {
        self.known_entities.iter().any(|e| e == entity)
    }

    /// Buffer an entity write.
    ///
    /// Returns the status the guest sees.
    pub fn store_set(&mut self, entity: &str, id: &str, data: Value) -> Status {
        if !self.knows_entity(entity) {
            return Status::UnknownEntity;
        }
        if !data.is_object() {
            return Status::InvalidArgument;
        }
        // The id argument is authoritative: a payload whose `id` disagrees would
        // otherwise write under one key and be read back under another.
        if data.get("id").and_then(Value::as_str) != Some(id) {
            return Status::InvalidArgument;
        }

        self.operations.push(Operation {
            operation: OperationType::Set,
            entity_type: entity.to_string(),
            data,
        });
        Status::Ok
    }

    /// Buffer an entity deletion.
    pub fn store_remove(&mut self, entity: &str, id: &str) -> Status {
        if !self.knows_entity(entity) {
            return Status::UnknownEntity;
        }
        self.operations.push(Operation {
            operation: OperationType::Remove,
            entity_type: entity.to_string(),
            data: Value::String(id.to_string()),
        });
        Status::Ok
    }

    /// Emit a log line from the guest, tagged with the block it came from.
    pub fn log(&self, level: LogLevel, message: &str) {
        let height = self.block.height;
        match level {
            LogLevel::Trace => tracing::trace!(height, mapping = true, "{message}"),
            LogLevel::Debug => tracing::debug!(height, mapping = true, "{message}"),
            LogLevel::Info => tracing::info!(height, mapping = true, "{message}"),
            LogLevel::Warn => tracing::warn!(height, mapping = true, "{message}"),
            LogLevel::Error => tracing::error!(height, mapping = true, "{message}"),
        }
    }

    /// Take the buffered operations, leaving the context empty.
    pub fn take_operations(&mut self) -> Vec<Operation> {
        std::mem::take(&mut self.operations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn context() -> HostContext {
        HostContext::new(
            BlockContext {
                height: 100,
                hash: "0x64".into(),
                parent_hash: None,
                timestamp: None,
            },
            vec!["Transfer".into(), "Account".into()],
        )
    }

    #[test]
    fn a_valid_write_is_buffered_not_applied() {
        let mut ctx = context();
        let status = ctx.store_set("Transfer", "tx-1", json!({"id": "tx-1", "amount": "5"}));

        assert_eq!(status, Status::Ok);
        assert_eq!(ctx.operations.len(), 1);
        assert_eq!(ctx.operations[0].operation, OperationType::Set);
        assert_eq!(ctx.operations[0].entity_type, "Transfer");
    }

    #[test]
    fn writes_to_unknown_entities_are_rejected() {
        let mut ctx = context();
        // A typo in a mapping must surface, not vanish.
        assert_eq!(
            ctx.store_set("Trasnfer", "x", json!({"id": "x"})),
            Status::UnknownEntity
        );
        assert!(ctx.operations.is_empty());
    }

    #[test]
    fn a_payload_id_must_match_the_id_argument() {
        let mut ctx = context();
        // Otherwise the row is written under one key and read back under another.
        assert_eq!(
            ctx.store_set("Transfer", "tx-1", json!({"id": "tx-2"})),
            Status::InvalidArgument
        );
        assert_eq!(
            ctx.store_set("Transfer", "tx-1", json!({"amount": "5"})),
            Status::InvalidArgument
        );
        assert!(ctx.operations.is_empty());
    }

    #[test]
    fn non_object_payloads_are_rejected() {
        let mut ctx = context();
        assert_eq!(
            ctx.store_set("Transfer", "tx-1", json!("not an object")),
            Status::InvalidArgument
        );
        assert_eq!(
            ctx.store_set("Transfer", "tx-1", json!([1, 2, 3])),
            Status::InvalidArgument
        );
    }

    #[test]
    fn removals_record_the_id_only() {
        let mut ctx = context();
        assert_eq!(ctx.store_remove("Account", "acc-1"), Status::Ok);
        assert_eq!(ctx.operations[0].operation, OperationType::Remove);
        assert_eq!(ctx.operations[0].data, json!("acc-1"));

        assert_eq!(ctx.store_remove("Nope", "acc-1"), Status::UnknownEntity);
    }

    #[test]
    fn operations_keep_call_order() {
        let mut ctx = context();
        ctx.store_set("Transfer", "a", json!({"id": "a"}));
        ctx.store_remove("Account", "b");
        ctx.store_set("Transfer", "c", json!({"id": "c"}));

        let ops = ctx.take_operations();
        // Order is what makes replay deterministic.
        assert_eq!(
            ops.iter().map(|o| o.operation.as_str()).collect::<Vec<_>>(),
            vec!["Set", "Remove", "Set"]
        );
        // Taking empties the buffer, so the next block starts clean.
        assert!(ctx.operations.is_empty());
    }
}
