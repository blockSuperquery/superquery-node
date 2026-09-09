//! Resource limits for mapping execution.
//!
//! Guide §3.5 requires fuel, memory and timeout bounds. The three are not
//! redundant — each catches something the others cannot:
//!
//! - **fuel** bounds computation *deterministically*: the same module on the same
//!   input burns the same fuel on every machine, so a mapping that passes on a
//!   developer's laptop cannot fail only in production;
//! - **wall clock** bounds time spent *outside* the guest, which fuel does not
//!   measure — a slow `sq_chain_call` burns no fuel while it waits;
//! - **memory** bounds allocation, which neither of the others constrains.

use std::time::Duration;

/// Per-invocation limits for a mapping handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeLimits {
    /// Fuel units per handler invocation.
    pub fuel: u64,
    /// Wall-clock ceiling per invocation.
    pub timeout: Duration,
    /// Memory ceiling, in bytes.
    pub memory_bytes: usize,
}

impl RuntimeLimits {
    /// Build limits from the node's configured values.
    pub fn new(fuel: u64, timeout_ms: u64, memory_mb: u32) -> Self {
        Self {
            // Zero fuel would trap immediately; treat it as "at least try".
            fuel: fuel.max(1),
            timeout: Duration::from_millis(timeout_ms.max(1)),
            memory_bytes: (memory_mb.max(1) as usize) * 1024 * 1024,
        }
    }

    /// Memory ceiling in Wasm pages (64 KiB each), which is what Wasmtime wants.
    pub fn memory_pages(&self) -> usize {
        self.memory_bytes.div_ceil(64 * 1024)
    }
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        // Matches the NodeConfig defaults and ABI.md §7.
        Self::new(10_000_000_000, 5_000, 256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_abi_document() {
        let l = RuntimeLimits::default();
        assert_eq!(l.fuel, 10_000_000_000);
        assert_eq!(l.timeout, Duration::from_millis(5_000));
        assert_eq!(l.memory_bytes, 256 * 1024 * 1024);
    }

    #[test]
    fn memory_converts_to_whole_pages() {
        assert_eq!(RuntimeLimits::new(1, 1, 1).memory_pages(), 16);
        assert_eq!(RuntimeLimits::new(1, 1, 256).memory_pages(), 4096);
    }

    #[test]
    fn zero_limits_are_raised_so_a_handler_can_at_least_start() {
        let l = RuntimeLimits::new(0, 0, 0);
        assert_eq!(l.fuel, 1);
        assert_eq!(l.timeout, Duration::from_millis(1));
        assert!(l.memory_pages() >= 1);
    }
}
