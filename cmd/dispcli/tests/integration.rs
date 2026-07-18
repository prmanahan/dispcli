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

// ─────────────────────────────────────────────────────────────────────────
// Task 10 RED PHASE — output contract & error taxonomy, end-to-end through
// the CLI (spec 0001-envelope-assembly.md R8, plan Task 10).
//
// These tests exercise capability that Tasks 1-9 left unwired or unbuilt at
// the CLI layer, confirmed by manual probe against the binary before this
// suite was written (see the dispatch completion report for the exact
// probe transcripts):
//   - `worktree.required`/`worktree.commands` (main.rs currently reports
//     `required: agent_entry.worktree_required` unconditionally — ignoring
//     `envelope.worktree` nullness — and `commands: Vec::new()` always).
//   - `try_assemble` never calls `validate_request`/`validate_registry` —
//     both exist in `dispcli-core` and are unit-tested there, but a
//     request/registry that violates an R7/AC2.2 rule with no independent
//     assembly-time defense currently sails through the CLI unrejected.
//   - `blocks.order` duplicate ids are silently double-resolved and
//     double-emitted (see the `resolve_blocks`/`record_brace_warnings` doc
//     comments in `dispcli_core.rs`, which call this out as "Task 10
//     territory").
//   - `substitute_placeholders`'s sequential per-placeholder `.replace()`
//     calls let an earlier substitution's *output* be rescanned by a later
//     placeholder's pass (branch is substituted before report_path).
//
// Fixtures live under `tests/fixtures/{worktree,weight-block-dangling,
// duplicate-block,single-pass,assembly-failed,resolution-failed,wiring}/`
// plus additions to `tests/fixtures/malformed/` — each self-contained
// (own profile/skill/block files), mirroring the `happy/`/`light/`
// convention, so none of them can perturb the Task 5 golden-file
// comparison.
// ─────────────────────────────────────────────────────────────────────────

/// Scenario: `worktree.required` is false when the registry requires a
/// worktree but this dispatch has no `envelope.worktree` — the AND, not
/// just the registry flag.
///
/// Given `agents.implementer.worktree_required = true` and
/// `envelope.worktree = null`
/// When `dispcli assemble` runs
/// Then it exits 0, `worktree.required` is `false`, and `worktree.commands`
/// is present and empty. *(R8 worktree block, worktree.required rule)*
///
/// Currently fails: `cmd/dispcli/main.rs`'s `try_assemble` reports
/// `required: agent_entry.worktree_required` unconditionally, so this
/// fixture reports `true` today.
#[test]
fn worktree_required_is_false_when_envelope_worktree_is_null_even_if_registry_requires_it() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("document.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("worktree/request-null.json"))
        .arg("--config")
        .arg(fixture("worktree/registry.toml"))
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
    let summary: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout was not a single JSON object: {e}\nstdout={stdout:?}"));

    assert_eq!(
        summary["worktree"]["required"], false,
        "worktree_required=true in the registry but envelope.worktree=null \
         must report required=false (AND, not the registry flag alone)"
    );
    assert_eq!(
        summary["worktree"]["commands"]
            .as_array()
            .expect("worktree.commands must be an array")
            .len(),
        0,
        "commands must be empty when the worktree does not apply"
    );

    let components = summary["size"]["components"]
        .as_array()
        .expect("size.components must be an array");
    assert_eq!(
        components.len(),
        5,
        "envelope, profile, skill:core, block:metrics, task_body — the \
         worktree-conditional block must NOT be included when \
         envelope.worktree is null (R2 include=\"worktree\" rule)"
    );
}

/// Scenario: `worktree.required` is true and `worktree.commands` carries
/// the argv `git worktree add` command when both the registry requires a
/// worktree and this dispatch supplies one.
///
/// Given `agents.implementer.worktree_required = true` and
/// `envelope.worktree = "/fixtures/repo/.worktrees/feature-t10-wt-set"`
/// When `dispcli assemble` runs
/// Then it exits 0, `worktree.required` is `true`, and
/// `worktree.commands[0]` is exactly the R8-example argv form — no shell
/// string. *(AC8.3, R8 worktree block)*
///
/// Currently fails: `commands` is hardcoded `Vec::new()` in
/// `cmd/dispcli/main.rs` regardless of worktree state.
#[test]
fn worktree_required_true_reports_argv_worktree_add_command_when_both_conditions_hold() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("document.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("worktree/request-set.json"))
        .arg("--config")
        .arg(fixture("worktree/registry.toml"))
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
    let summary: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout was not a single JSON object: {e}\nstdout={stdout:?}"));

    assert_eq!(summary["worktree"]["required"], true);
    let commands = summary["worktree"]["commands"]
        .as_array()
        .expect("worktree.commands must be an array");
    assert!(
        !commands.is_empty(),
        "commands must be non-empty when the worktree applies, got: {summary}"
    );
    let expected_argv = serde_json::json!([
        "git",
        "-C",
        "/fixtures/repo",
        "worktree",
        "add",
        "/fixtures/repo/.worktrees/feature-t10-wt-set",
        "-b",
        "feature-t10-wt-set"
    ]);
    assert_eq!(
        commands[0], expected_argv,
        "commands[0] must be the R8-example argv array — no shell string, \
         `-C <repo> worktree add <worktree-path> -b <branch>`"
    );

    // Byte accounting (AC8.1) under a genuinely dynamic block set: the
    // worktree-conditional block is now included, so this exercises a
    // component count the happy-path fixture (always-null worktree) never
    // does.
    let components = summary["size"]["components"]
        .as_array()
        .expect("size.components must be an array");
    assert_eq!(
        components.len(),
        6,
        "envelope, profile, skill:core, block:metrics, block:worktree-note, \
         task_body — the worktree-conditional block IS included when \
         envelope.worktree is non-null"
    );
    let total_bytes = summary["size"]["total_bytes"]
        .as_u64()
        .expect("size.total_bytes must be an integer");
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
        "components must sum exactly to total_bytes (AC8.1)"
    );
    let written_len = fs::metadata(&out_path)
        .expect("document was not written")
        .len();
    assert_eq!(
        total_bytes, written_len,
        "total_bytes must equal the written document's byte length (AC8.1)"
    );
}

