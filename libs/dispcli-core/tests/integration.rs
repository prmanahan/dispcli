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
    AgentEntry, BlockEntry, BlocksSection, ContentResolver, DispatchRequest, DocumentSink,
    Envelope, EnvelopeRequest, Error, ErrorKind, Include, PatternEntry, PermissionMode, Registry,
    RegistryMeta, ScopeOverride, SkillEntry, Tier, assemble_standard, parse_registry,
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

// ============================================================================
// Task 4 — R4 envelope construction + R5 assembly (standard weight)
// ============================================================================

/// A registry matching the R2 example shape closely enough to exercise
/// every `include` kind: `metrics` is `always`, `merge-msg` is
/// `worktree`, `task-tracking` is `task`. The `implementation` pattern's
/// skill order (`verify`, then `rust`) is deliberately the reverse of
/// insertion order in the `skills` map, so an order-preserving bug in
/// [`dispcli_core::assemble_standard`] would show up as a body-order
/// failure, not accidentally pass via map iteration order.
fn sample_registry() -> Registry {
    let mut agents = BTreeMap::new();
    agents.insert(
        "implementer".to_string(),
        AgentEntry {
            profile: "team/implementer.md".to_string(),
            default_mode: PermissionMode::BypassPermissions,
            worktree_required: true,
        },
    );

    let mut skills = BTreeMap::new();
    skills.insert(
        "rust".to_string(),
        SkillEntry {
            path: "skills/rust.md".to_string(),
        },
    );
    skills.insert(
        "verify".to_string(),
        SkillEntry {
            path: "skills/verify.md".to_string(),
        },
    );

    let mut patterns = BTreeMap::new();
    patterns.insert(
        "implementation".to_string(),
        PatternEntry {
            skills: vec!["verify".to_string(), "rust".to_string()],
        },
    );

    let mut blocks = BTreeMap::new();
    blocks.insert(
        "metrics".to_string(),
        BlockEntry {
            path: "skills/dispatch-metrics.md".to_string(),
            include: Include::Always,
        },
    );
    blocks.insert(
        "merge-msg".to_string(),
        BlockEntry {
            path: "skills/dispatch-merge-msg.md".to_string(),
            include: Include::Worktree,
        },
    );
    blocks.insert(
        "task-tracking".to_string(),
        BlockEntry {
            path: "skills/dispatch-task-tracking.md".to_string(),
            include: Include::Task,
        },
    );

    Registry {
        registry: RegistryMeta {
            skills_root: "skills".to_string(),
        },
        agents,
        skills,
        patterns,
        blocks: BlocksSection {
            order: vec![
                "metrics".to_string(),
                "merge-msg".to_string(),
                "task-tracking".to_string(),
            ],
            blocks,
        },
        weights: BTreeMap::new(),
    }
}

/// A minimal well-formed request matching [`sample_registry`]'s
/// `implementer`/`implementation` ids — `worktree` and `task_id` both
/// null (so only the `always` block is included by default; tests that
/// need `worktree`/`task` blocks set those fields explicitly).
fn sample_request() -> DispatchRequest {
    DispatchRequest {
        agent: "implementer".to_string(),
        task_pattern: "implementation".to_string(),
        tier: Tier::T2,
        weight: "standard".to_string(),
        mode_override: None,
        skills_add: Vec::new(),
        skills_remove: Vec::new(),
        task_body: "Do the thing.".to_string(),
        envelope: EnvelopeRequest {
            dispatch_id: 42,
            task_id: None,
            spec_id: None,
            spec_version: None,
            parent_commit: "e2aca810f3f5a11c880beb555bf3ac0be2466e17".to_string(),
            repo: "/abs/path/to/repo".to_string(),
            worktree: None,
            branch: "feature-x".to_string(),
            report_path: None,
            deadline_minutes: None,
            command_scope_subtract: Vec::new(),
            command_scope_add: Vec::new(),
            touch_scope: Vec::new(),
            forbid_scope: Vec::new(),
            verify: Vec::new(),
        },
    }
}

