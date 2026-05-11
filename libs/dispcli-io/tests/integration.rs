//! Integration tests for `dispcli-io`.
//!
//! v0 surface is the scaffold; real adapter tests land with the spec.

#[test]
fn version_is_non_empty() {
    assert!(!dispcli_io::version().is_empty());
}
