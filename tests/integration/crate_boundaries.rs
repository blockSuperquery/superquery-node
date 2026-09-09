//! Enforces the workspace's dependency invariants.
//!
//! Guide Milestone 3's acceptance criteria include "core crate has no Alloy
//! imports" and "EVM details live only under `chains/evm`". Those are easy to
//! violate by accident — adding one convenient import compiles fine and only
//! reveals itself when a second chain is attempted, by which point the leak has
//! spread.
//!
//! So it is a test. It reads the manifests rather than the source, because a
//! dependency is what actually creates the coupling.

use std::path::{Path, PathBuf};

/// Chain SDKs that must never appear outside a chain adapter crate.
const CHAIN_SDKS: &[&str] = &[
    "alloy",
    "ethers",
    "web3",
    "subxt",
    "solana-client",
    "solana-sdk",
    "stellar-sdk",
    "cosmrs",
];

/// Crates permitted to depend on a chain SDK.
const CHAIN_CRATES: &[&str] = &["superquery-chain-evm"];

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is this test package (`<root>/tests`); the workspace
    // root is its parent.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("test package must live under the workspace root")
        .to_path_buf()
}

/// Read a crate manifest's `[dependencies]` section as raw text.
fn manifest_text(relative: &str) -> String {
    let path = workspace_root().join(relative).join("Cargo.toml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

/// Whether `manifest` declares a dependency on `crate_name`.
///
/// Matches a line starting with the name, so `superquery-chain-evm` does not
/// count as a hit for `evm`, and a mention inside a comment does not either.
fn declares_dependency(manifest: &str, crate_name: &str) -> bool {
    manifest.lines().any(|line| {
        let line = line.trim();
        if line.starts_with('#') {
            return false;
        }
        line.split_once(['=', '.'])
            .map(|(name, _)| name.trim() == crate_name)
            .unwrap_or(false)
    })
}

#[test]
fn no_chain_sdk_outside_chain_crates() {
    let crates = [
        ("crates/chain-api", "superquery-chain-api"),
        ("crates/config", "superquery-config"),
        ("crates/store", "superquery-store"),
        ("crates/core", "superquery-core"),
        ("crates/dispatcher", "superquery-dispatcher"),
        ("crates/runtime", "superquery-runtime"),
    ];

    for (path, name) in crates {
        assert!(
            !CHAIN_CRATES.contains(&name),
            "test setup error: {name} is listed as a chain crate"
        );

        let manifest = manifest_text(path);
        for sdk in CHAIN_SDKS {
            assert!(
                !declares_dependency(&manifest, sdk),
                "{name} depends on the chain SDK '{sdk}'.\n\
                 Guide Milestone 3: chain specifics belong in a chain adapter crate \
                 (crates/chains/*), reached through the ChainAdapter trait."
            );
        }
    }
}

#[test]
fn the_evm_crate_is_the_only_alloy_user() {
    let manifest = manifest_text("crates/chains/evm");
    assert!(
        declares_dependency(&manifest, "alloy"),
        "the EVM adapter is expected to depend on alloy"
    );
}

#[test]
fn nothing_depends_on_a_chain_crate_except_the_binary() {
    let engine_crates = [
        ("crates/chain-api", "superquery-chain-api"),
        ("crates/config", "superquery-config"),
        ("crates/store", "superquery-store"),
        ("crates/core", "superquery-core"),
        ("crates/dispatcher", "superquery-dispatcher"),
        ("crates/runtime", "superquery-runtime"),
    ];

    for (path, name) in engine_crates {
        let manifest = manifest_text(path);
        for chain_crate in CHAIN_CRATES {
            assert!(
                !declares_dependency(&manifest, chain_crate),
                "{name} depends on {chain_crate}.\n\
                 Only the binary may name a concrete chain; everything else is \
                 generic over ChainAdapter."
            );
        }
    }
}

#[test]
fn the_binary_wires_the_chain_in() {
    // The composition root is the one place a concrete chain is named.
    let manifest = manifest_text("bins/superquery-node");
    assert!(declares_dependency(&manifest, "superquery-chain-evm"));
    assert!(declares_dependency(&manifest, "superquery-core"));
}

#[test]
fn dependency_detection_does_not_produce_false_positives() {
    // Guard the guard: a substring or a comment must not count as a dependency.
    let manifest = "\
[dependencies]\n\
superquery-chain-evm.workspace = true\n\
# alloy = \"2.4\"  -- intentionally not enabled here\n\
tokio = { version = \"1\" }\n";

    assert!(declares_dependency(manifest, "superquery-chain-evm"));
    assert!(declares_dependency(manifest, "tokio"));
    // Commented out, so not a real dependency.
    assert!(!declares_dependency(manifest, "alloy"));
    // A substring of another name is not a match.
    assert!(!declares_dependency(manifest, "evm"));
    assert!(!declares_dependency(manifest, "superquery-chain"));
}