/// A [`FakeResolver`] with content for every path [`sample_registry`]
/// declares (profile, both skills, all three blocks) — the common case
/// where a test doesn't care which block/skill ids get exercised, only
/// their assembled order/filtering/substitution.
fn full_resolver() -> FakeResolver {
    FakeResolver {
        files: BTreeMap::from([
            ("team/implementer.md", "PROFILE CONTENT"),
            ("skills/verify.md", "VERIFY SKILL CONTENT"),
            ("skills/rust.md", "RUST SKILL CONTENT"),
            ("skills/dispatch-metrics.md", "METRICS BLOCK CONTENT"),
            ("skills/dispatch-merge-msg.md", "MERGE MSG BLOCK CONTENT"),
            (
                "skills/dispatch-task-tracking.md",
                "TASK TRACKING BLOCK CONTENT",
            ),
        ]),
    }
}

#[test]
fn envelope_schema_emits_every_key_with_explicit_nulls_and_fixed_order() {
    let mut request = sample_request();
    request.envelope.task_id = Some(7);
    request.envelope.spec_id = Some("docs/specs/0001-envelope-assembly.md".to_string());
    request.envelope.spec_version = None;
    request.envelope.worktree = None;
    request.envelope.deadline_minutes = Some(30);
    request.envelope.report_path = Some("/abs/path/to/repo/scratch/custom-report.md".to_string());

    let envelope = Envelope::from_request(&request);
    let yaml = envelope.to_yaml_string();

    let expected = concat!(
        "---\n",
        "dispatch_id: 42\n",
        "task_id: 7\n",
        "agent_id: implementer\n",
        "spec_id: docs/specs/0001-envelope-assembly.md\n",
        "spec_version: null\n",
        "parent_commit: e2aca810f3f5a11c880beb555bf3ac0be2466e17\n",
        "repo: /abs/path/to/repo\n",
        "worktree: null\n",
        "branch: feature-x\n",
        "report_path: /abs/path/to/repo/scratch/custom-report.md\n",
        "deadline_minutes: 30\n",
        "command_scope_subtract: []\n",
        "command_scope_add: []\n",
        "touch_scope: []\n",
        "forbid_scope: []\n",
        "verify: []\n",
        "---",
    );
    assert_eq!(
        yaml, expected,
        "envelope schema must match R4 byte-for-byte modulo values \
         (bare scalars — matches the reference dispatch-envelope convention)"
    );
}

#[test]
fn envelope_schema_renders_non_empty_scope_and_glob_arrays() {
    let mut request = sample_request();
    request.envelope.worktree = Some("/abs/path/to/worktree".to_string());
    request.envelope.command_scope_subtract = vec![ScopeOverride {
        capability: "push".to_string(),
        reason: "no direct push".to_string(),
    }];
    request.envelope.command_scope_add = vec![ScopeOverride {
        capability: "docker".to_string(),
        reason: "container build needed".to_string(),
    }];
    request.envelope.touch_scope = vec!["libs/dispcli-core/**".to_string()];
    request.envelope.forbid_scope = vec!["Cargo.toml".to_string()];
    request.envelope.verify = vec!["check".to_string()];

    let envelope = Envelope::from_request(&request);
    let yaml = envelope.to_yaml_string();

    let expected = concat!(
        "---\n",
        "dispatch_id: 42\n",
        "task_id: null\n",
        "agent_id: implementer\n",
        "spec_id: null\n",
        "spec_version: null\n",
        "parent_commit: e2aca810f3f5a11c880beb555bf3ac0be2466e17\n",
        "repo: /abs/path/to/repo\n",
        "worktree: /abs/path/to/worktree\n",
        "branch: feature-x\n",
        "report_path: /abs/path/to/worktree/scratch/dispatch-42-report.md\n",
        "deadline_minutes: null\n",
        "command_scope_subtract: [{capability: \"push\", reason: \"no direct push\"}]\n",
        "command_scope_add: [{capability: \"docker\", reason: \"container build needed\"}]\n",
        "touch_scope: [\"libs/dispcli-core/**\"]\n",
        "forbid_scope: [\"Cargo.toml\"]\n",
        "verify: [\"check\"]\n",
        "---",
    );
    assert_eq!(yaml, expected);
}

