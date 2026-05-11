//! dispcli-io — native IO adapters for `dispcli-core`.
//!
//! This crate is the native-only IO layer. It wires filesystem reads
//! (skill resolution from `skills/*.md`), envelope writes to scratch, and
//! any other host-provided IO the CLI binary needs. The mnemra WASM
//! plugin will replace this crate with host-function adapters; the core
//! crate stays untouched.
//!
//! v0 scaffold: real adapters land when the spec defines the
//! `dispcli-core` traits they implement. See `docs/specs/`.

/// Returns the crate version, sourced from `Cargo.toml` at build time.
///
/// Used by the v0 scaffold to prove the crate is wired into the
/// workspace. Safe to delete once real adapters land.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
