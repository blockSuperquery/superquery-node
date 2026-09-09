//! Cross-crate integration tests for `superquery-node`.
//!
//! This package carries no library code; it exists so the `tests/` directory in
//! the guide's §5 layout is compiled and run by `cargo test --workspace`.
//!
//! - `integration/` — tests spanning several crates, and tests asserting
//!   properties of the workspace itself (see `crate_boundaries.rs`).
//! - `fixtures/` — shared test data.
//! - `reorg/` — reorg and rewind scenarios (guide Milestone 11).