#[test]
fn report_path_defaults_using_repo_when_no_worktree() {
    let mut request = sample_request();
    request.envelope.dispatch_id = 99;
    request.envelope.worktree = None;
    request.envelope.report_path = None;

    let envelope = Envelope::from_request(&request);
    assert_eq!(
        envelope.report_path,
        "/abs/path/to/repo/scratch/dispatch-99-report.md"
    );
}

#[test]
fn report_path_defaults_using_worktree_when_present() {
    let mut request = sample_request();
    request.envelope.dispatch_id = 99;
    request.envelope.worktree = Some("/abs/path/to/worktree".to_string());
    request.envelope.report_path = None;

    let envelope = Envelope::from_request(&request);
    assert_eq!(
        envelope.report_path,
        "/abs/path/to/worktree/scratch/dispatch-99-report.md"
    );
}

#[test]
fn report_path_uses_explicit_value_when_non_null() {
    let mut request = sample_request();
    request.envelope.worktree = Some("/abs/path/to/worktree".to_string());
    request.envelope.report_path = Some("/abs/custom/report.md".to_string());

    let envelope = Envelope::from_request(&request);
    assert_eq!(envelope.report_path, "/abs/custom/report.md");
}

#[test]
fn envelope_scalar_quoting_is_conditional_on_yaml_ambiguity() {
    // A branch name that looks like a YAML integer must be quoted, or it
    // would parse back as a number, not the string "123".
    let mut numeric_branch = sample_request();
    numeric_branch.envelope.branch = "123".to_string();
    let yaml = Envelope::from_request(&numeric_branch).to_yaml_string();
    assert!(
        yaml.contains("branch: \"123\"\n"),
        "a numeric-looking scalar must be quoted to stay a string"
    );

    // A branch name colliding with a YAML reserved literal must be
    // quoted, or "null" would parse back as the null value.
    let mut reserved_word_branch = sample_request();
    reserved_word_branch.envelope.branch = "null".to_string();
    let yaml = Envelope::from_request(&reserved_word_branch).to_yaml_string();
    assert!(
        yaml.contains("branch: \"null\"\n"),
        "a reserved-literal scalar must be quoted to stay a string"
    );

    // An ordinary identifier-shaped value stays bare — matches the
    // reference dispatch-envelope convention (e.g. `agent_id: Forge`,
    // `branch: dispatch-t4-assembly`).
    let ordinary = sample_request();
    let yaml = Envelope::from_request(&ordinary).to_yaml_string();
    assert!(
        yaml.contains("branch: feature-x\n"),
        "an unambiguous scalar should render bare, not quoted"
    );
}

#[test]
fn assemble_standard_orders_envelope_profile_skills_blocks_task_body() {
    let registry = sample_registry();
    let resolver = full_resolver();
    let request = sample_request(); // worktree=None, task_id=None -> only "always" block included.

    let assembled = assemble_standard(&request, &registry, &resolver)
        .expect("assembly should succeed with fully-resolvable fixtures");

    // AC4.4 — envelope is the first block of the document.
    assert!(assembled.document.starts_with("---\ndispatch_id: 42\n"));

    let envelope_pos = 0usize;
    let profile_pos = assembled
        .document
        .find("PROFILE CONTENT")
        .expect("profile present");
    let verify_pos = assembled
        .document
        .find("VERIFY SKILL CONTENT")
        .expect("verify skill present");
    let rust_pos = assembled
        .document
        .find("RUST SKILL CONTENT")
        .expect("rust skill present");
    let metrics_pos = assembled
        .document
        .find("METRICS BLOCK CONTENT")
        .expect("metrics block present");
    let task_pos = assembled
        .document
        .find("Do the thing.")
        .expect("task body present");

    // AC5.1 — envelope -> profile -> skills (pattern order: verify, rust)
    // -> blocks -> task body.
    assert!(envelope_pos < profile_pos);
    assert!(profile_pos < verify_pos);
    assert!(verify_pos < rust_pos);
    assert!(rust_pos < metrics_pos);
    assert!(metrics_pos < task_pos);

    // Only the `always` block should be present — no worktree, no task.
    assert!(!assembled.document.contains("MERGE MSG BLOCK CONTENT"));
    assert!(!assembled.document.contains("TASK TRACKING BLOCK CONTENT"));
}