/// Scenario: `worktree.required` stays false when the registry does NOT
/// require a worktree, even though this dispatch happens to supply one —
/// proving the rule is a genuine AND, not `envelope.worktree.is_some()`
/// alone.
///
/// Given `agents.implementer-passive.worktree_required = false` and
/// `envelope.worktree` non-null
/// When `dispcli assemble` runs
/// Then `worktree.required` is `false` and `worktree.commands` is empty.
/// *(R8 worktree block, worktree.required rule — the fourth truth-table
/// cell not covered by the other two worktree tests or the Task 5
/// happy-path fixture)*
#[test]
fn worktree_required_stays_false_when_registry_does_not_require_it_even_if_worktree_set() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("document.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("worktree/request-passive-set.json"))
        .arg("--config")
        .arg(fixture("worktree/registry.toml"))
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
    let summary: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout was not a single JSON object: {e}\nstdout={stdout:?}"));

    assert_eq!(
        summary["worktree"]["required"], false,
        "worktree_required=false in the registry must win even when \
         envelope.worktree is set"
    );
    assert_eq!(
        summary["worktree"]["commands"]
            .as_array()
            .expect("worktree.commands must be an array")
            .len(),
        0
    );
}

/// Scenario: a weight class's `blocks` list naming a fully-undeclared
/// block id (no `[blocks.<id>]` table anywhere in the registry) is
/// rejected as `config_invalid`, not silently dropped.
///
/// Given `[weights.ghost-block]` with `blocks = ["nonexistent-block"]`,
/// where `nonexistent-block` has no `[blocks.nonexistent-block]` table
/// When `dispcli assemble` runs with `weight: "ghost-block"`
/// Then it exits 4 (`config_invalid`), stdout stays empty, and the error
/// names the dangling reference. *(R2 AC2.2)*
///
/// **This is the tripwire test for `validate_registry` wiring** (item 7 of
/// the Task 10 dispatch): `resolve_blocks_for_weight` only ever iterates
/// `registry.blocks.order`, so an id absent from `blocks.order` (whether
/// declared elsewhere or not) is silently skipped — never resolved, never
/// erroring. Unlike the sibling `weights.<id>.skills` case (independently
/// defended by `resolve_fixed_skills`'s own `registry.skills.get(...)`
/// check), no other code path in `dispcli-core` can ever produce
/// `config_invalid` for this exact shape — only `validate_registry`
/// (already implemented, not yet called by `cmd/dispcli::try_assemble`)
/// can. Confirmed by manual probe: today this exits 0 and silently
/// assembles without the block.
///
/// **Distinct from, and must not be conflated with,** the AC6.3 sibling
/// test below (`..._declared_but_absent_from_order_...`): that one uses a
/// block id that IS declared as `[blocks.<id>]` but simply absent from
/// `blocks.order` — AC2.2 does not cover it (the table satisfies AC2.2's
/// declaration requirement), so it needs its own assertion. This test's
/// value is specifically that the id has NO declaration anywhere — the one
/// shape `validate_registry`'s AC2.2 check is the sole defense for.
#[test]
fn weight_class_dangling_block_reference_is_rejected_as_config_invalid_not_silently_dropped() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("unused.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("weight-block-dangling/request.json"))
        .arg("--config")
        .arg(fixture("weight-block-dangling/registry.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert_eq!(
        output.status.code(),
        Some(4),
        "a weight class's dangling `blocks` reference must be config_invalid \
         (exit 4), not a silent successful assembly. stdout={} stderr={}",
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
    assert_eq!(error["error"]["kind"], "config_invalid");
    let details = error["error"]["details"]
        .as_array()
        .expect("error.details must be an array");
    let has_dangling_reference = details.iter().any(|d| {
        d["value"].as_str() == Some("nonexistent-block")
            || d["value"]
                .as_str()
                .is_some_and(|v| v.contains("weights.ghost-block.blocks"))
    });
    assert!(
        has_dangling_reference,
        "error.details must name the dangling weights.ghost-block.blocks \
         reference, got: {details:?}"
    );
}

/// Scenario: a weight class's `blocks` list naming a block id that IS
/// declared as `[blocks.<id>]` — satisfying AC2.2's declaration
/// requirement — but is absent from `blocks.order`, is rejected as
/// `config_invalid`, not silently dropped.
///
/// Given `[blocks.declared-not-ordered]` (a real, resolvable block table)
/// and `[weights.ghost-block-ordered]` with
/// `blocks = ["declared-not-ordered"]`, where `"declared-not-ordered"` is
/// absent from `[blocks] order`
/// When `dispcli assemble` runs with `weight: "ghost-block-ordered"`
/// Then it exits 4 (`config_invalid`), stdout stays empty, and the error
/// names both the weight class and the unreachable id. *(R6, AC6.3 —
/// amendment 2026-07-18, spec commit b2f1627)*
///
/// Ratified text, quoted verbatim (spec lines 311-319) — this closes the
/// dispatch 1705 open question the `weight-block-dangling/` fixture's own
/// comment used to carve out:
///
/// > A block id named in a weight class's `blocks` list but absent from
/// > `blocks.order` is a config error naming the weight class and the
/// > unreachable id. **Declaration is not sufficient** ...: a
/// > `[blocks.<id>]` table satisfies AC2.2's declaration requirement while
/// > still being unreachable if `order` omits it, so AC2.2 does not cover
/// > this case.
///
/// **Distinct from the tripwire test above — does not replace it, and
/// must not be weakened into it.** That test's block id has no
/// `[blocks.<id>]` declaration anywhere (AC2.2's territory: `validate_registry`
/// checking `registry.blocks.blocks` is the only defense). This test's
/// block id IS declared — the same `registry.blocks.blocks` check
/// `validate_registry` already runs would pass it clean — but it's
/// unreachable because `blocks.order` omits it, which is the specific gap
/// AC6.3 closes and today's `validate_registry` does not check at all.
/// Both fixtures must independently produce `config_invalid`; a fix that
/// only satisfies one of the two tests is incomplete.
///
/// Currently fails, and for a reason distinct from the tripwire test's:
/// even once `validate_registry` is wired into `try_assemble` (closing the
/// tripwire test), `validate_registry`'s existing dangling-reference check
/// only asks "does `[blocks.<id>]` exist?" — true here — never "is `<id>`
/// reachable via `blocks.order`?" So this case needs `validate_registry`
/// itself to gain a new check, not merely to be wired in. Confirmed by
/// manual probe: today this exits 0 and the block is silently dropped
/// entirely — the assembled document's `size.components` has no
/// `block:declared-not-ordered` entry at all (envelope, profile, skill,
/// task_body only; `blocks.order` only lists `"metrics"`, which isn't in
/// this weight's `blocks` list either, so neither block is ever included).
#[test]
fn weight_class_block_declared_but_absent_from_order_is_rejected_as_config_invalid_per_ac6_3() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("unused.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture(
            "weight-block-dangling/request-declared-not-ordered.json",
        ))
        .arg("--config")
        .arg(fixture("weight-block-dangling/registry.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert_eq!(
        output.status.code(),
        Some(4),
        "a weight class's declared-but-unordered block reference must be \
         config_invalid (exit 4, AC6.3), not a silent successful assembly \
         that drops the block. stdout={} stderr={}",
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
    assert_eq!(error["error"]["kind"], "config_invalid");

    // AC6.3: "a config error naming the weight class and the unreachable
    // id" — both, not just one or the other.
    let detail_values: Vec<&str> = error["error"]["details"]
        .as_array()
        .expect("error.details must be an array")
        .iter()
        .filter_map(|d| d["value"].as_str())
        .collect();
    let names_weight_class = detail_values
        .iter()
        .any(|v| v.contains("ghost-block-ordered"));
    let names_unreachable_id = detail_values
        .iter()
        .any(|v| v.contains("declared-not-ordered"));
    assert!(
        names_weight_class,
        "AC6.3: error.details must name the weight class \
         'ghost-block-ordered', got: {detail_values:?}"
    );
    assert!(
        names_unreachable_id,
        "AC6.3: error.details must name the unreachable id \
         'declared-not-ordered', got: {detail_values:?}"
    );
}

/// Collect every `"value"` recorded under a given `"key"` from a
/// `config_invalid` error's `details` array, in array order — the
/// black-box counterpart to `dispcli-core`'s `Error::all_details`. Each
/// violating instance contributes a `field`/`value`/`reason` detail
/// triple (per-key, position-paired), so filtering by `key` and zipping
/// the resulting vectors by index recovers per-instance pairing without
/// assuming a specific insertion order between distinct instances.
fn all_detail_values_for_key<'a>(details: &'a [Value], key: &str) -> Vec<&'a str> {
    details
        .iter()
        .filter(|d| d["key"].as_str() == Some(key))
        .filter_map(|d| d["value"].as_str())
        .collect()
}

