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

// ─────────────────────────────────────────────────────────────────────────
// Task 5 RED PHASE — `dispcli assemble` black-box acceptance tests
// (spec 0001-envelope-assembly.md R8/R9, plan Task 5).
//
// Written against the spec ONLY, before `assemble` exists — they fail by
// design right now (the binary is still the version-string scaffold above).
// Forge implements to green in a later dispatch; do not weaken or delete
// an assertion here to make the suite pass — see `skills/bdd.md` and
// `team/glitch.md` <push-back-when>.
//
// Fixtures live under `tests/fixtures/`:
//   - `happy/` — one agent ("implementer"), one pattern ("implementation")
//     with one skill ("core"), one always-included block ("metrics"), a
//     non-worktree dispatch (envelope.worktree = null, registry
//     worktree_required = false). Empty skills_add/skills_remove (sidesteps
//     Gap-2), absent scope globs (sidesteps Gap-3), registry-dir-relative
//     paths (sidesteps Gap-1) — per the plan's Task 5 fixture-design note.
//   - `happy/golden.md` — the exact expected assembled document, derived
//     from R4 (envelope schema + byte-for-byte field order) and R5 (body
//     order profile -> skill -> block -> task body, `\n\n` joining per the
//     Gap-4 resolution). See the dispatch completion report for the
//     component-by-component derivation and the scalar-quoting caveat.
//   - `malformed/` — a syntactically invalid (truncated) request JSON, for
//     the `request_invalid` failure-path scenario (AC8.2).
// ─────────────────────────────────────────────────────────────────────────

use serde_json::Value;
use std::fs;
use std::path::PathBuf;

/// Resolve a path under `cmd/dispcli/tests/fixtures/`.
fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(rel)
}

