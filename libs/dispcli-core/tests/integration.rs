//! Integration tests for `dispcli-core` — Task 1 (types + parse) and
//! Task 2 (resolver traits + error taxonomy).
//!
//! Exercises the public parse functions against string fixtures matching
//! the R1/R2 example shapes in `docs/specs/0001-envelope-assembly.md`,
//! the R8 error-kind→exit-code mapping, and the R3 `ContentResolver`/
//! `DocumentSink` traits via in-memory fakes. No filesystem —
//! `dispcli-core` is IO-free (architectural invariant, AC3.1).

use std::cell::RefCell;
use std::collections::BTreeMap;

use dispcli_core::{
    ContentResolver, DocumentSink, Error, ErrorKind, Include, PermissionMode, Tier, parse_registry,
    parse_request,
};

const REQUEST_JSON: &str = r#"{
    "agent": "implementer",
    "task_pattern": "implementation",
    "tier": "t2",
    "weight": "standard",
    "mode_override": null,
    "skills_add": [],
    "skills_remove": [],
    "task_body": "Do the thing.",
    "envelope": {
        "dispatch_id": 42,
        "task_id": 7,
        "spec_id": "docs/specs/0001-envelope-assembly.md",
        "spec_version": "e2aca810f3f5a11c880beb555bf3ac0be2466e17",
        "parent_commit": "e2aca810f3f5a11c880beb555bf3ac0be2466e17",
        "repo": "/abs/path/to/repo",
        "worktree": null,
        "branch": "feature-x",
        "report_path": null,
        "deadline_minutes": null,
        "command_scope_subtract": [],
        "command_scope_add": [],
        "touch_scope": [],
        "forbid_scope": [],
        "verify": []
    }
}"#;

const REQUEST_WITH_UNKNOWN_TOP_LEVEL_FIELD: &str = r#"{
    "agent": "implementer",
    "task_pattern": "implementation",
    "tier": "t2",
    "task_body": "Do the thing.",
    "nope": true,
    "envelope": {
        "dispatch_id": 42,
        "parent_commit": "e2aca810f3f5a11c880beb555bf3ac0be2466e17",
        "repo": "/abs/path/to/repo",
        "branch": "feature-x"
    }
}"#;

const REQUEST_WITH_UNKNOWN_ENVELOPE_FIELD: &str = r#"{
    "agent": "implementer",
    "task_pattern": "implementation",
    "tier": "t2",
    "task_body": "Do the thing.",
    "envelope": {
        "dispatch_id": 42,
        "parent_commit": "e2aca810f3f5a11c880beb555bf3ac0be2466e17",
        "repo": "/abs/path/to/repo",
        "branch": "feature-x",
        "nope": true
    }
}"#;

const REGISTRY_TOML: &str = r#"
[registry]
skills_root = "skills"

[agents.implementer]
profile = "team/implementer.md"
default_mode = "bypassPermissions"
worktree_required = true

[agents.researcher]
profile = "team/researcher.md"
default_mode = "default"
worktree_required = false

[skills.rust]
path = "skills/rust.md"

[skills.verify]
path = "skills/verify.md"

[patterns.implementation]
skills = ["verify", "rust", "tdd"]

[blocks]
order = ["metrics", "completion-report", "merge-msg", "task-tracking", "working-dir", "scope-boundaries"]

[blocks.metrics]
path = "skills/dispatch-metrics.md"
include = "always"

[blocks.merge-msg]
path = "skills/dispatch-merge-msg.md"
include = "worktree"

[blocks.task-tracking]
path = "skills/dispatch-task-tracking.md"
include = "task"

[weights.standard]
profile_sections = "all"
blocks = "all"

[weights.light]
profile_sections = ["role", "persona", "command-scope"]
skills = ["verify"]
blocks = ["metrics", "working-dir", "scope-boundaries"]
"#;

#[test]
fn version_is_non_empty() {
    assert!(!dispcli_core::version().is_empty());
}

#[test]
fn request_deserializes_from_r1_example_shape() {
    let request = parse_request(REQUEST_JSON).expect("well-formed request should parse");
    assert_eq!(request.agent, "implementer");
    assert_eq!(request.task_pattern, "implementation");
    assert_eq!(request.tier, Tier::T2);
    assert_eq!(request.weight, "standard");
    assert_eq!(request.mode_override, None);
    assert!(request.skills_add.is_empty());
    assert!(request.skills_remove.is_empty());
    assert_eq!(request.envelope.dispatch_id, 42);
    assert_eq!(request.envelope.task_id, Some(7));
    assert_eq!(
        request.envelope.spec_id.as_deref(),
        Some("docs/specs/0001-envelope-assembly.md")
    );
    assert_eq!(request.envelope.branch, "feature-x");
    assert_eq!(request.envelope.parent_commit.len(), 40);
    assert_eq!(request.envelope.worktree, None);
    assert_eq!(request.envelope.report_path, None);
}

