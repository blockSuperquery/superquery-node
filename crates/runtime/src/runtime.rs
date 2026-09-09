//! The mapping runtime trait, and module validation.
//!
//! Validation happens at **load**, not at call time. A module that imports
//! something forbidden, or declares the wrong ABI version, is refused before it
//! ever runs — so a misbuilt project fails at startup with a clear message instead
//! of part-way through a sync.

use async_trait::async_trait;

use crate::abi::{guest_fn, ABI_VERSION, HOST_MODULE};
use crate::error::{Result, RuntimeError};
use crate::host::HostContext;

/// Runs a project's mapping handlers.
#[async_trait]
pub trait MappingRuntime: Send + Sync {
    /// Invoke `handler` with a JSON-encoded input.
    ///
    /// `ctx` accumulates the writes the handler performs; the caller commits them.
    async fn invoke(
        &self,
        handler: &str,
        input: &serde_json::Value,
        ctx: &mut HostContext,
    ) -> Result<()>;

    /// Handler names the loaded module exports.
    fn exported_handlers(&self) -> &[String];

    /// Whether the module exports `handler`.
    fn has_handler(&self, handler: &str) -> bool {
        self.exported_handlers().iter().any(|h| h == handler)
    }
}

/// Check a module's declared ABI version against this host's.
pub fn check_abi_version(found: i32) -> Result<()> {
    if found == ABI_VERSION {
        Ok(())
    } else {
        Err(RuntimeError::AbiVersionMismatch {
            found,
            expected: ABI_VERSION,
        })
    }
}

/// Check that every ABI-required export is present.
pub fn check_required_exports(exports: &[String]) -> Result<()> {
    for required in guest_fn::REQUIRED {
        if !exports.iter().any(|e| e == required) {
            return Err(RuntimeError::MissingExport((*required).to_string()));
        }
    }
    Ok(())
}

/// Check that a module imports nothing outside the permitted host surface.
///
/// This is the capability model in force (guide §3.5): WASI is not linked, so a
/// module wanting a filesystem or a clock has to import it, and importing it fails
/// here. `imports` is `(module, name)` pairs.
pub fn check_imports(imports: &[(String, String)]) -> Result<()> {
    for (module, name) in imports {
        if module != HOST_MODULE || !crate::abi::host_fn::ALL.contains(&name.as_str()) {
            return Err(RuntimeError::ForbiddenImport {
                module: module.clone(),
                name: name.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_matching_abi_version_is_accepted() {
        assert!(check_abi_version(1).is_ok());
    }

    #[test]
    fn a_mismatched_abi_version_is_refused_not_guessed() {
        let err = check_abi_version(2).unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::AbiVersionMismatch {
                found: 2,
                expected: 1
            }
        ));
        // The message must name both versions, or the fix is guesswork.
        let msg = err.to_string();
        assert!(msg.contains("module declares 2"));
        assert!(msg.contains("host implements 1"));
    }

    #[test]
    fn every_required_export_must_be_present() {
        let complete: Vec<String> = guest_fn::REQUIRED.iter().map(|s| s.to_string()).collect();
        assert!(check_required_exports(&complete).is_ok());

        let missing_alloc = vec!["sq_abi_version".to_string(), "sq_dealloc".to_string()];
        assert!(matches!(
            check_required_exports(&missing_alloc),
            Err(RuntimeError::MissingExport(name)) if name == "sq_alloc"
        ));
    }

    #[test]
    fn permitted_host_imports_are_accepted() {
        let imports: Vec<(String, String)> = crate::abi::host_fn::ALL
            .iter()
            .map(|f| (HOST_MODULE.to_string(), f.to_string()))
            .collect();
        assert!(check_imports(&imports).is_ok());
    }

    #[test]
    fn wasi_imports_are_refused_at_load() {
        // The capability model: no filesystem, clock, or randomness.
        let wasi = vec![("wasi_snapshot_preview1".to_string(), "fd_write".to_string())];
        assert!(matches!(
            check_imports(&wasi),
            Err(RuntimeError::ForbiddenImport { .. })
        ));

        let clock = vec![(
            "wasi_snapshot_preview1".to_string(),
            "clock_time_get".to_string(),
        )];
        assert!(check_imports(&clock).is_err());
    }

    #[test]
    fn an_unknown_function_in_the_host_module_is_still_refused() {
        // Right module, wrong function — must not be waved through.
        let sneaky = vec![(HOST_MODULE.to_string(), "sq_read_file".to_string())];
        assert!(matches!(
            check_imports(&sneaky),
            Err(RuntimeError::ForbiddenImport { name, .. }) if name == "sq_read_file"
        ));
    }

    #[test]
    fn a_module_importing_nothing_is_fine() {
        assert!(check_imports(&[]).is_ok());
    }
}