/// Scenario: `dispcli --version` still reports the core version.
///
/// Given the existing scaffold's `--version` behavior
/// When invoked with `--version`
/// Then it exits 0 and reports a version string — preserved through the
/// restructure to an `assemble` subcommand. *(R9)*
#[test]
fn version_flag_still_reports_core_version() {
    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("--version")
        .output()
        .expect("binary failed to execute");

    assert!(
        output.status.success(),
        "--version exited non-zero: {output:?}"
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout was not utf-8");
    assert!(
        stdout.contains("0.1.0"),
        "expected --version output to report the core version (0.1.0), got: {stdout:?}"
    );
}

/// Scenario: a happy-path `assemble` succeeds and reports a complete R8
/// summary.
///
/// Given a well-formed request + registry (one agent, one pattern with one
/// skill, one always-included block, non-worktree dispatch — R1/R2)
/// When `dispcli assemble --request <req> --config <cfg> --out <out>` runs
/// Then it exits 0, prints exactly one JSON summary object to stdout,
/// writes the document to `--out`, and the summary carries every R8 field
/// with the values this fixture implies. *(R9, AC8.1, AC8.3)*
#[test]
fn happy_path_assemble_exits_zero_and_reports_full_summary() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("document.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("happy/request.json"))
        .arg("--config")
        .arg(fixture("happy/registry.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert!(
        output.status.success(),
        "assemble exited non-zero. stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout was not utf-8");

    // Exactly one JSON object on stdout (AC8.1): `serde_json::from_str`
    // rejects any trailing non-whitespace content, so this also catches a
    // second object or stray prose appended after the summary.
    let summary: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout was not a single JSON object: {e}\nstdout={stdout:?}"));

    assert_eq!(summary["agent"], "implementer");
    assert_eq!(summary["tier"], "t2");
    assert_eq!(summary["weight"], "standard");
    assert_eq!(
        summary["mode"], "bypassPermissions",
        "mode_override is null in the request -> falls back to the registry's default_mode"
    );
    assert_eq!(
        summary["working_dir"], "/fixtures/repo",
        "working_dir is `repo` when envelope.worktree is null"
    );

    // document_path: compare canonicalized paths so a tmp-dir symlink
    // (e.g. macOS /tmp -> /private/tmp) can't cause a false failure
    // regardless of whether the implementation echoes --out verbatim or
    // canonicalizes it.
    let reported_path = summary["document_path"]
        .as_str()
        .expect("document_path must be a string");
    let reported_canon =
        fs::canonicalize(reported_path).expect("document_path did not resolve to a real file");
    let expected_canon = fs::canonicalize(&out_path).expect("document was not written to --out");
    assert_eq!(
        reported_canon, expected_canon,
        "document_path must point at the file written to --out"
    );

    // worktree block (R8, AC8.3): required is false (registry
    // worktree_required=false AND envelope.worktree=null) and commands is
    // present and EMPTY (not null/omitted) for this non-worktree dispatch.
    assert_eq!(summary["worktree"]["required"], false);
    assert_eq!(
        summary["worktree"]["commands"]
            .as_array()
            .expect("worktree.commands must be an array")
            .len(),
        0,
        "worktree.commands must be present and empty for a non-worktree dispatch (AC8.3)"
    );
    // worktree.path for a non-worktree dispatch is not pinned by the R8
    // example (it only shows the worktree-required case). Natural-reading
    // assumption: it mirrors envelope.worktree (null). Flagged as a minor
    // spec gap in the dispatch report — a Forge deviation here (e.g.
    // echoing `repo` instead) is a reconciliation point, not necessarily a
    // bug.
    assert!(
        summary["worktree"]["path"].is_null(),
        "worktree.path is expected null when envelope.worktree is null (assumption — see report)"
    );

    // size block (AC8.1): components sum exactly to total_bytes, and
    // total_bytes matches the written document's on-disk byte length.
    let total_bytes = summary["size"]["total_bytes"]
        .as_u64()
        .expect("size.total_bytes must be an integer");
    let components = summary["size"]["components"]
        .as_array()
        .expect("size.components must be an array");
    assert_eq!(
        components.len(),
        5,
        "expected 5 assembled sections: envelope, profile, 1 skill, 1 block, task body (R5 order)"
    );
    let component_sum: u64 = components
        .iter()
        .map(|c| {
            c["bytes"]
                .as_u64()
                .expect("component.bytes must be an integer")
        })
        .sum();
    assert_eq!(
        component_sum, total_bytes,
        "size.components bytes must sum exactly to total_bytes (AC8.1)"
    );
    assert_eq!(
        components[0]["section"], "envelope",
        "the envelope is always the first assembled component (AC4.4/AC5.1)"
    );
    assert_eq!(
        components[1]["section"], "profile:implementer",
        "R8 example format: profile:<agent>"
    );
    assert_eq!(
        components[2]["section"], "skill:core",
        "R8 example format: skill:<skill id>"
    );
    // components[3] (the "metrics" block) and components[4] (task body)
    // section-name strings aren't pinned by the R8 example (it only shows
    // envelope/profile/skill) — not asserted here, see the dispatch report.

    let written_len = fs::metadata(&out_path)
        .expect("document was not written to --out")
        .len();
    assert_eq!(
        total_bytes, written_len,
        "size.total_bytes must equal the written document's byte length (AC8.1)"
    );

    assert_eq!(
        summary["verify_recipes"]
            .as_array()
            .expect("verify_recipes must be an array")
            .len(),
        0,
        "no verify entries in the request -> empty verify_recipes"
    );
    assert_eq!(
        summary["warnings"]
            .as_array()
            .expect("warnings must be an array")
            .len(),
        0,
        "no unsupported placeholders or scope-overlap in this fixture -> no warnings"
    );
}

/// Scenario: the happy-path document byte-matches the committed golden.
///
/// Given the same happy-path fixture
/// When `dispcli assemble` runs
/// Then the written document is byte-identical to `happy/golden.md` —
/// envelope (R4 field order, explicit null/[]), then body in R5 order
/// (profile -> skill -> block -> task body), joined by `\n\n` (Gap-4).
/// *(AC9.1)*
#[test]
fn happy_path_document_byte_matches_golden() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("document.md");

    Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("happy/request.json"))
        .arg("--config")
        .arg(fixture("happy/registry.toml"))
        .arg("--out")
        .arg(&out_path)
        .assert()
        .success();

    let actual = fs::read(&out_path).expect("document was not written");
    let expected = fs::read(fixture("happy/golden.md")).expect("golden fixture missing");

    assert_eq!(
        actual,
        expected,
        "assembled document did not byte-match the golden.\n--- actual ---\n{}\n--- expected ---\n{}",
        String::from_utf8_lossy(&actual),
        String::from_utf8_lossy(&expected)
    );
}