#[test]
fn request_applies_default_weight_when_omitted() {
    let minimal = r#"{
        "agent": "implementer",
        "task_pattern": "implementation",
        "tier": "t1",
        "task_body": "Do the thing.",
        "envelope": {
            "dispatch_id": 1,
            "parent_commit": "e2aca810f3f5a11c880beb555bf3ac0be2466e17",
            "repo": "/abs/path/to/repo",
            "branch": "feature-x"
        }
    }"#;
    let request = parse_request(minimal).expect("minimal request should parse");
    assert_eq!(request.weight, "standard");
}

#[test]
fn request_rejects_unknown_top_level_field() {
    let result = parse_request(REQUEST_WITH_UNKNOWN_TOP_LEVEL_FIELD);
    assert!(
        result.is_err(),
        "unknown top-level key should fail to parse"
    );
}

#[test]
fn request_rejects_unknown_envelope_field() {
    let result = parse_request(REQUEST_WITH_UNKNOWN_ENVELOPE_FIELD);
    assert!(result.is_err(), "unknown envelope key should fail to parse");
}

#[test]
fn registry_deserializes_from_r2_example_shape() {
    let registry = parse_registry(REGISTRY_TOML).expect("well-formed registry should parse");
    assert_eq!(registry.registry.skills_root, "skills");

    assert_eq!(registry.agents.len(), 2);
    assert_eq!(
        registry.agents["implementer"].default_mode,
        PermissionMode::BypassPermissions
    );
    assert!(registry.agents["implementer"].worktree_required);
    assert_eq!(
        registry.agents["researcher"].default_mode,
        PermissionMode::Default
    );
    assert!(!registry.agents["researcher"].worktree_required);

    assert_eq!(registry.skills.len(), 2);
    assert_eq!(registry.skills["rust"].path, "skills/rust.md");

    assert_eq!(
        registry.patterns["implementation"].skills,
        vec!["verify", "rust", "tdd"]
    );

    assert_eq!(registry.blocks.order.len(), 6);
    assert_eq!(registry.blocks.blocks["metrics"].include, Include::Always);
    assert_eq!(
        registry.blocks.blocks["merge-msg"].include,
        Include::Worktree
    );
    assert_eq!(
        registry.blocks.blocks["task-tracking"].include,
        Include::Task
    );

    assert_eq!(registry.weights.len(), 2);
    assert!(registry.weights["standard"].skills.is_none());
}

#[test]
fn tier_round_trips_through_all_variants() {
    let cases = [
        (Tier::T1, "\"t1\""),
        (Tier::T2, "\"t2\""),
        (Tier::T3, "\"t3\""),
    ];
    for (variant, wire) in cases {
        let json = serde_json::to_string(&variant).expect("tier should serialize");
        assert_eq!(json, wire);
        let back: Tier = serde_json::from_str(&json).expect("tier should round-trip");
        assert_eq!(back, variant);
    }
}

#[test]
fn tier_rejects_unknown_value() {
    let result: Result<Tier, _> = serde_json::from_str("\"t4\"");
    assert!(
        result.is_err(),
        "tier outside t1|t2|t3 should fail to deserialize"
    );
}

#[test]
fn permission_mode_round_trips_through_all_variants() {
    let cases = [
        (PermissionMode::Default, "\"default\""),
        (PermissionMode::AcceptEdits, "\"acceptEdits\""),
        (PermissionMode::BypassPermissions, "\"bypassPermissions\""),
        (PermissionMode::DontAsk, "\"dontAsk\""),
    ];
    for (variant, wire) in cases {
        let json = serde_json::to_string(&variant).expect("mode should serialize");
        assert_eq!(json, wire);
        let back: PermissionMode = serde_json::from_str(&json).expect("mode should round-trip");
        assert_eq!(back, variant);
    }
}

#[test]
fn permission_mode_rejects_plan() {
    // "plan" is a real Claude Code permission mode elsewhere, but it is
    // deliberately excluded from this closed enum (R7) — rejecting it
    // with the dedicated "plan mode is not dispatchable" message is
    // validation logic for a later task; here it just needs to fail to
    // deserialize like any other unrecognized value.
    let result: Result<PermissionMode, _> = serde_json::from_str("\"plan\"");
    assert!(
        result.is_err(),
        "\"plan\" is outside the closed permission-mode enum and should fail to deserialize"
    );
}

#[test]
fn include_round_trips_through_all_variants() {
    let cases = [
        (Include::Always, "\"always\""),
        (Include::Worktree, "\"worktree\""),
        (Include::Task, "\"task\""),
    ];
    for (variant, wire) in cases {
        let json = serde_json::to_string(&variant).expect("include should serialize");
        assert_eq!(json, wire);
        let back: Include = serde_json::from_str(&json).expect("include should round-trip");
        assert_eq!(back, variant);
    }
}