#[test]
fn blocks_are_filtered_by_include_always_worktree_task() {
    let registry = sample_registry();
    let resolver = full_resolver();

    // Case 1: no worktree, no task -> only "metrics" (always) included.
    let mut request = sample_request();
    request.envelope.worktree = None;
    request.envelope.task_id = None;
    let assembled = assemble_standard(&request, &registry, &resolver).expect("case 1 assembles");
    assert!(assembled.document.contains("METRICS BLOCK CONTENT"));
    assert!(!assembled.document.contains("MERGE MSG BLOCK CONTENT"));
    assert!(!assembled.document.contains("TASK TRACKING BLOCK CONTENT"));

    // Case 2: worktree present, no task -> metrics + merge-msg.
    let mut request = sample_request();
    request.envelope.worktree = Some("/abs/path/to/worktree".to_string());
    request.envelope.task_id = None;
    let assembled = assemble_standard(&request, &registry, &resolver).expect("case 2 assembles");
    assert!(assembled.document.contains("METRICS BLOCK CONTENT"));
    assert!(assembled.document.contains("MERGE MSG BLOCK CONTENT"));
    assert!(!assembled.document.contains("TASK TRACKING BLOCK CONTENT"));

    // Case 3: task present, no worktree -> metrics + task-tracking.
    let mut request = sample_request();
    request.envelope.worktree = None;
    request.envelope.task_id = Some(7);
    let assembled = assemble_standard(&request, &registry, &resolver).expect("case 3 assembles");
    assert!(assembled.document.contains("METRICS BLOCK CONTENT"));
    assert!(!assembled.document.contains("MERGE MSG BLOCK CONTENT"));
    assert!(assembled.document.contains("TASK TRACKING BLOCK CONTENT"));

    // Case 4: both worktree and task present -> all three blocks.
    let mut request = sample_request();
    request.envelope.worktree = Some("/abs/path/to/worktree".to_string());
    request.envelope.task_id = Some(7);
    let assembled = assemble_standard(&request, &registry, &resolver).expect("case 4 assembles");
    assert!(assembled.document.contains("METRICS BLOCK CONTENT"));
    assert!(assembled.document.contains("MERGE MSG BLOCK CONTENT"));
    assert!(assembled.document.contains("TASK TRACKING BLOCK CONTENT"));
}

#[test]
fn placeholders_substitute_in_skills_and_blocks_but_not_profile_or_task_body() {
    let registry = sample_registry();
    let resolver = FakeResolver {
        files: BTreeMap::from([
            (
                "team/implementer.md",
                "Profile references {dispatch_id} but should stay literal.",
            ),
            (
                "skills/verify.md",
                "Skill for dispatch {dispatch_id} on branch {branch} at {project_path}, \
                 report to {report_path}, worktree {worktree_path}, task {task_id}, \
                 agent {agent_name}.",
            ),
            ("skills/rust.md", "RUST SKILL CONTENT"),
            (
                "skills/dispatch-metrics.md",
                "Block for dispatch {dispatch_id}.",
            ),
            // envelope.worktree/task_id are set below to exercise the
            // {worktree_path}/{task_id} placeholders, which also triggers
            // these two blocks' `include` conditions — both need content.
            ("skills/dispatch-merge-msg.md", "MERGE MSG BLOCK CONTENT"),
            (
                "skills/dispatch-task-tracking.md",
                "TASK TRACKING BLOCK CONTENT",
            ),
        ]),
    };
    let mut request = sample_request();
    request.envelope.dispatch_id = 42;
    request.envelope.task_id = Some(7);
    request.envelope.worktree = Some("/abs/path/to/worktree".to_string());
    request.envelope.branch = "feature-x".to_string();
    request.task_body = "Task body references {dispatch_id} but should stay literal.".to_string();

    let assembled =
        assemble_standard(&request, &registry, &resolver).expect("assembly should succeed");

    // Profile: NOT substituted (R5 — substitution applies to skill/block
    // content only).
    assert!(
        assembled
            .document
            .contains("Profile references {dispatch_id} but should stay literal.")
    );
    // Task body: NOT substituted.
    assert!(
        assembled
            .document
            .contains("Task body references {dispatch_id} but should stay literal.")
    );
    // Skill: every supported placeholder substituted.
    assert!(assembled.document.contains(
        "Skill for dispatch 42 on branch feature-x at /abs/path/to/repo, \
         report to /abs/path/to/worktree/scratch/dispatch-42-report.md, \
         worktree /abs/path/to/worktree, task 7, agent implementer."
    ));
    // Block: substituted too.
    assert!(assembled.document.contains("Block for dispatch 42."));
}