/// Scenario: an unparseable `--request` produces a structured error on
/// stderr and nothing on stdout.
///
/// Given a `--request` file containing syntactically invalid (truncated)
/// JSON
/// When `dispcli assemble` runs
/// Then nothing is printed to stdout, a single `{"error": {...}}` JSON
/// object is printed to stderr with kind `request_invalid`, and the
/// process exits 3. *(AC8.2, R8 error taxonomy)*
///
/// Chosen over the "missing --request file" variant specifically to avoid
/// a genuine spec ambiguity (usage=2 vs request_invalid=3 for a
/// nonexistent path) — flagged in the dispatch report's spec-gaps section.
#[test]
fn malformed_request_json_reports_request_invalid_error() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("unused.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("malformed/request-malformed.json"))
        .arg("--config")
        .arg(fixture("happy/registry.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert_eq!(
        output.status.code(),
        Some(3),
        "malformed request JSON should map to request_invalid (exit 3, R8). stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout was not utf-8");
    assert!(
        stdout.is_empty(),
        "stdout must stay empty on failure (AC8.2), got: {stdout:?}"
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr was not utf-8");
    let error: Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("stderr was not a single JSON object: {e}\nstderr={stderr:?}"));

    assert_eq!(error["error"]["kind"], "request_invalid");
    assert!(
        error["error"]["message"].is_string(),
        "error.message must be a string"
    );
    assert!(
        error["error"]["details"].is_array(),
        "error.details must be an array"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Task 8 — R6 weight classes, end-to-end through the CLI.
//
// Fixtures live under `tests/fixtures/light/` — a self-contained registry
// declaring both `standard` and `light` weight classes over the same
// agent/skill/block declarations, differing only in `profile_sections`
// (light restricts to `["role"]`, excluding a `<persona>` section the
// standard weight includes verbatim). Deliberately separate from
// `happy/` so this fixture can never perturb the happy-path golden byte
// comparison.
// ─────────────────────────────────────────────────────────────────────────

/// Scenario: a `light` weight dispatch reports a smaller size in the R8
/// summary than the same fixture assembled at `standard` weight.
///
/// Given a registry declaring both a `standard` and a `light` weight
/// class (light restricting `profile_sections` to `["role"]`)
/// When `dispcli assemble` runs against the same fixture profile/skill/
/// block content, once per weight
/// Then both invocations succeed, each summary reports its applied
/// `weight` verbatim, and the light dispatch's `size.total_bytes` is
/// strictly smaller than the standard dispatch's. *(AC6.2 — "the summary
/// reports which weight class applied and the resulting component
/// sizes... so the operator can confirm a light dispatch actually came
/// out light")*
#[test]
fn light_weight_dispatch_reports_a_smaller_summary_size_than_standard() {
    let standard_tempdir = tempfile::tempdir().expect("tempdir");
    let standard_out = standard_tempdir.path().join("standard.md");
    let standard_output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("light/request-standard.json"))
        .arg("--config")
        .arg(fixture("light/registry.toml"))
        .arg("--out")
        .arg(&standard_out)
        .output()
        .expect("binary failed to execute");
    assert!(
        standard_output.status.success(),
        "standard-weight assemble exited non-zero. stdout={} stderr={}",
        String::from_utf8_lossy(&standard_output.stdout),
        String::from_utf8_lossy(&standard_output.stderr)
    );
    let standard_stdout = String::from_utf8(standard_output.stdout).expect("stdout was not utf-8");
    let standard_summary: Value = serde_json::from_str(standard_stdout.trim())
        .unwrap_or_else(|e| panic!("standard stdout was not a single JSON object: {e}"));
    assert_eq!(standard_summary["weight"], "standard");

    let light_tempdir = tempfile::tempdir().expect("tempdir");
    let light_out = light_tempdir.path().join("light.md");
    let light_output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("light/request-light.json"))
        .arg("--config")
        .arg(fixture("light/registry.toml"))
        .arg("--out")
        .arg(&light_out)
        .output()
        .expect("binary failed to execute");
    assert!(
        light_output.status.success(),
        "light-weight assemble exited non-zero. stdout={} stderr={}",
        String::from_utf8_lossy(&light_output.stdout),
        String::from_utf8_lossy(&light_output.stderr)
    );
    let light_stdout = String::from_utf8(light_output.stdout).expect("stdout was not utf-8");
    let light_summary: Value = serde_json::from_str(light_stdout.trim())
        .unwrap_or_else(|e| panic!("light stdout was not a single JSON object: {e}"));
    assert_eq!(
        light_summary["weight"], "light",
        "AC6.2: the summary must report which weight class applied"
    );

    let standard_bytes = standard_summary["size"]["total_bytes"]
        .as_u64()
        .expect("standard size.total_bytes must be an integer");
    let light_bytes = light_summary["size"]["total_bytes"]
        .as_u64()
        .expect("light size.total_bytes must be an integer");
    assert!(
        light_bytes < standard_bytes,
        "AC6.2: a light dispatch must be observably smaller than standard \
         in the R8 summary's size accounting — standard={standard_bytes} \
         light={light_bytes}"
    );
}