#[test]
fn include_rejects_unknown_value() {
    let result: Result<Include, _> = serde_json::from_str("\"sometimes\"");
    assert!(
        result.is_err(),
        "include outside always|worktree|task should fail to deserialize"
    );
}

// ============================================================================
// Task 2 — R8 error taxonomy
// ============================================================================

#[test]
fn error_kind_maps_to_r8_exit_code_table() {
    let cases = [
        (ErrorKind::Usage, 2),
        (ErrorKind::RequestInvalid, 3),
        (ErrorKind::ConfigInvalid, 4),
        (ErrorKind::ResolutionFailed, 5),
        (ErrorKind::AssemblyFailed, 6),
        (ErrorKind::IoFailed, 7),
    ];
    for (kind, code) in cases {
        assert_eq!(
            kind.exit_code(),
            code,
            "{kind:?} should map to exit code {code}"
        );
    }
}

#[test]
fn resolution_failed_error_exposes_id_path_and_cause() {
    let err = Error::resolution_failed("rust", "skills/rust.md", "No such file or directory");
    assert_eq!(err.kind, ErrorKind::ResolutionFailed);
    assert_eq!(err.detail("id"), Some("rust"));
    assert_eq!(err.detail("path"), Some("skills/rust.md"));
    assert_eq!(err.detail("cause"), Some("No such file or directory"));
}

#[test]
fn error_kind_display_matches_wire_string() {
    let cases = [
        (ErrorKind::Usage, "usage"),
        (ErrorKind::RequestInvalid, "request_invalid"),
        (ErrorKind::ConfigInvalid, "config_invalid"),
        (ErrorKind::ResolutionFailed, "resolution_failed"),
        (ErrorKind::AssemblyFailed, "assembly_failed"),
        (ErrorKind::IoFailed, "io_failed"),
    ];
    for (kind, wire) in cases {
        assert_eq!(kind.to_string(), wire);
        let json = serde_json::to_string(&kind).expect("kind should serialize");
        assert_eq!(json, format!("\"{wire}\""));
    }
}

#[test]
fn parse_request_maps_serde_error_to_request_invalid() {
    let result = parse_request("not json");
    let err = result.expect_err("malformed JSON should fail to parse");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
}

#[test]
fn parse_registry_maps_toml_error_to_config_invalid() {
    let result = parse_registry("[");
    let err = result.expect_err("malformed TOML should fail to parse");
    assert_eq!(err.kind, ErrorKind::ConfigInvalid);
}

// ============================================================================
// Task 2 — R3 resolver traits (in-memory fakes, no filesystem — AC3.1/AC3.2)
// ============================================================================

/// An in-memory `ContentResolver` fake. Proves the trait's signature
/// takes no concrete filesystem type — a `BTreeMap` lookup satisfies it.
struct FakeResolver {
    files: BTreeMap<&'static str, &'static str>,
}

impl ContentResolver for FakeResolver {
    fn resolve(&self, id: &str, path: &str) -> Result<String, Error> {
        self.files
            .get(path)
            .map(|content| (*content).to_string())
            .ok_or_else(|| Error::resolution_failed(id, path, "not found in fake resolver"))
    }
}

#[test]
fn content_resolver_fake_resolves_known_path() {
    let mut files = BTreeMap::new();
    files.insert("skills/rust.md", "rust skill content");
    let resolver = FakeResolver { files };

    let content = resolver
        .resolve("rust", "skills/rust.md")
        .expect("known path should resolve");
    assert_eq!(content, "rust skill content");
}

#[test]
fn content_resolver_fake_reports_resolution_failed_for_missing_path() {
    let resolver = FakeResolver {
        files: BTreeMap::new(),
    };

    let err = resolver
        .resolve("rust", "skills/rust.md")
        .expect_err("missing path should fail to resolve");
    assert_eq!(err.kind, ErrorKind::ResolutionFailed);
    assert_eq!(err.detail("id"), Some("rust"));
    assert_eq!(err.detail("path"), Some("skills/rust.md"));
}

/// An in-memory `DocumentSink` fake. Proves the trait's signature takes
/// no concrete filesystem type — a `RefCell<Vec<_>>` recorder satisfies
/// it.
struct FakeSink {
    written: RefCell<Vec<(String, String)>>,
}

impl DocumentSink for FakeSink {
    fn write(&self, path: &str, document: &str) -> Result<(), Error> {
        self.written
            .borrow_mut()
            .push((path.to_string(), document.to_string()));
        Ok(())
    }
}

#[test]
fn document_sink_fake_records_write() {
    let sink = FakeSink {
        written: RefCell::new(Vec::new()),
    };
    sink.write("/tmp/out.md", "document body")
        .expect("fake sink write should succeed");
    assert_eq!(
        sink.written.borrow().as_slice(),
        &[("/tmp/out.md".to_string(), "document body".to_string())]
    );
}
