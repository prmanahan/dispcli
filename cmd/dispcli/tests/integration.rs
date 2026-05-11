//! Integration tests for `dispcli`.
//!
//! Tests invoke the binary as a subprocess via `assert_cmd`, verifying the
//! real CLI surface — no mocked arg parsing. v0 surface is the scaffold;
//! real assertions land with the spec.

use assert_cmd::Command;

#[test]
fn scaffold_runs_and_exits_success() {
    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .output()
        .expect("binary failed to execute");
    assert!(output.status.success(), "dispcli scaffold exited non-zero");
    let stdout = String::from_utf8(output.stdout).expect("stdout was not utf-8");
    assert!(
        stdout.starts_with("dispcli scaffold"),
        "unexpected stdout: {stdout:?}"
    );
}
