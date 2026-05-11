//! Integration tests for `dispcli-core`.
//!
//! These tests exercise the public API against the real surface — no
//! mocks. v0 surface is the scaffold; real assertions land with the spec.

#[test]
fn version_is_non_empty() {
    assert!(!dispcli_core::version().is_empty());
}
