//! dispcli-core — IO-free dispatch-envelope construction.
//!
//! This crate is the long-lived core. It must remain IO-free: no
//! `std::fs`, no `std::process`, no stdin/stdout. Inputs and outputs are
//! plain Rust structs. IO is provided by callers — `dispcli-io` for the
//! native CLI binary, host functions for the future mnemra WASM plugin.
//!
//! v0 scaffold: the real surface (envelope types, skill resolver trait,
//! assembly logic) lands when the spec defines it. See `docs/specs/`.

/// Returns the crate version, sourced from `Cargo.toml` at build time.
///
/// Used by the v0 scaffold binary to prove cross-crate wiring works
/// end-to-end. Safe to delete once the real public surface lands.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