#[test]
fn component_bytes_sum_to_document_length_under_blank_line_join() {
    let registry = sample_registry();
    let resolver = full_resolver();
    let mut request = sample_request();
    request.envelope.worktree = Some("/abs/path/to/worktree".to_string());
    request.envelope.task_id = Some(7);

    let assembled =
        assemble_standard(&request, &registry, &resolver).expect("assembly should succeed");

    // envelope + profile + 2 skills + 3 blocks (all included) + task_body.
    assert_eq!(assembled.components.len(), 8);

    let sum: u64 = assembled.components.iter().map(|c| c.bytes).sum();
    assert_eq!(
        sum,
        assembled.document.len() as u64,
        "AC8.1 — component bytes must sum exactly to the document's byte length"
    );
}

#[test]
fn separator_bytes_are_attributed_to_the_preceding_component() {
    let mut registry = sample_registry();
    registry
        .patterns
        .get_mut("implementation")
        .expect("pattern present in fixture")
        .skills = Vec::new();
    registry.blocks.order = Vec::new();

    let resolver = FakeResolver {
        files: BTreeMap::from([("team/implementer.md", "PROFILE")]),
    };
    let mut request = sample_request();
    request.task_body = "TASK".to_string();

    let assembled =
        assemble_standard(&request, &registry, &resolver).expect("assembly should succeed");

    // Only 3 components remain: envelope, profile, task_body (no skills,
    // no blocks) — proves the accounting per-component, not just in sum.
    assert_eq!(assembled.components.len(), 3);

    let envelope_yaml = Envelope::from_request(&request).to_yaml_string();
    // Gap-4 (resolved 2026-07-15) — the pinned inter-section separator.
    const SEP: &str = "\n\n";

    assert_eq!(
        assembled.components[0].bytes,
        (envelope_yaml.len() + SEP.len()) as u64,
        "envelope (not last) carries its own bytes plus the following separator"
    );
    assert_eq!(
        assembled.components[1].bytes,
        ("PROFILE".len() + SEP.len()) as u64,
        "profile (not last) carries its own bytes plus the following separator"
    );
    assert_eq!(
        assembled.components[2].bytes,
        "TASK".len() as u64,
        "task_body (last component) carries no trailing separator"
    );

    let sum: u64 = assembled.components.iter().map(|c| c.bytes).sum();
    assert_eq!(sum, assembled.document.len() as u64);
}

#[test]
fn assemble_standard_reports_unknown_agent_as_request_invalid() {
    let registry = sample_registry();
    let resolver = full_resolver();
    let mut request = sample_request();
    request.agent = "ghost".to_string();

    let err = assemble_standard(&request, &registry, &resolver)
        .expect_err("unknown agent should fail assembly");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
}

#[test]
fn assemble_standard_propagates_resolution_failure_from_resolver() {
    let registry = sample_registry();
    let resolver = FakeResolver {
        files: BTreeMap::new(),
    };
    let request = sample_request();

    let err = assemble_standard(&request, &registry, &resolver)
        .expect_err("missing profile content should fail assembly");
    assert_eq!(err.kind, ErrorKind::ResolutionFailed);
}

