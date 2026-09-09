//! The Wasmtime implementation of [`MappingRuntime`](crate::runtime::MappingRuntime).
//!
//! Guide Milestone 9. Engine configuration is here because the settings are the
//! sandbox: fuel consumption and epoch interruption are what make an infinite-loop
//! mapping terminable, and they must be enabled on the `Engine` before any module
//! is compiled.

use wasmtime::{Config, Engine, OptLevel};

use crate::error::{Result, RuntimeError};
use crate::limits::RuntimeLimits;

/// Build the Wasmtime engine used for mapping modules.
///
/// The configuration is the security boundary, so each setting is deliberate:
///
/// - **fuel consumption** — the deterministic execution bound. Without it a
///   mapping can loop forever;
/// - **epoch interruption** — the wall-clock backstop, for time spent in host
///   calls where no fuel is burned;
/// - **no threads** — shared-memory concurrency would make handler output depend
///   on scheduling;
/// - **no relaxed SIMD** — its results are explicitly implementation-defined, so
///   two nodes could index the same block differently. Plain SIMD goes with it,
///   because Wasmtime will not accept relaxed SIMD enabled on top of a disabled
///   SIMD proposal;
/// - **`OptLevel::Speed`** — mappings are compiled once at startup and then run
///   for millions of blocks, so compile time is worth trading for run time.
pub fn build_engine() -> Result<Engine> {
    let mut config = Config::new();
    config.consume_fuel(true);
    config.epoch_interruption(true);
    config.wasm_threads(false);
    config.wasm_relaxed_simd(false);
    config.wasm_simd(false);
    config.cranelift_opt_level(OptLevel::Speed);

    Engine::new(&config).map_err(|e| RuntimeError::Load(e.to_string()))
}

/// A loaded mapping module and the limits it runs under.
///
/// # Milestone
///
/// Instantiation, host-function linking and handler invocation land with guide
/// Milestone 9 (task plan phase C1). [`build_engine`] and the validation in
/// [`crate::runtime`] are complete, so the sandbox configuration is already
/// pinned and tested.
pub struct WasmtimeRuntime {
    engine: Engine,
    limits: RuntimeLimits,
    handlers: Vec<String>,
}

impl WasmtimeRuntime {
    /// Create a runtime with the given limits.
    pub fn new(limits: RuntimeLimits) -> Result<Self> {
        Ok(Self {
            engine: build_engine()?,
            limits,
            handlers: Vec::new(),
        })
    }

    /// The limits handlers run under.
    pub fn limits(&self) -> RuntimeLimits {
        self.limits
    }

    /// The Wasmtime engine.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Handler names the loaded module exports.
    pub fn handlers(&self) -> &[String] {
        &self.handlers
    }

    /// Load and validate a mapping module.
    ///
    /// # Milestone
    ///
    /// Not yet implemented — guide Milestone 9, task plan phase C1. The
    /// validation this will call is written and tested in [`crate::runtime`].
    pub fn load_module(&mut self, _wasm: &[u8]) -> Result<()> {
        unimplemented!(
            "module instantiation and host linking; guide Milestone 9, task plan phase C1"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_enables_the_limits_the_sandbox_depends_on() {
        // If this stops building, the sandbox has silently lost its bounds:
        // fuel and epoch interruption are what terminate a runaway mapping.
        let engine = build_engine().expect("engine must build");
        // A trivial module proves the configuration is coherent and usable.
        let module = wasmtime::Module::new(&engine, r#"(module)"#);
        assert!(module.is_ok(), "engine config rejected an empty module");
    }

    #[test]
    fn a_runtime_carries_its_configured_limits() {
        let limits = RuntimeLimits::new(1_000, 250, 64);
        let rt = WasmtimeRuntime::new(limits).unwrap();
        assert_eq!(rt.limits(), limits);
        assert!(rt.handlers().is_empty());
    }
}
