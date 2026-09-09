//! Mapping ABI v1 — the constants and types shared with guest modules.
//!
//! The full specification is in `crates/runtime/ABI.md`. This module is the
//! machine-readable half: every name and number here appears in that document, and
//! the tests below assert they still agree.
//!
//! Guide Milestone 8.

use serde::{Deserialize, Serialize};

/// ABI version this host implements.
///
/// A module declaring a different version is refused at load. Guessing would be
/// worse than failing: a mapping run against the wrong ABI writes wrong data
/// silently.
pub const ABI_VERSION: i32 = 1;

/// Import module name for host functions.
pub const HOST_MODULE: &str = "superquery";

/// Names of the host functions a guest may import.
pub mod host_fn {
    /// Fetch one entity by id.
    pub const STORE_GET: &str = "sq_store_get";
    /// Insert or update one entity.
    pub const STORE_SET: &str = "sq_store_set";
    /// Delete one entity by id.
    pub const STORE_REMOVE: &str = "sq_store_remove";
    /// Emit a log line.
    pub const LOG: &str = "sq_log";
    /// Perform a controlled, height-pinned chain read.
    pub const CHAIN_CALL: &str = "sq_chain_call";

    /// Every host function, for import-table construction and tests.
    pub const ALL: &[&str] = &[STORE_GET, STORE_SET, STORE_REMOVE, LOG, CHAIN_CALL];
}

/// Names a guest module must export.
pub mod guest_fn {
    /// Returns the ABI version the module was built against.
    pub const ABI_VERSION: &str = "sq_abi_version";
    /// Allocates guest memory for the host to write into.
    pub const ALLOC: &str = "sq_alloc";
    /// Frees a region previously returned to the guest.
    pub const DEALLOC: &str = "sq_dealloc";

    /// Exports required of every module, regardless of its handlers.
    pub const REQUIRED: &[&str] = &[ABI_VERSION, ALLOC, DEALLOC];
}

/// Status codes returned by host functions.
///
/// `0` is success and `1` is a non-error "not found"; everything negative is a
/// host error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Status {
    /// The call succeeded.
    Ok = 0,
    /// No such entity. Only `sq_store_get` returns this, and it is not an error.
    NotFound = 1,
    /// Bad pointer, length, or malformed JSON.
    InvalidArgument = -1,
    /// Entity type is not in the project schema.
    UnknownEntity = -2,
    /// The store could not accept the operation.
    StoreError = -3,
    /// The guest attempted something its capabilities do not permit.
    CapabilityDenied = -4,
    /// Fuel or memory exhausted.
    ResourceExhausted = -5,
}

impl Status {
    /// The raw code crossing the ABI boundary.
    pub fn code(self) -> i32 {
        self as i32
    }

    /// Whether this represents a host-side failure.
    ///
    /// [`Status::NotFound`] is deliberately not an error — an entity that does not
    /// exist yet is the normal case on a mapping's first write.
    pub fn is_error(self) -> bool {
        self.code() < 0
    }
}

/// Log levels accepted by `sq_log`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum LogLevel {
    /// Trace.
    Trace = 0,
    /// Debug.
    Debug = 1,
    /// Info.
    Info = 2,
    /// Warn.
    Warn = 3,
    /// Error.
    Error = 4,
}

impl LogLevel {
    /// Decode a level from the guest.
    ///
    /// An unrecognised value becomes [`LogLevel::Info`] rather than an error: a
    /// bad log level is not worth failing a block over.
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => LogLevel::Trace,
            1 => LogLevel::Debug,
            3 => LogLevel::Warn,
            4 => LogLevel::Error,
            _ => LogLevel::Info,
        }
    }
}

/// The block context passed to every handler.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BlockContext {
    /// Block height.
    pub height: u64,
    /// Block hash.
    pub hash: String,
    /// Parent block hash.
    pub parent_hash: Option<String>,
    /// Block timestamp, RFC 3339.
    pub timestamp: Option<String>,
}

/// The data source that caused a handler to run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DataSourceContext {
    /// Data source name.
    pub name: String,
    /// Template parameters, e.g. a discovered contract address.
    pub parameters: serde_json::Value,
}

/// The JSON payload handed to a handler.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HandlerInput {
    /// The block being indexed.
    pub block: BlockContext,
    /// Which kind of handler this is.
    pub kind: superquery_chain_api::HandlerKind,
    /// Chain-specific input, shaped by the adapter.
    pub payload: serde_json::Value,
    /// The data source this handler belongs to.
    pub data_source: DataSourceContext,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_match_the_abi_document() {
        // ABI.md §6. These numbers are a wire contract; changing one silently
        // breaks every already-compiled mapping.
        assert_eq!(Status::Ok.code(), 0);
        assert_eq!(Status::NotFound.code(), 1);
        assert_eq!(Status::InvalidArgument.code(), -1);
        assert_eq!(Status::UnknownEntity.code(), -2);
        assert_eq!(Status::StoreError.code(), -3);
        assert_eq!(Status::CapabilityDenied.code(), -4);
        assert_eq!(Status::ResourceExhausted.code(), -5);
    }

    #[test]
    fn not_found_is_not_an_error() {
        assert!(!Status::Ok.is_error());
        assert!(!Status::NotFound.is_error());
        assert!(Status::InvalidArgument.is_error());
        assert!(Status::ResourceExhausted.is_error());
    }

    #[test]
    fn log_levels_match_the_abi_document() {
        assert_eq!(LogLevel::from_code(0), LogLevel::Trace);
        assert_eq!(LogLevel::from_code(2), LogLevel::Info);
        assert_eq!(LogLevel::from_code(4), LogLevel::Error);
        // An out-of-range level must not fail the block.
        assert_eq!(LogLevel::from_code(99), LogLevel::Info);
        assert_eq!(LogLevel::from_code(-1), LogLevel::Info);
    }

    #[test]
    fn abi_version_is_one() {
        assert_eq!(ABI_VERSION, 1);
    }

    #[test]
    fn host_and_guest_symbol_names_are_stable() {
        // Renaming any of these breaks compiled mappings, so pin them explicitly.
        assert_eq!(HOST_MODULE, "superquery");
        assert_eq!(
            host_fn::ALL,
            &[
                "sq_store_get",
                "sq_store_set",
                "sq_store_remove",
                "sq_log",
                "sq_chain_call"
            ]
        );
        assert_eq!(
            guest_fn::REQUIRED,
            &["sq_abi_version", "sq_alloc", "sq_dealloc"]
        );
    }

    #[test]
    fn handler_input_serializes_as_camel_case() {
        let input = HandlerInput {
            block: BlockContext {
                height: 100,
                hash: "0x64".into(),
                parent_hash: Some("0x63".into()),
                timestamp: None,
            },
            kind: superquery_chain_api::HandlerKind::Event,
            payload: serde_json::json!({"topic": "0xddf2"}),
            data_source: DataSourceContext {
                name: "Erc20".into(),
                parameters: serde_json::json!({"address": "0xabc"}),
            },
        };

        let json = serde_json::to_value(&input).unwrap();
        // ABI.md §8 spells these in camelCase; guests decode against that.
        assert!(json["block"]["parentHash"].is_string());
        assert!(json["dataSource"]["name"].is_string());
        assert_eq!(json["kind"], "event");

        let round_tripped: HandlerInput = serde_json::from_value(json).unwrap();
        assert_eq!(round_tripped, input);
    }
}