/// Scenario: a `[blocks.<id>]` table that is declared (a real, resolvable
/// path), absent from `blocks.order`, and named by NO weight class is
/// rejected as `config_invalid` — not silently dead config.
///
/// Given `[blocks.orphan-block]` (a real, resolvable block table) with no
/// entry in `[blocks] order` and no weight class's `blocks` list naming it
/// When `dispcli assemble` runs with `weight: "standard"` (whose
/// `blocks = "all"` sentinel never inspects `orphan-block` — see R6's "all"
/// vs. list distinction)
/// Then it exits 4 (`config_invalid`), stdout stays empty, no output file
/// is written, and the error names `orphan-block` with `reason = "orphan"`.
/// *(R6, AC6.4 — amendment 2026-07-18, spec commit pending)*
///
/// Ratified text, quoted verbatim (spec lines 320-323):
///
/// > Every declared `[blocks.<id>]` table must appear in `blocks.order`.
/// > A declared block absent from `order` is unreachable —
/// > `blocks.order` is the sole source of iteration order — and is a
/// > config error naming the unreachable id.
///
/// **Distinct from both sibling tests above — generalizes, does not
/// replace, AC6.3.** The AC6.3 test's `declared-not-ordered` id IS named
/// by a weight class (`weights.ghost-block-ordered.blocks`), so the
/// existing weight-scoped `unreachable` check in `validate_registry`
/// already catches it. This fixture's `orphan-block` is named by nothing —
/// `weights.standard.blocks = "all"` is the closed-vocabulary sentinel,
/// never an explicit id list (see
/// `validate_registry_accepts_weight_class_all_sentinel_without_treating_it_as_a_dangling_id`,
/// `libs/dispcli-core/tests/integration.rs`) — so no weight-scoped code
/// path ever inspects it. Only a registry-wide scan of every declared
/// `[blocks.<id>]` table against `blocks.order`, independent of any
/// weight, can catch this shape. Per the spec's reporting rule (lines
/// 327-331), this must land in the same combined `config_invalid` class
/// as AC2.2/AC6.3, distinguished by `reason = "orphan"` — not a new `Err`
/// class.
///
/// Currently fails: `validate_registry` has no check at all today for
/// "does every declared block appear in `order`?" — only the reverse
/// direction (does every `order` entry resolve to a declared table?,
/// AC2.2) and the weight-scoped unreachable check (AC6.3). Confirmed by
/// manual probe: today this registry validates cleanly and `dispcli
/// assemble` exits 0, silently never emitting `orphan-block`'s content
/// (it's absent from `order`, so `resolve_blocks_for_weight` never visits
/// it) — the exact "silently dead config" failure mode AC6.4's spec text
/// names.
#[test]
fn declared_block_absent_from_order_and_named_by_no_weight_is_rejected_as_config_invalid_per_ac6_4()
{
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("unused.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("weight-block-orphan/request.json"))
        .arg("--config")
        .arg(fixture("weight-block-orphan/registry.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert_eq!(
        output.status.code(),
        Some(4),
        "an orphan declared-but-unordered block must be config_invalid \
         (exit 4, AC6.4), not a silent successful assembly that drops the \
         block. stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout was not utf-8");
    assert!(
        stdout.is_empty(),
        "stdout must stay empty on failure (AC8.2), got: {stdout:?}"
    );
    assert!(
        !out_path.exists(),
        "no partial output on failure (AC1.1) — the document must not be written"
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr was not utf-8");
    let error: Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("stderr was not a single JSON object: {e}\nstderr={stderr:?}"));
    assert_eq!(error["error"]["kind"], "config_invalid");

    let details = error["error"]["details"]
        .as_array()
        .expect("error.details must be an array");
    let names_orphan_id = all_detail_values_for_key(details, "value")
        .iter()
        .any(|v| v.contains("orphan-block"));
    assert!(
        names_orphan_id,
        "AC6.4: error.details must name the orphan id 'orphan-block', \
         got: {details:?}"
    );
    let reasons = all_detail_values_for_key(details, "reason");
    assert!(
        reasons.contains(&"orphan"),
        "AC6.4: error.details must carry reason=\"orphan\" for a declared \
         block absent from blocks.order and named by no weight class \
         (distinct from AC2.2's \"undeclared\" and AC6.3's \
         \"unreachable\"), got reasons: {reasons:?}"
    );
}

/// Scenario: a registry that co-declares an AC6.4 orphan block (named by
/// no weight class) AND an unrelated AC2.2 dangling weight-block
/// reference reports BOTH instances, each with its own `reason`, in one
/// combined `config_invalid` error — neither masks the other.
///
/// Given `[blocks.orphan-block]` (declared, absent from `order`, named by
/// no weight — same shape as the fixture above) and, in the same
/// registry, `[weights.dangling]` with `blocks = ["nonexistent-block"]`
/// (no `[blocks.nonexistent-block]` table anywhere — AC2.2's dangling
/// shape)
/// When `dispcli assemble` runs with `weight: "dangling"`
/// Then it exits 4 (`config_invalid`) and the error's `details` name BOTH
/// `orphan-block` (`reason = "orphan"`) AND `nonexistent-block`
/// (`reason = "undeclared"`) together. *(R6, AC6.4 — "every instance
/// together, no masking", spec lines 327-331)*
///
/// **This is the black-box counterpart to the core-level regression
/// lock** `validate_registry_reports_both_undeclared_and_unreachable_instances_together_when_both_present`
/// (`libs/dispcli-core/tests/integration.rs`) — that test pins AC2.2 +
/// AC6.3 co-presence; this one pins AC6.4 + AC2.2 co-presence, and exists
/// specifically to stop a future implementer from reintroducing the
/// priority-ordered-split-by-defect-shape bug the ratified AC6.4 text
/// calls out by name: "any priority-ordered split lets one class mask
/// another, and masking regresses whichever pinned error payload lost." A
/// fix that gives AC6.4 its own `Err` class, or that returns early on the
/// first defect shape found, passes the single-defect fixture above but
/// fails this one — masking one instance behind the other.
///
/// Currently fails for two independent reasons, either of which is
/// sufficient on its own: (1) `validate_registry` has no AC6.4 orphan
/// check at all yet (same root cause as the fixture above), and (2) even
/// once one lands, an implementation that checks orphan status ahead of
/// and returns early on the AC2.2 dangling check (or vice versa) would
/// report only one instance here, not both — this test's value is
/// specifically catching that regression, not just proving AC6.4 exists.
#[test]
fn orphan_block_and_dangling_weight_block_reference_both_appear_in_one_combined_config_invalid_error_per_ac6_4_no_masking_rule()
 {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("unused.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("weight-block-orphan/request-with-dangling.json"))
        .arg("--config")
        .arg(fixture("weight-block-orphan/registry-with-dangling.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert_eq!(
        output.status.code(),
        Some(4),
        "a registry with both an AC6.4 orphan and an AC2.2 dangling \
         reference must be config_invalid (exit 4). stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout was not utf-8");
    assert!(
        stdout.is_empty(),
        "stdout must stay empty on failure (AC8.2), got: {stdout:?}"
    );
    assert!(
        !out_path.exists(),
        "no partial output on failure (AC1.1) — the document must not be written"
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr was not utf-8");
    let error: Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("stderr was not a single JSON object: {e}\nstderr={stderr:?}"));
    assert_eq!(error["error"]["kind"], "config_invalid");

    let details = error["error"]["details"]
        .as_array()
        .expect("error.details must be an array");
    let values = all_detail_values_for_key(details, "value");
    let reasons = all_detail_values_for_key(details, "reason");
    assert_eq!(
        values.len(),
        reasons.len(),
        "every violating instance must carry exactly one paired \
         value/reason detail — got {} value details and {} reason \
         details, from: {details:?}",
        values.len(),
        reasons.len()
    );

    // Position-pair value <-> reason (mirrors `Error::all_details`'s
    // documented per-instance triple pairing) and confirm BOTH instances
    // are present with the CORRECT reason each — not merely that both
    // values appear somewhere and both reasons appear somewhere, which
    // would also pass a buggy implementation that swapped which reason
    // goes with which value.
    let paired: Vec<(&str, &str)> = values.into_iter().zip(reasons).collect();
    let orphan_paired_correctly = paired
        .iter()
        .any(|(value, reason)| value.contains("orphan-block") && *reason == "orphan");
    let dangling_paired_correctly = paired
        .iter()
        .any(|(value, reason)| value.contains("nonexistent-block") && *reason == "undeclared");
    assert!(
        orphan_paired_correctly,
        "no-masking: error.details must pair 'orphan-block' with \
         reason=\"orphan\", got pairs: {paired:?}"
    );
    assert!(
        dangling_paired_correctly,
        "no-masking: error.details must pair 'nonexistent-block' with \
         reason=\"undeclared\" — the AC2.2 instance must survive \
         alongside the AC6.4 instance, neither masking the other, got \
         pairs: {paired:?}"
    );
}

/// Scenario: a duplicated id in `registry.blocks.order` is rejected, not
/// silently double-emitted.
///
/// Given `[blocks] order = ["metrics", "metrics"]`
/// When `dispcli assemble` runs
/// Then it exits 4 (`config_invalid`) and stdout stays empty.
///
/// Currently fails: today this exits 0 and the block's content is
/// resolved and emitted **twice** (confirmed by manual probe — two
/// identical `block:metrics` components in the summary, two copies of the
/// block's prose in the document). See the `record_brace_warnings` doc
/// comment in `dispcli_core.rs`, which names this "Task 10 territory."
#[test]
fn duplicate_block_order_entry_is_rejected_as_config_invalid_not_double_emitted() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("unused.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("duplicate-block/request.json"))
        .arg("--config")
        .arg(fixture("duplicate-block/registry.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert_eq!(
        output.status.code(),
        Some(4),
        "a duplicated blocks.order id must be config_invalid (exit 4), not \
         a silent double emission. stdout={} stderr={}",
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
    assert_eq!(error["error"]["kind"], "config_invalid");
}

/// Scenario: a 39-char `parent_commit` — one short of R7's 40-hex
/// requirement — is rejected end-to-end through the CLI, proving
/// `validate_request` is actually wired into `try_assemble`'s pipeline
/// (not merely an unwired `dispcli-core` function the unit tests call
/// directly).
///
/// Given a request whose `envelope.parent_commit` is 39 hex characters
/// When `dispcli assemble` runs
/// Then it exits 3 (`request_invalid`), stdout stays empty, and the error
/// names `envelope.parent_commit`. *(R7, AC7.2)*
///
/// Currently fails: nothing in the assembly path (`assemble`,
/// `Envelope::from_request`) validates `parent_commit`'s shape — it is
/// copied into the envelope verbatim — so today this exits 0 with the
/// malformed SHA baked into the written document (confirmed by manual
/// probe). `validate_request` already rejects this
/// (`validate_request_rejects_39_char_parent_commit` passes at the
/// `dispcli-core` unit level) but `cmd/dispcli::try_assemble` never calls
/// it.
#[test]
fn bad_parent_commit_sha_is_rejected_as_request_invalid_through_the_cli() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("unused.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("wiring/request-bad-parent-commit.json"))
        .arg("--config")
        .arg(fixture("happy/registry.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert_eq!(
        output.status.code(),
        Some(3),
        "a 39-char parent_commit must be request_invalid (exit 3) end-to-end. \
         stdout={} stderr={}",
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
    let fields = error["error"]["details"]
        .as_array()
        .expect("error.details must be an array")
        .iter()
        .filter_map(|d| d["value"].as_str())
        .collect::<Vec<_>>();
    assert!(
        fields.contains(&"envelope.parent_commit"),
        "error.details must name envelope.parent_commit, got: {fields:?}"
    );
    assert!(
        !out_path.exists(),
        "no partial output on failure (AC1.1) — the document must not be written"
    );
}

/// Scenario: a `branch` value containing the literal token `{report_path}`
/// survives single-pass substitution as a literal (is NOT second-order-
/// substituted) — and *because* it then survives into the assembled
/// skill section, it trips AC5.2's unresolved-supported-placeholder check.
///
/// Given `envelope.branch = "x{report_path}y"` and a skill whose only
/// placeholder is `{branch}`
/// When `dispcli assemble` runs
/// Then it exits 6 (`assembly_failed`), stdout stays empty, and the error
/// names the offending section and placeholder.
///
/// **Ratified reading (amends the original brief item 9, which conflated
/// AC5.3 with AC5.2 — see dispatch 1705's amendment).** `{report_path}` is
/// in R5's supported-placeholder table (spec lines 255-263). AC5.2 (spec
/// lines 269-275), quoted verbatim:
///
/// > Any `{placeholder}` from the supported set remaining unsubstituted in
/// > a substituted section — skills or template blocks, per R5's
/// > substitution scope ... — is an assembly error, not a warning. A
/// > supported placeholder left in the profile or task body, which R5
/// > passes through verbatim, is **not** an error and **not** an AC5.3
/// > warning ...; this silent passthrough is an accepted v0 hole.
///
/// A skill section is exactly the scope AC5.2 covers (the profile/task-body
/// passthrough hole is explicitly narrower than "everywhere") — so a
/// substitution-introduced `{report_path}` surviving into a skill section
/// is the assembly error, not a silent pass-through. "Survives verbatim
/// rather than being replaced" (AC5.3's rule) only applies to tokens
/// *outside* the supported set; `{report_path}` is inside it.
///
/// This stays a genuine red test today, for the same underlying defect the
/// original brief identified: `substitute_placeholders` runs the
/// `{branch}` `.replace` call before the `{report_path}` `.replace` call,
/// so the `{report_path}` token this substitution just introduced gets
/// rescanned and replaced by the very next line — the assembled skill
/// content today reads
/// `Working branch: x/fixtures/repo/scratch/dispatch-801-report.mdy` and
/// assembly **succeeds** (exit 0), never reaching the AC5.2 check at all.
/// Once the single-pass fix lands, the literal token will survive into the
/// section content, and *that* must then trip AC5.2 — exit 6, not exit 0.
#[test]
fn branch_value_containing_report_path_token_reports_assembly_failed_per_ac5_2() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("unused.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("single-pass/request-report-path.json"))
        .arg("--config")
        .arg(fixture("single-pass/registry.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert_eq!(
        output.status.code(),
        Some(6),
        "a branch value that single-pass-substitutes to a literal, \
         supported-but-unresolved {{report_path}} token in a skill section \
         must be assembly_failed (exit 6, AC5.2) — not silently second-\
         order-substituted to a successful exit 0. stdout={} stderr={}",
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
    assert_eq!(error["error"]["kind"], "assembly_failed");
    assert_eq!(error["error"]["details"][0]["value"], "skill:branch-echo");
    assert_eq!(error["error"]["details"][1]["value"], "{report_path}");
}

/// Scenario: a `branch` value containing the literal token `{task_id}`,
/// with `envelope.task_id` null, single-pass-substitutes to a literal and
/// trips AC5.2 — the exact sibling of the `{report_path}` case above,
/// same AC5.2 reasoning (see that test's doc comment for the ratified
/// quote from spec lines 269-275; `{task_id}` is also in R5's
/// supported-placeholder table, spec line 258).
///
/// Given `envelope.branch = "x{task_id}y"`, `envelope.task_id = null`, and
/// a skill whose only placeholder is `{branch}`
/// When `dispcli assemble` runs
/// Then it exits 6 (`assembly_failed`), naming the section that used
/// `{branch}` (not some unrelated skill) and the `{task_id}` placeholder.
///
/// **This test is a regression lock, not a red test — it passes today.**
/// Per manual probe (predating this amendment), today's actual behavior is
/// already `exit 6` with correct attribution to `skill:branch-echo` — the
/// section that used `{branch}`, not some other section. Kept and labeled
/// explicitly rather than deleted: a naive single-pass fix to the
/// `{report_path}` defect above could plausibly special-case "skip the
/// AC5.2 check on any substitution-introduced token," which would silently
/// regress *this* passing case to exit 0. The attribution assertions catch
/// that a fix also doesn't accidentally blame a different section.
#[test]
fn branch_value_containing_task_id_token_reports_assembly_failed_per_ac5_2_regression_lock() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("unused.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("single-pass/request-task-id.json"))
        .arg("--config")
        .arg(fixture("single-pass/registry.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert_eq!(
        output.status.code(),
        Some(6),
        "regression lock: a branch-introduced, unresolved {{task_id}} token \
         in a skill section must stay assembly_failed (exit 6, AC5.2). \
         stdout={} stderr={}",
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
    assert_eq!(error["error"]["kind"], "assembly_failed");
    assert_eq!(
        error["error"]["details"][0]["value"], "skill:branch-echo",
        "regression lock: attribution must name the section that actually \
         used {{branch}} — not an unrelated skill"
    );
    assert_eq!(error["error"]["details"][1]["value"], "{task_id}");
}

/// Scenario: `dispcli assemble` invoked without the required `--request`
/// flag is a usage error.
///
/// Given no `--request` argument
/// When `dispcli assemble --config <cfg>` runs
/// Then it exits 2 (`usage`) and stdout stays empty. *(R8 error table,
/// AC8.2)*
///
/// Not asserted: stderr's exact shape. Clap's own argument-parsing
/// failure produces its standard human-readable usage text, not the
/// `{"error": {...}}` JSON shape `report_error` emits for every other
/// kind — `ErrorKind::Usage`'s doc comment says it's "never emitted by
/// this crate," which reads as intentional, but R8's stderr-JSON
/// contract is stated without an explicit usage carve-out. Flagged as a
/// spec ambiguity in the completion report rather than asserted either
/// way here.
#[test]
fn usage_error_for_missing_required_flag_exits_2_with_empty_stdout() {
    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--config")
        .arg(fixture("happy/registry.toml"))
        .output()
        .expect("binary failed to execute");

    assert_eq!(
        output.status.code(),
        Some(2),
        "a missing required flag must be a usage error (exit 2). stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout was not utf-8");
    assert!(
        stdout.is_empty(),
        "stdout must stay empty on failure (AC8.2), got: {stdout:?}"
    );
}

/// Scenario: a registry-declared skill path with no file on disk is
/// `resolution_failed`, end-to-end through the CLI.
///
/// Given `[skills.core] path = "skills/does-not-exist.md"` and no such
/// file
/// When `dispcli assemble` runs
/// Then it exits 5 (`resolution_failed`), stdout stays empty, and the
/// error names the registry id and the resolved path. *(AC3.3, AC8.2)*
#[test]
fn missing_skill_file_reports_resolution_failed_through_the_cli() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("unused.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("resolution-failed/request.json"))
        .arg("--config")
        .arg(fixture("resolution-failed/registry.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert_eq!(
        output.status.code(),
        Some(5),
        "a missing skill file must be resolution_failed (exit 5). stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout was not utf-8");
    assert!(
        stdout.is_empty(),
        "stdout must stay empty on failure, got: {stdout:?}"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr was not utf-8");
    let error: Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("stderr was not a single JSON object: {e}\nstderr={stderr:?}"));
    assert_eq!(error["error"]["kind"], "resolution_failed");
    let fields = error["error"]["details"]
        .as_array()
        .expect("error.details must be an array")
        .iter()
        .filter_map(|d| d["key"].as_str())
        .collect::<Vec<_>>();
    assert!(
        fields.contains(&"id") && fields.contains(&"path") && fields.contains(&"cause"),
        "resolution_failed details must carry id/path/cause (AC3.3), got: {fields:?}"
    );
}

/// Scenario: an always-included skill authoring `{task_id}` verbatim, in a
/// no-task dispatch, is `assembly_failed`, end-to-end through the CLI.
///
/// Given a skill whose content is `Task id: {task_id}` and
/// `envelope.task_id = null`
/// When `dispcli assemble` runs
/// Then it exits 6 (`assembly_failed`), stdout stays empty, and the error
/// names the offending section and placeholder. *(AC5.2, AC8.2)*
///
/// Contrast with the single-pass tests above: here `{task_id}` is
/// author-written in the skill's own source file — the intentional AC5.2
/// failure mode — not introduced by another placeholder's substitution.
#[test]
fn author_written_unresolved_placeholder_reports_assembly_failed_through_the_cli() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("unused.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("assembly-failed/request.json"))
        .arg("--config")
        .arg(fixture("assembly-failed/registry.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert_eq!(
        output.status.code(),
        Some(6),
        "an author-written unresolved placeholder must be assembly_failed \
         (exit 6). stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout was not utf-8");
    assert!(
        stdout.is_empty(),
        "stdout must stay empty on failure, got: {stdout:?}"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr was not utf-8");
    let error: Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("stderr was not a single JSON object: {e}\nstderr={stderr:?}"));
    assert_eq!(error["error"]["kind"], "assembly_failed");
    assert_eq!(error["error"]["details"][0]["value"], "skill:needs-task");
    assert_eq!(error["error"]["details"][1]["value"], "{task_id}");
}

/// Scenario: a `DocumentSink` write failure — the output path's parent
/// cannot be created because a path component is an existing regular
/// file, not a directory — is `io_failed`, end-to-end through the CLI.
///
/// Given `--out <tempdir>/blocker-file/subdir/document.md` where
/// `blocker-file` already exists as a plain file
/// When `dispcli assemble` runs
/// Then it exits 7 (`io_failed`) and stdout stays empty. *(AC8.2)*
#[test]
fn write_failure_reports_io_failed_through_the_cli() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let blocker = tempdir.path().join("blocker-file");
    fs::write(&blocker, b"not a directory").expect("failed to write blocker file");
    let out_path = blocker.join("subdir").join("document.md");

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

    assert_eq!(
        output.status.code(),
        Some(7),
        "a sink write failure must be io_failed (exit 7). stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout was not utf-8");
    assert!(
        stdout.is_empty(),
        "stdout must stay empty on failure, got: {stdout:?}"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr was not utf-8");
    let error: Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("stderr was not a single JSON object: {e}\nstderr={stderr:?}"));
    assert_eq!(error["error"]["kind"], "io_failed");
}

/// Scenario: a syntactically invalid registry TOML file is
/// `config_invalid`, end-to-end through the CLI.
///
/// Given a `--config` file with an unterminated table header
/// When `dispcli assemble` runs
/// Then it exits 4 (`config_invalid`) and stdout stays empty. *(AC8.2)*
#[test]
fn malformed_registry_toml_reports_config_invalid_through_the_cli() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("unused.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture("happy/request.json"))
        .arg("--config")
        .arg(fixture("malformed/registry-malformed.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert_eq!(
        output.status.code(),
        Some(4),
        "malformed registry TOML should map to config_invalid (exit 4). \
         stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout was not utf-8");
    assert!(
        stdout.is_empty(),
        "stdout must stay empty on failure, got: {stdout:?}"
    );
}

/// Scenario: a fuzz-ish sweep of malformed request inputs across the
/// taxonomy never panics — every one produces a structured JSON error on
/// stderr, an exit code in the documented R8 range, and an empty stdout.
///
/// *(AC8.4 — "no panics anywhere in the taxonomy")*
#[test]
fn malformed_inputs_never_panic_and_always_report_structured_errors() {
    let cases: &[(&str, &str)] = &[
        ("truncated JSON", "malformed/request-malformed.json"),
        ("wrong field type", "malformed/request-wrong-type.json"),
        ("invalid UTF-8 bytes", "malformed/request-invalid-utf8.bin"),
    ];

    for (label, rel_path) in cases {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let out_path = tempdir.path().join("unused.md");

        let output = Command::cargo_bin("dispcli")
            .unwrap()
            .arg("assemble")
            .arg("--request")
            .arg(fixture(rel_path))
            .arg("--config")
            .arg(fixture("happy/registry.toml"))
            .arg("--out")
            .arg(&out_path)
            .output()
            .expect("binary failed to execute");

        let code = output
            .status
            .code()
            .unwrap_or_else(|| panic!("[{label}] process terminated by signal, not a clean exit"));
        assert!(
            (2..=7).contains(&code),
            "[{label}] exit code must be in the R8 range 2..=7, got {code}"
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.is_empty(),
            "[{label}] stdout must stay empty on failure, got: {stdout:?}"
        );

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("panicked at"),
            "[{label}] a Rust panic message on stderr means AC8.4 was violated \
             (a production path panicked instead of returning a structured \
             error) — stderr={stderr:?}"
        );
        let _error: Value = serde_json::from_str(stderr.trim()).unwrap_or_else(|e| {
            panic!("[{label}] stderr was not a single structured JSON error object: {e}\nstderr={stderr:?}")
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Task 10 RED PHASE, third increment — R7 `branch` validation, end-to-end
// through the CLI (spec 0001-envelope-assembly.md R7 rule table, `branch`
// row; AC7.1, AC7.2).
//
// Coverage hole closed by dispatch 1705's second amendment: the `branch`
// rule was added by the 2026-07-17 spec amendment (commit 06517e82), after
// Task 6 (which owns R7 validation) had already merged — a ratified rule
// the plan's Task 10 section never picked up. `validate_request` has zero
// branch-shape checking today; confirmed by `git grep -n branch` over
// `dispcli_core.rs` finding nothing beyond the type field and the
// substitution call site.
//
// Ratified rule text, transcribed verbatim (spec line 319):
//   "Required, non-empty, and a valid git ref name (`git check-ref-format`
//   semantics): no control characters or spaces, no `..`, no leading `-`,
//   no trailing `.lock`, and none of `~ ^ : ? * [`."
//
// Each rejection fixture below isolates exactly ONE clause — every other
// field is fully R1/R7/R2-valid (40-hex parent_commit, absolute repo,
// known agent/pattern/weight) and uses `happy/registry.toml` unmodified —
// so once branch validation is implemented, only the branch check can
// possibly fire for these. The acceptance fixtures prove the boundary the
// other direction: an over-strict implementation is exactly as much a bug
// as an under-strict one (AC7.1's "one rejection test and one acceptance
// test at the boundary" for every rule).
//
// Deliberately NOT tested (per instruction — `git check-ref-format` has
// rules this spec row doesn't enumerate: a single `@`, consecutive
// slashes, a trailing slash, empty path components between slashes).
// Testing those would encode a rule the ratified text doesn't state.
// ─────────────────────────────────────────────────────────────────────────

/// Scenario: a `branch` containing a metacharacter from the forbidden set
/// `~ ^ : ? * [` is rejected.
///
/// Given `envelope.branch = "feature?bad"`
/// When `dispcli assemble` runs
/// Then it exits 3 (`request_invalid`), stdout stays empty, and the error
/// names `envelope.branch` and the offending value. *(R7 branch row,
/// AC7.1, AC7.2)*
///
/// Currently fails: nothing in `validate_request` checks `branch`'s shape
/// at all, so this exits 0 today with the metacharacter baked verbatim
/// into the written envelope.
#[test]
fn branch_with_forbidden_metacharacter_is_rejected_as_request_invalid() {
    assert_branch_rejected("branch-validation/request-metachar.json", "feature?bad");
}

/// Scenario: a `branch` containing a `..` sequence is rejected.
///
/// Given `envelope.branch = "feature..bad"`
/// When `dispcli assemble` runs
/// Then it exits 3 (`request_invalid`) naming `envelope.branch` and the
/// offending value. *(R7 branch row)*
#[test]
fn branch_with_dotdot_sequence_is_rejected_as_request_invalid() {
    assert_branch_rejected("branch-validation/request-dotdot.json", "feature..bad");
}

/// Scenario: a `branch` starting with `-` is rejected.
///
/// Given `envelope.branch = "-feature-bad"`
/// When `dispcli assemble` runs
/// Then it exits 3 (`request_invalid`) naming `envelope.branch` and the
/// offending value. *(R7 branch row)*
#[test]
fn branch_with_leading_dash_is_rejected_as_request_invalid() {
    assert_branch_rejected(
        "branch-validation/request-leading-dash.json",
        "-feature-bad",
    );
}

/// Scenario: a `branch` ending in `.lock` is rejected.
///
/// Given `envelope.branch = "feature-bad.lock"`
/// When `dispcli assemble` runs
/// Then it exits 3 (`request_invalid`) naming `envelope.branch` and the
/// offending value. *(R7 branch row)*
#[test]
fn branch_with_trailing_dot_lock_is_rejected_as_request_invalid() {
    assert_branch_rejected(
        "branch-validation/request-trailing-lock.json",
        "feature-bad.lock",
    );
}

/// Scenario: a `branch` containing a space is rejected — the chosen
/// representative of the rule's "no control characters or spaces" clause
/// (a space is named explicitly in the rule text; a literal control
/// character would assert the identical code path).
///
/// Given `envelope.branch = "feature bad"`
/// When `dispcli assemble` runs
/// Then it exits 3 (`request_invalid`) naming `envelope.branch` and the
/// offending value. *(R7 branch row)*
#[test]
fn branch_with_embedded_space_is_rejected_as_request_invalid() {
    assert_branch_rejected("branch-validation/request-space.json", "feature bad");
}

/// Scenario: an empty `branch` is rejected — the rule's "non-empty" clause,
/// distinct from the character/sequence rules: an empty string parses fine
/// at the JSON-shape level (the field is a required `String`, not
/// `Option<String>`, so *omitting* it is already a parse-time failure —
/// but an empty string still satisfies that shape), so this needs its own
/// check.
///
/// Given `envelope.branch = ""`
/// When `dispcli assemble` runs
/// Then it exits 3 (`request_invalid`) naming `envelope.branch`. *(R7
/// branch row: "Required, non-empty, ...")*
#[test]
fn empty_branch_is_rejected_as_request_invalid() {
    assert_branch_rejected("branch-validation/request-empty.json", "");
}

/// Shared assertion body for the six branch-rejection scenarios above:
/// exit 3, empty stdout, structured `request_invalid` stderr naming
/// `envelope.branch` as the field and `expected_value` as the offending
/// value (AC7.2). Not a loop-based fuzz sweep (contrast with
/// `malformed_inputs_never_panic_and_always_report_structured_errors`) —
/// each scenario is its own named `#[test]` per the BDD one-scenario-per-
/// function convention (`skills/bdd.md`); this only factors out the
/// identical assertion body so six near-identical blocks don't drift.
#[cfg(test)]
fn assert_branch_rejected(request_fixture: &str, expected_value: &str) {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("unused.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture(request_fixture))
        .arg("--config")
        .arg(fixture("happy/registry.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert_eq!(
        output.status.code(),
        Some(3),
        "[{request_fixture}] an invalid branch must be request_invalid \
         (exit 3) end-to-end. stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout was not utf-8");
    assert!(
        stdout.is_empty(),
        "[{request_fixture}] stdout must stay empty on failure (AC8.2), got: {stdout:?}"
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr was not utf-8");
    let error: Value = serde_json::from_str(stderr.trim()).unwrap_or_else(|e| {
        panic!("[{request_fixture}] stderr was not a single JSON object: {e}\nstderr={stderr:?}")
    });
    assert_eq!(error["error"]["kind"], "request_invalid");

    let details = error["error"]["details"]
        .as_array()
        .expect("error.details must be an array");
    let field_paths: Vec<&str> = details
        .iter()
        .filter(|d| d["key"].as_str() == Some("field"))
        .filter_map(|d| d["value"].as_str())
        .collect();
    let offending_values: Vec<&str> = details
        .iter()
        .filter(|d| d["key"].as_str() == Some("value"))
        .filter_map(|d| d["value"].as_str())
        .collect();
    assert!(
        field_paths.contains(&"envelope.branch"),
        "[{request_fixture}] AC7.2: error.details must identify the field \
         path envelope.branch, got field paths: {field_paths:?}"
    );
    assert!(
        offending_values.contains(&expected_value),
        "[{request_fixture}] AC7.2: error.details must carry the offending \
         value {expected_value:?}, got: {offending_values:?}"
    );
    assert!(
        !out_path.exists(),
        "[{request_fixture}] no partial output on failure (AC1.1)"
    );
}

/// Scenario: a slash-namespaced branch (`feature/t10-output-contract`) is
/// a legal git ref name and must not be rejected.
///
/// Given `envelope.branch = "feature/t10-output-contract"`
/// When `dispcli assemble` runs
/// Then it exits 0 and the written document's envelope carries the branch
/// verbatim. *(R7 branch row — acceptance boundary, AC7.1)*
///
/// Passes today (nothing rejects any branch yet) — this is the acceptance
/// half of AC7.1's boundary pair, not a red test. It becomes load-bearing
/// once branch validation lands: a validator that treats `/` as
/// disallowed (confusing it with a forbidden metacharacter, none of which
/// include `/`) would regress this from green to a false rejection.
#[test]
fn branch_with_slash_namespace_is_accepted() {
    assert_branch_accepted(
        "branch-validation/request-valid-slash.json",
        "feature/t10-output-contract",
    );
}

/// Scenario: `.lock` is only forbidden as a *trailing* element — a branch
/// containing `.lock` mid-string is legal.
///
/// Given `envelope.branch = "my.lock.branch"`
/// When `dispcli assemble` runs
/// Then it exits 0 and the written document's envelope carries the branch
/// verbatim. *(R7 branch row — acceptance boundary)*
///
/// Passes today, same rationale as the slash test above. This is the
/// specific case the dispatch called out as the sharpest way to over-
/// strict-ify the rule: a validator that greps for the substring `.lock`
/// anywhere in the branch, rather than checking only the trailing
/// position, would reject this legal value.
#[test]
fn branch_with_non_trailing_dot_lock_is_accepted() {
    assert_branch_accepted(
        "branch-validation/request-valid-lock-not-trailing.json",
        "my.lock.branch",
    );
}

/// Scenario: curly braces are not in the R7 metacharacter set
/// (`~ ^ : ? * [`) — a brace-bearing branch is legal.
///
/// Given `envelope.branch = "x{report_path}y"` — the exact value
/// `single-pass/request-report-path.json` uses
/// When `dispcli assemble` runs
/// Then it exits 0 and the written document's envelope carries the branch
/// verbatim. *(R7 branch row — acceptance boundary)*
///
/// This is the specific check the dispatch asked to verify rather than
/// assume: the existing single-pass fixtures (`x{report_path}y`,
/// `x{task_id}y`) use brace-bearing branch values for an unrelated
/// property (AC5.2 single-pass substitution). If a future branch-
/// validation implementation over-reached and rejected braces, those two
/// tests would silently flip from their intended failure mode
/// (assembly_failed, exit 6) to request_invalid (exit 3) — a
/// wrong-reason regression this test catches independently, in isolation
/// from AC5.2's own machinery (this fixture's skill has no placeholders
/// at all, so nothing but branch validation can reject or fail it).
#[test]
fn branch_with_braces_is_accepted_matching_the_single_pass_fixtures() {
    assert_branch_accepted(
        "branch-validation/request-valid-braces.json",
        "x{report_path}y",
    );
}

/// Shared assertion body for the three branch-acceptance scenarios above:
/// exit 0, and the written document's envelope contains `branch:
/// <expected_value>` verbatim (proving the value was neither rejected nor
/// mangled).
#[cfg(test)]
fn assert_branch_accepted(request_fixture: &str, expected_branch: &str) {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let out_path = tempdir.path().join("document.md");

    let output = Command::cargo_bin("dispcli")
        .unwrap()
        .arg("assemble")
        .arg("--request")
        .arg(fixture(request_fixture))
        .arg("--config")
        .arg(fixture("happy/registry.toml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("binary failed to execute");

    assert!(
        output.status.success(),
        "[{request_fixture}] a legal branch value must not be rejected. \
         stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let document = fs::read_to_string(&out_path).expect("document was not written");
    let expected_line = format!("branch: {expected_branch}");
    assert!(
        document.contains(&expected_line),
        "[{request_fixture}] expected the envelope to carry `{expected_line}` \
         verbatim — got document:\n{document}"
    );
}