// ============================================================================
// Warden review round (dispatch 1595) — section naming coverage + untested
// error branches (AC1.1/AC2.2) + control-char scalar quoting.
// ============================================================================

#[test]
fn assembled_components_pin_exact_section_naming_and_order() {
    let registry = sample_registry();
    let resolver = full_resolver();
    let request = sample_request(); // worktree=None, task_id=None -> only "metrics" block included.

    let assembled = assemble_standard(&request, &registry, &resolver)
        .expect("assembly should succeed with fully-resolvable fixtures");

    let sections: Vec<&str> = assembled
        .components
        .iter()
        .map(|c| c.section.as_str())
        .collect();
    assert_eq!(
        sections,
        vec![
            "envelope",
            "profile:implementer",
            "skill:verify",
            "skill:rust",
            "block:metrics",
            "task_body",
        ],
        "size.components[].section naming convention (R8) must pin exact \
         kind strings in assembly order — a rename or reformat here would \
         pass every other test silently"
    );
}

#[test]
fn assemble_standard_reports_unknown_task_pattern_as_request_invalid() {
    let registry = sample_registry();
    let resolver = full_resolver();
    let mut request = sample_request();
    request.task_pattern = "ghost-pattern".to_string();

    let err = assemble_standard(&request, &registry, &resolver)
        .expect_err("unknown task_pattern should fail assembly");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("task_pattern"));
    assert_eq!(
        err.detail("value"),
        Some("ghost-pattern"),
        "error must name the offending task_pattern id"
    );
}

#[test]
fn assemble_standard_reports_dangling_pattern_skill_as_config_invalid() {
    let mut registry = sample_registry();
    registry
        .patterns
        .get_mut("implementation")
        .expect("pattern present in fixture")
        .skills = vec!["ghost-skill".to_string()];
    let resolver = full_resolver();
    let request = sample_request();

    let err = assemble_standard(&request, &registry, &resolver)
        .expect_err("pattern referencing an unknown skill id should fail assembly");
    assert_eq!(err.kind, ErrorKind::ConfigInvalid);
    assert_eq!(err.detail("pattern"), Some("implementation"));
    assert_eq!(
        err.detail("skill"),
        Some("ghost-skill"),
        "error must name the offending dangling skill id"
    );
}

#[test]
fn assemble_standard_reports_dangling_block_order_entry_as_config_invalid() {
    let mut registry = sample_registry();
    registry.blocks.order = vec!["ghost-block".to_string()];
    let resolver = full_resolver();
    let request = sample_request();

    let err = assemble_standard(&request, &registry, &resolver)
        .expect_err("blocks.order referencing an unknown block id should fail assembly");
    assert_eq!(err.kind, ErrorKind::ConfigInvalid);
    assert_eq!(
        err.detail("block"),
        Some("ghost-block"),
        "error must name the offending dangling block id"
    );
}

#[test]
fn envelope_scalar_with_embedded_newline_renders_quoted_and_preserves_line_structure() {
    let ordinary = sample_request();
    let ordinary_lines = Envelope::from_request(&ordinary)
        .to_yaml_string()
        .lines()
        .count();

    let mut with_newline = sample_request();
    with_newline.envelope.branch = "line-one\nline-two".to_string();
    let yaml = Envelope::from_request(&with_newline).to_yaml_string();

    // The embedded newline must be escaped inside a double-quoted scalar
    // (a literal backslash-n), never emitted as a bare newline byte — a
    // bare newline would split the field across two lines and corrupt
    // the envelope's fixed field order (AC4.3).
    assert!(
        yaml.contains("branch: \"line-one\\nline-two\"\n"),
        "an embedded control character must force double-quoting with the \
         newline escaped, not literal: {yaml:?}"
    );

    // Line count must match the ordinary (no-control-char) case — a bare
    // embedded newline would add a line and shift every field after it.
    assert_eq!(
        yaml.lines().count(),
        ordinary_lines,
        "an embedded control character rendered bare would insert an \
         extra line, corrupting the envelope's fixed field-order schema"
    );
}
