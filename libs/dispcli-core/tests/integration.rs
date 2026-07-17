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
    AgentEntry, AllOrList, BlockEntry, BlocksSection, ContentResolver, DispatchRequest,
    DocumentSink, Envelope, EnvelopeRequest, Error, ErrorKind, Include, PatternEntry,
    PermissionMode, Registry, RegistryMeta, ScopeOverride, SkillEntry, Tier, WeightClass, assemble,
    assemble_standard, parse_registry, parse_request, parse_verify_entry, scope_overlap_warnings,
    unsupported_brace_tokens, validate_registry, validate_request,
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

    // Task 6 — `validate_request`'s unknown-weight check (AC1.1) needs
    // "standard" declared, since `sample_request()` defaults to it; the
    // weight-class *content* is irrelevant to every other test using this
    // fixture (Task 4's `assemble_standard` never consults `weights`).
    let mut weights = BTreeMap::new();
    weights.insert(
        "standard".to_string(),
        WeightClass {
            profile_sections: AllOrList::All("all".to_string()),
            skills: None,
            blocks: AllOrList::All("all".to_string()),
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
        weights,
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

#[test]
fn assemble_standard_resolution_failure_names_the_registry_key_and_declared_path() {
    // R2/AC2.1: "a registry referencing a file that cannot be resolved
    // fails at assembly time with an error naming the registry key and
    // the path." Removes only the "rust" skill's file from an otherwise-
    // complete resolver so the failure is unambiguous about which
    // registry key/path it names — a bare `ErrorKind` check (as in
    // `assemble_standard_propagates_resolution_failure_from_resolver`
    // above) wouldn't distinguish "failed" from "failed naming the right
    // thing".
    let registry = sample_registry();
    let mut files = full_resolver().files;
    files.remove("skills/rust.md");
    let resolver = FakeResolver { files };
    let request = sample_request();

    let err = assemble_standard(&request, &registry, &resolver)
        .expect_err("missing skill content should fail assembly");
    assert_eq!(err.kind, ErrorKind::ResolutionFailed);
    assert_eq!(err.detail("id"), Some("rust"));
    assert_eq!(err.detail("path"), Some("skills/rust.md"));
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

// ============================================================================
// Task 6 — R1 AC1.1 + R7 request-side validation (`validate_request`,
// `parse_verify_entry`, `scope_overlap_warnings`, plan-mode enrichment on
// `parse_request`/`parse_registry`).
// ============================================================================

// ---- R1 AC1.1 — unknown agent / task_pattern / weight / skill id --------

#[test]
fn validate_request_accepts_known_agent_pattern_weight_and_skills() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.skills_add = vec!["rust".to_string()];
    request.skills_remove = vec!["verify".to_string()];
    validate_request(&request, &registry).expect("known ids should validate");
}

#[test]
fn validate_request_rejects_unknown_agent_with_field_and_value() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.agent = "ghost".to_string();
    let err = validate_request(&request, &registry).expect_err("unknown agent should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("agent"));
    assert_eq!(err.detail("value"), Some("ghost"));
}

#[test]
fn validate_request_rejects_unknown_task_pattern_with_field_and_value() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.task_pattern = "ghost-pattern".to_string();
    let err =
        validate_request(&request, &registry).expect_err("unknown task_pattern should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("task_pattern"));
    assert_eq!(err.detail("value"), Some("ghost-pattern"));
}

#[test]
fn validate_request_rejects_unknown_weight_with_field_and_value() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.weight = "ghost-weight".to_string();
    let err = validate_request(&request, &registry).expect_err("unknown weight should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("weight"));
    assert_eq!(err.detail("value"), Some("ghost-weight"));
}

#[test]
fn validate_request_rejects_unknown_skill_ids_in_skills_add_and_remove_together() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.skills_add = vec!["ghost-add".to_string()];
    request.skills_remove = vec!["ghost-remove".to_string()];
    let err =
        validate_request(&request, &registry).expect_err("unknown skill ids should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(
        err.all_details("field"),
        vec!["skills_add[0]", "skills_remove[0]"],
        "both unknown skill ids should be reported together (collect-all-in-class)"
    );
    assert_eq!(err.all_details("value"), vec!["ghost-add", "ghost-remove"]);
}

#[test]
fn validate_request_reports_only_the_first_failing_class_when_multiple_classes_are_invalid() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.agent = "ghost".to_string(); // unknown-id class — checked first.
    request.envelope.parent_commit = "too-short".to_string(); // SHA class — checked second.
    let err =
        validate_request(&request, &registry).expect_err("request should fail on unknown agent");
    assert_eq!(
        err.detail("field"),
        Some("agent"),
        "the unknown-id class is checked before the SHA class; only its \
         violation should surface in this call"
    );
}

// ---- R7 — parent_commit / spec_version: 40-char lower-hex ---------------

const VALID_SHA: &str = "e2aca810f3f5a11c880beb555bf3ac0be2466e17";

#[test]
fn validate_request_accepts_40_char_lowercase_sha_in_parent_commit_and_spec_version() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.envelope.spec_version = Some(VALID_SHA.to_string());
    validate_request(&request, &registry).expect("40-char lowercase hex should validate");
}

#[test]
fn validate_request_rejects_39_char_parent_commit() {
    let registry = sample_registry();
    let mut request = sample_request();
    let short = VALID_SHA
        .get(..39)
        .expect("VALID_SHA has at least 39 bytes");
    request.envelope.parent_commit = short.to_string();
    let err = validate_request(&request, &registry).expect_err("39-char SHA should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("envelope.parent_commit"));
    assert_eq!(err.detail("value"), Some(short));
}

#[test]
fn validate_request_rejects_41_char_parent_commit() {
    let registry = sample_registry();
    let mut request = sample_request();
    let long = format!("{VALID_SHA}0");
    request.envelope.parent_commit = long.clone();
    let err = validate_request(&request, &registry).expect_err("41-char SHA should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("envelope.parent_commit"));
    assert_eq!(err.detail("value"), Some(long.as_str()));
}

#[test]
fn validate_request_rejects_uppercase_hex_sha() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.envelope.parent_commit = VALID_SHA.to_ascii_uppercase();
    let err =
        validate_request(&request, &registry).expect_err("uppercase hex SHA should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("envelope.parent_commit"));
}

#[test]
fn validate_request_rejects_bad_spec_version_alongside_valid_parent_commit() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.envelope.spec_version = Some("short".to_string());
    let err =
        validate_request(&request, &registry).expect_err("bad spec_version should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("envelope.spec_version"));
    assert_eq!(err.detail("value"), Some("short"));
}

// ---- R7 — repo / worktree / report_path: absolute when non-null ---------

#[test]
fn validate_request_accepts_absolute_repo_worktree_and_report_path() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.envelope.worktree = Some("/abs/path/to/worktree".to_string());
    request.envelope.report_path = Some("/abs/path/to/report.md".to_string());
    validate_request(&request, &registry).expect("absolute paths should validate");
}

#[test]
fn validate_request_rejects_relative_repo_path() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.envelope.repo = "relative/path".to_string();
    let err =
        validate_request(&request, &registry).expect_err("relative repo path should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("envelope.repo"));
    assert_eq!(err.detail("value"), Some("relative/path"));
}

#[test]
fn validate_request_rejects_relative_worktree_and_report_path_together() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.envelope.worktree = Some("relative/worktree".to_string());
    request.envelope.report_path = Some("relative/report.md".to_string());
    let err = validate_request(&request, &registry).expect_err("relative paths should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(
        err.all_details("field"),
        vec!["envelope.worktree", "envelope.report_path"],
        "both relative-path violations must be reported together"
    );
    assert_eq!(
        err.all_details("value"),
        vec!["relative/worktree", "relative/report.md"]
    );
}

// ---- R7 — verify entries --------------------------------------------------

#[test]
fn parse_verify_entry_strips_just_prefix_to_same_recipe_as_bare_form() {
    let with_prefix = parse_verify_entry("just check").expect("should parse");
    let bare = parse_verify_entry("check").expect("should parse");
    assert_eq!(with_prefix.recipe, "check");
    assert_eq!(bare.recipe, "check");
    assert!(with_prefix.args.is_empty());
    assert!(bare.args.is_empty());
}

#[test]
fn parse_verify_entry_accepts_recipe_with_args() {
    let parsed = parse_verify_entry("check --release").expect("should parse");
    assert_eq!(parsed.recipe, "check");
    assert_eq!(parsed.args, vec!["--release".to_string()]);
}

#[test]
fn parse_verify_entry_rejects_empty_after_trim() {
    let err = parse_verify_entry("   ").expect_err("blank entry should be rejected");
    assert!(err.contains("empty"), "error should say 'empty': {err}");

    let err_empty = parse_verify_entry("").expect_err("empty string should be rejected");
    assert!(
        err_empty.contains("empty"),
        "error should say 'empty': {err_empty}"
    );
}

#[test]
fn parse_verify_entry_treats_bare_just_with_no_trailing_content_as_a_literal_recipe_name() {
    // Rule order is "trim; THEN strip a leading `just ` token" — trimming
    // "just " (trailing space, nothing after) already consumes the space
    // the prefix match needs, so the literal 4-char token "just" no
    // longer matches the 5-char prefix "just " and is treated as a
    // (nonsensical but well-formed) recipe name in its own right. This is
    // the deliberate consequence of applying the R7 rule steps in the
    // order given, not a special case carved out for it.
    let parsed = parse_verify_entry("just ").expect("bare 'just' should parse as a recipe name");
    assert_eq!(parsed.recipe, "just");
    assert!(parsed.args.is_empty());
}

#[test]
fn parse_verify_entry_rejects_shell_metacharacters() {
    let err =
        parse_verify_entry("check; rm -rf /").expect_err("metacharacter entry should be rejected");
    assert!(
        err.contains("metacharacter"),
        "error should name the metacharacter rejection: {err}"
    );
}

#[test]
fn parse_verify_entry_rejects_every_shell_metacharacter() {
    // Regression lock for `VERIFY_SHELL_METACHARACTERS` (11 entries,
    // dispcli_core.rs). `parse_verify_entry_rejects_shell_metacharacters`
    // above only exercises `;` and `&` — this table drives all 11
    // independently, so silently dropping any single char from the
    // denylist array makes this test fail. Table literal, not a re-import
    // of the private const: integration tests only see the public
    // surface, so this list is the regression oracle, not a mirror.
    let metacharacters = ['&', '|', ';', '>', '<', '`', '$', '(', ')', '\n', '\r'];
    assert_eq!(
        metacharacters.len(),
        11,
        "this table must track all 11 VERIFY_SHELL_METACHARACTERS entries"
    );
    for c in metacharacters {
        // Embed the char mid-string (not at either end) so `trim()` never
        // strips it before the metacharacter scan runs — load-bearing for
        // '\n'/'\r', which `trim()` would otherwise remove at a boundary.
        let entry = format!("check{c}rm -rf /");
        let err = match parse_verify_entry(&entry) {
            Err(e) => e,
            Ok(parsed) => panic!("entry containing {c:?} should be rejected, got {parsed:?}"),
        };
        assert!(
            err.contains("metacharacter"),
            "error for {c:?} should name the metacharacter rejection: {err}"
        );
    }
}

#[test]
fn validate_request_accepts_verify_entries_just_check_and_bare_check() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.envelope.verify = vec!["just check".to_string(), "check".to_string()];
    validate_request(&request, &registry).expect("'just check' and 'check' should both validate");
}

#[test]
fn validate_request_collects_all_bad_verify_entries() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.envelope.verify = vec![
        "check".to_string(),            // good
        "check; rm -rf /".to_string(),  // bad — metacharacter
        "   ".to_string(),              // bad — empty after trim
        "check && echo hi".to_string(), // bad — metacharacter
    ];
    let err = validate_request(&request, &registry)
        .expect_err("a request with three bad verify entries should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(
        err.all_details("field"),
        vec![
            "envelope.verify[1]",
            "envelope.verify[2]",
            "envelope.verify[3]"
        ],
        "all three bad verify entries must be reported together, not just the first"
    );
    assert_eq!(
        err.all_details("value"),
        vec!["check; rm -rf /", "   ", "check && echo hi"]
    );
}

// ---- R7 — command_scope_subtract / command_scope_add ---------------------

#[test]
fn validate_request_accepts_scope_override_with_capability_and_reason() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.envelope.command_scope_subtract = vec![ScopeOverride {
        capability: "push".to_string(),
        reason: "no direct push".to_string(),
    }];
    validate_request(&request, &registry).expect("non-empty capability + reason should validate");
}

#[test]
fn validate_request_rejects_scope_override_missing_reason() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.envelope.command_scope_subtract = vec![ScopeOverride {
        capability: "push".to_string(),
        reason: String::new(),
    }];
    let err = validate_request(&request, &registry).expect_err("empty reason should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(
        err.detail("field"),
        Some("envelope.command_scope_subtract[0].reason")
    );
    assert_eq!(err.detail("value"), Some(""));
}

#[test]
fn validate_request_rejects_scope_override_missing_capability() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.envelope.command_scope_add = vec![ScopeOverride {
        capability: "   ".to_string(),
        reason: "container build needed".to_string(),
    }];
    let err =
        validate_request(&request, &registry).expect_err("blank capability should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(
        err.detail("field"),
        Some("envelope.command_scope_add[0].capability")
    );
}

// ---- R7 — scope globs + AC7.3 trailing-slash normalization ---------------

#[test]
fn validate_request_accepts_compilable_glob_patterns() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.envelope.touch_scope = vec!["libs/dispcli-core/**".to_string()];
    request.envelope.forbid_scope = vec!["Cargo.toml".to_string()];
    validate_request(&request, &registry).expect("compilable globs should validate");
}

#[test]
fn validate_request_rejects_uncompilable_glob_pattern() {
    let registry = sample_registry();
    let mut request = sample_request();
    // An unclosed character class does not compile as a glob.
    request.envelope.touch_scope = vec!["libs/[a-".to_string()];
    let err = validate_request(&request, &registry).expect_err("malformed glob should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("envelope.touch_scope[0]"));
}

#[test]
fn validate_request_accepts_normalized_form_of_trailing_slash_glob() {
    let registry = sample_registry();
    let mut request = sample_request();
    request.envelope.touch_scope = vec!["libs/dispcli-core/".to_string()];
    assert!(
        validate_request(&request, &registry).is_ok(),
        "a trailing-slash entry must validate against its normalized form, not reject it"
    );
}

#[test]
fn trailing_slash_scope_entry_normalizes_to_double_star_in_emitted_envelope() {
    let mut request = sample_request();
    request.envelope.touch_scope = vec!["libs/dispcli-core/".to_string()];
    request.envelope.forbid_scope = vec!["docs/".to_string()];

    let yaml = Envelope::from_request(&request).to_yaml_string();

    assert!(
        yaml.contains("touch_scope: [\"libs/dispcli-core/**\"]"),
        "trailing-slash entry must normalize to path/** and be observable \
         in the emitted envelope (AC7.3): {yaml}"
    );
    assert!(
        yaml.contains("forbid_scope: [\"docs/**\"]"),
        "trailing-slash entry must normalize in forbid_scope too: {yaml}"
    );
}

// ---- Gap-3 — scope-overlap warning (tractable reading only) --------------

#[test]
fn scope_overlap_warns_on_identical_normalized_duplicate() {
    let touch = vec!["Cargo.toml".to_string(), "libs/dispcli-core/".to_string()];
    let forbid = vec!["libs/dispcli-core/**".to_string()];
    let warnings = scope_overlap_warnings(&touch, &forbid);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("libs/dispcli-core/**"));
}

#[test]
fn scope_overlap_does_not_warn_on_differing_patterns_that_would_glob_intersect() {
    // Tractable reading only (Gap-3, v0): `libs/**` and `libs/foo.rs` overlap
    // under true glob-intersection semantics, but are not identical
    // normalized strings — general glob-intersection is deferred to v1+,
    // and this function must not attempt it.
    let touch = vec!["libs/**".to_string()];
    let forbid = vec!["libs/foo.rs".to_string()];
    assert!(
        scope_overlap_warnings(&touch, &forbid).is_empty(),
        "tractable reading must not attempt glob-intersection matching"
    );
}

#[test]
fn scope_overlap_returns_no_warnings_when_arrays_are_disjoint() {
    let touch = vec!["libs/dispcli-core/**".to_string()];
    let forbid = vec!["Cargo.toml".to_string()];
    assert!(scope_overlap_warnings(&touch, &forbid).is_empty());
}

// ---- R7 — mode values (closed enum; "plan" rejected with dedicated msg) --

#[test]
fn parse_request_rejects_plan_mode_with_dedicated_message_and_field_detail() {
    let request_json = format!(
        r#"{{
            "agent": "implementer",
            "task_pattern": "implementation",
            "tier": "t2",
            "mode_override": "plan",
            "task_body": "Do the thing.",
            "envelope": {{
                "dispatch_id": 1,
                "parent_commit": "{VALID_SHA}",
                "repo": "/abs/path/to/repo",
                "branch": "feature-x"
            }}
        }}"#
    );
    let err = parse_request(&request_json).expect_err("plan mode should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert!(
        err.message.contains("not dispatchable"),
        "message must state plan mode is not dispatchable: {}",
        err.message
    );
    assert_eq!(err.detail("field"), Some("mode_override"));
    assert_eq!(err.detail("value"), Some("plan"));
}

#[test]
fn parse_request_accepts_every_closed_permission_mode_value() {
    for mode in ["default", "acceptEdits", "bypassPermissions", "dontAsk"] {
        let request_json = format!(
            r#"{{
                "agent": "implementer",
                "task_pattern": "implementation",
                "tier": "t2",
                "mode_override": "{mode}",
                "task_body": "Do the thing.",
                "envelope": {{
                    "dispatch_id": 1,
                    "parent_commit": "{VALID_SHA}",
                    "repo": "/abs/path/to/repo",
                    "branch": "feature-x"
                }}
            }}"#
        );
        let request = parse_request(&request_json)
            .unwrap_or_else(|e| panic!("mode '{mode}' should parse: {e}"));
        assert!(request.mode_override.is_some());
    }
}

#[test]
fn parse_registry_rejects_plan_default_mode_naming_the_offending_agent() {
    let registry_toml = r#"
[registry]
skills_root = "skills"

[agents.implementer]
profile = "team/implementer.md"
default_mode = "plan"
worktree_required = true

[blocks]
order = []
"#;
    let err = parse_registry(registry_toml).expect_err("plan default_mode should be rejected");
    assert_eq!(err.kind, ErrorKind::ConfigInvalid);
    assert!(
        err.message.contains("not dispatchable"),
        "message must state plan mode is not dispatchable: {}",
        err.message
    );
    assert_eq!(err.detail("field"), Some("agents.implementer.default_mode"));
    assert_eq!(err.detail("value"), Some("plan"));
}

// ---- R2 AC2.3 — `include` values (closed enum) ---------------------------

#[test]
fn parse_registry_rejects_include_value_outside_closed_enum() {
    // R2/AC2.3: `include` is closed to always|worktree|task. No new
    // runtime check is needed for this in `validate_registry` — `Include`
    // is already a closed 3-variant enum (Task 1), so a value outside the
    // set fails at `parse_registry` time, mapped to `config_invalid` by
    // the standard `toml::de::Error` mapping (`From<toml::de::Error>`) —
    // same "define errors out of existence" treatment `Tier`/
    // `PermissionMode` get elsewhere in this file.
    let registry_toml = r#"
[registry]
skills_root = "skills"

[blocks]
order = ["metrics"]

[blocks.metrics]
path = "skills/dispatch-metrics.md"
include = "sometimes"
"#;
    let err = parse_registry(registry_toml)
        .expect_err("include value outside always|worktree|task should be rejected");
    assert_eq!(err.kind, ErrorKind::ConfigInvalid);
}

// ---- R7 — tier values (closed enum) --------------------------------------

#[test]
fn parse_request_rejects_bogus_tier_value() {
    let request_json = format!(
        r#"{{
            "agent": "implementer",
            "task_pattern": "implementation",
            "tier": "t4",
            "task_body": "Do the thing.",
            "envelope": {{
                "dispatch_id": 1,
                "parent_commit": "{VALID_SHA}",
                "repo": "/abs/path/to/repo",
                "branch": "feature-x"
            }}
        }}"#
    );
    let err = parse_request(&request_json)
        .expect_err("tier 't4' is outside t1|t2|t3 and should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
}

// ---- R1 AC1.1 — missing required field -----------------------------------

#[test]
fn parse_request_rejects_missing_required_field_naming_it_in_the_message() {
    // "agent" is omitted entirely.
    let request_json = format!(
        r#"{{
            "task_pattern": "implementation",
            "tier": "t2",
            "task_body": "Do the thing.",
            "envelope": {{
                "dispatch_id": 1,
                "parent_commit": "{VALID_SHA}",
                "repo": "/abs/path/to/repo",
                "branch": "feature-x"
            }}
        }}"#
    );
    let err = parse_request(&request_json).expect_err("missing required field should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert!(
        err.message.contains("agent"),
        "the missing field's name should appear in the error message: {}",
        err.message
    );
}

// ============================================================================
// Task 7 — R2 AC2.2 registry self-consistency validation
// (`validate_registry`). AC2.1 (unresolvable paths) is exercised at the
// dispcli-io resolver level (see that crate's integration tests) plus
// `assemble_standard_resolution_failure_names_the_registry_key_and_declared_path`
// above; AC2.3 (closed `include` enum) is exercised via `parse_registry`
// directly — see `parse_registry_rejects_include_value_outside_closed_enum`
// above.
// ============================================================================

#[test]
fn validate_registry_accepts_self_consistent_registry() {
    let registry = sample_registry();
    validate_registry(&registry).expect("sample_registry's own cross-references should validate");
}

#[test]
fn validate_registry_rejects_dangling_pattern_skill_with_field_and_value() {
    let mut registry = sample_registry();
    registry
        .patterns
        .get_mut("implementation")
        .expect("pattern present in fixture")
        .skills = vec!["verify".to_string(), "ghost-skill".to_string()];

    let err = validate_registry(&registry)
        .expect_err("pattern referencing an undeclared skill id should be rejected");
    assert_eq!(err.kind, ErrorKind::ConfigInvalid);
    assert_eq!(
        err.detail("field"),
        Some("patterns.implementation.skills[1]")
    );
    assert_eq!(err.detail("value"), Some("ghost-skill"));
}

#[test]
fn validate_registry_rejects_dangling_blocks_order_entry_with_field_and_value() {
    let mut registry = sample_registry();
    registry.blocks.order = vec!["metrics".to_string(), "ghost-block".to_string()];

    let err = validate_registry(&registry)
        .expect_err("blocks.order referencing an undeclared block id should be rejected");
    assert_eq!(err.kind, ErrorKind::ConfigInvalid);
    assert_eq!(err.detail("field"), Some("blocks.order[1]"));
    assert_eq!(err.detail("value"), Some("ghost-block"));
}

#[test]
fn validate_registry_rejects_dangling_weight_skill_with_field_and_value() {
    let mut registry = sample_registry();
    registry.weights.insert(
        "light".to_string(),
        WeightClass {
            profile_sections: AllOrList::All("all".to_string()),
            skills: Some(vec!["ghost-skill".to_string()]),
            blocks: AllOrList::All("all".to_string()),
        },
    );

    let err = validate_registry(&registry)
        .expect_err("weight class referencing an undeclared skill id should be rejected");
    assert_eq!(err.kind, ErrorKind::ConfigInvalid);
    assert_eq!(err.detail("field"), Some("weights.light.skills[0]"));
    assert_eq!(err.detail("value"), Some("ghost-skill"));
}

#[test]
fn validate_registry_rejects_dangling_weight_block_list_entry_with_field_and_value() {
    let mut registry = sample_registry();
    registry.weights.insert(
        "light".to_string(),
        WeightClass {
            profile_sections: AllOrList::All("all".to_string()),
            skills: None,
            blocks: AllOrList::List(vec!["metrics".to_string(), "ghost-block".to_string()]),
        },
    );

    let err = validate_registry(&registry).expect_err(
        "weight class block list referencing an undeclared block id should be rejected",
    );
    assert_eq!(err.kind, ErrorKind::ConfigInvalid);
    assert_eq!(err.detail("field"), Some("weights.light.blocks[1]"));
    assert_eq!(err.detail("value"), Some("ghost-block"));
}

#[test]
fn validate_registry_accepts_weight_class_all_sentinel_without_treating_it_as_a_dangling_id() {
    // Trap 1 (spec "open vs. closed vocabularies" note): the `"all"`
    // sentinel is a closed-vocabulary VALUE, not an id to resolve. A
    // naive implementation that iterated `AllOrList::All(s)` as if it
    // held a one-element id list would report "all" as a dangling block
    // id (no `[blocks.all]` table exists) — this pins that it must not.
    let registry_toml = r#"
[registry]
skills_root = "skills"

[blocks]
order = []

[weights.standard]
profile_sections = "all"
blocks = "all"
"#;
    let registry = parse_registry(registry_toml).expect("well-formed registry should parse");
    validate_registry(&registry)
        .expect("the \"all\" sentinel must never be checked as a dangling block id");
}

#[test]
fn validate_registry_accepts_weight_class_with_valid_explicit_skill_and_block_lists() {
    // Boundary-acceptance counterpart to the two dangling-weight-list
    // rejection tests above: a weight class using the `AllOrList::List`
    // form (not the sentinel) with every entry declared should validate
    // cleanly.
    let mut registry = sample_registry();
    registry.weights.insert(
        "light".to_string(),
        WeightClass {
            profile_sections: AllOrList::List(vec!["role".to_string()]),
            skills: Some(vec!["verify".to_string()]),
            blocks: AllOrList::List(vec!["metrics".to_string()]),
        },
    );

    validate_registry(&registry)
        .expect("weight class with fully-declared explicit skill/block lists should validate");
}

#[test]
fn validate_registry_collects_dangling_references_across_patterns_blocks_and_weights_together() {
    let mut registry = sample_registry();
    registry
        .patterns
        .get_mut("implementation")
        .expect("pattern present in fixture")
        .skills = vec!["ghost-pattern-skill".to_string()];
    registry.blocks.order = vec!["ghost-block".to_string()];
    registry.weights.insert(
        "light".to_string(),
        WeightClass {
            profile_sections: AllOrList::All("all".to_string()),
            skills: Some(vec!["ghost-weight-skill".to_string()]),
            blocks: AllOrList::List(vec!["ghost-weight-block".to_string()]),
        },
    );

    let err = validate_registry(&registry)
        .expect_err("dangling refs across all four sources should be rejected together");
    assert_eq!(err.kind, ErrorKind::ConfigInvalid);
    assert_eq!(
        err.all_details("field"),
        vec![
            "patterns.implementation.skills[0]",
            "blocks.order[0]",
            "weights.light.skills[0]",
            "weights.light.blocks[0]",
        ],
        "one combined class: every dangling reference across patterns, \
         blocks.order, and weights must be collected together, not \
         source-by-source (same treatment validate_request gives its \
         unknown-id class spanning agent/task_pattern/weight/skills_add/\
         skills_remove)"
    );
    assert_eq!(
        err.all_details("value"),
        vec![
            "ghost-pattern-skill",
            "ghost-block",
            "ghost-weight-skill",
            "ghost-weight-block",
        ]
    );
}

// ============================================================================
// Task 9 — R5 placeholder-substitution completeness (AC5.2/AC5.3/AC5.4):
// unresolved-supported-placeholder assembly failures, unsupported-brace-
// token warnings, and the full skill-merge rule (pattern order, then
// skills_add, dedup-keeps-first, minus skills_remove — Gap-2, ruled
// 2026-07-15).
// ============================================================================

// ---- AC5.4 / Gap-2 — skill merge: pattern + skills_add, dedup, remove ---

#[test]
fn resolve_skills_merges_pattern_and_skills_add_with_dedup_keeping_first_occurrence() {
    let mut registry = sample_registry();
    registry.skills.insert(
        "tdd".to_string(),
        SkillEntry {
            path: "skills/tdd.md".to_string(),
        },
    );
    let mut files = full_resolver().files;
    files.insert("skills/tdd.md", "TDD SKILL CONTENT");
    let resolver = FakeResolver { files };

    let mut request = sample_request();
    // "verify" is already in the pattern's skill list — this must not
    // duplicate it or move it to the skills_add position. "tdd" is new.
    request.skills_add = vec!["verify".to_string(), "tdd".to_string()];

    let assembled = assemble_standard(&request, &registry, &resolver)
        .expect("skills_add merge should assemble cleanly");

    let skill_sections: Vec<&str> = assembled
        .components
        .iter()
        .map(|c| c.section.as_str())
        .filter(|s| s.starts_with("skill:"))
        .collect();
    assert_eq!(
        skill_sections,
        vec!["skill:verify", "skill:rust", "skill:tdd"],
        "AC5.4/Gap-2: pattern order first, then skills_add in array order, \
         with the duplicate 'verify' kept at its FIRST occurrence (pattern \
         position) — dedup keeps first occurrence, not last"
    );
    assert_eq!(
        assembled.document.matches("VERIFY SKILL CONTENT").count(),
        1,
        "a skill present via both the pattern mapping and skills_add must \
         be included exactly once (AC5.4)"
    );
}

#[test]
fn resolve_skills_applies_skills_remove_to_a_pattern_skill() {
    let registry = sample_registry();
    let resolver = full_resolver();
    let mut request = sample_request();
    request.skills_remove = vec!["rust".to_string()];

    let assembled = assemble_standard(&request, &registry, &resolver)
        .expect("removing a present pattern skill should assemble cleanly");

    let skill_sections: Vec<&str> = assembled
        .components
        .iter()
        .map(|c| c.section.as_str())
        .filter(|s| s.starts_with("skill:"))
        .collect();
    assert_eq!(
        skill_sections,
        vec!["skill:verify"],
        "skills_remove must drop the matching skill from the effective set"
    );
    assert!(!assembled.document.contains("RUST SKILL CONTENT"));
}

#[test]
fn validate_request_rejects_skills_remove_of_skill_absent_from_effective_set() {
    let mut registry = sample_registry();
    // "tdd" is a real registry skill (passes the AC1.1 unknown-id check)
    // but is not part of the "implementation" pattern nor skills_add —
    // absent from the effective set.
    registry.skills.insert(
        "tdd".to_string(),
        SkillEntry {
            path: "skills/tdd.md".to_string(),
        },
    );
    let mut request = sample_request();
    request.skills_remove = vec!["tdd".to_string()];

    let err = validate_request(&request, &registry)
        .expect_err("skills_remove of a skill absent from the effective set should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("skills_remove[0]"));
    assert_eq!(err.detail("value"), Some("tdd"));
}

#[test]
fn assemble_standard_rejects_skills_remove_of_skill_absent_from_effective_set() {
    // Defense-in-depth counterpart to the validate_request test above —
    // assemble_standard must independently enforce AC5.4 when called
    // without validate_request having run first (same precedent as the
    // existing unknown-agent/unknown-task_pattern defense-in-depth
    // checks).
    let mut registry = sample_registry();
    registry.skills.insert(
        "tdd".to_string(),
        SkillEntry {
            path: "skills/tdd.md".to_string(),
        },
    );
    let resolver = full_resolver();
    let mut request = sample_request();
    request.skills_remove = vec!["tdd".to_string()];

    let err = assemble_standard(&request, &registry, &resolver)
        .expect_err("assembly must independently reject skills_remove of an absent skill (AC5.4)");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("skills_remove[0]"));
    assert_eq!(err.detail("value"), Some("tdd"));
}

#[test]
fn validate_request_reports_unknown_registry_id_before_absent_from_effective_set_class() {
    let registry = sample_registry();
    let mut request = sample_request();
    // "ghost" is not a registered skill at all -> the unknown-id class
    // (checked first) must fire, not the absent-from-effective-set class.
    request.skills_remove = vec!["ghost".to_string()];
    let err = validate_request(&request, &registry)
        .expect_err("skills_remove of a wholly unregistered skill should be rejected");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("skills_remove[0]"));
    assert_eq!(err.detail("value"), Some("ghost"));
    assert!(
        err.message.contains("unknown registry ids"),
        "the unknown-id class must fire before the absent-from-effective-set \
         class for a skill id that isn't even registered: {}",
        err.message
    );
}

#[test]
fn assemble_standard_reports_dangling_skills_add_entry_as_request_invalid() {
    // AC1.1: an unknown skill id in skills_add is a request-side problem
    // (the caller named a skill that doesn't exist) — distinct from the
    // pattern-sourced dangling-reference case (config_invalid, AC2.2)
    // covered by `assemble_standard_reports_dangling_pattern_skill_as_config_invalid`
    // above. Only reachable when assemble_standard runs without
    // validate_request having caught it first as an AC1.1 unknown id.
    let registry = sample_registry();
    let resolver = full_resolver();
    let mut request = sample_request();
    request.skills_add = vec!["ghost-add-skill".to_string()];

    let err = assemble_standard(&request, &registry, &resolver)
        .expect_err("skills_add referencing an undeclared skill should fail assembly");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("skills_add[0]"));
    assert_eq!(err.detail("value"), Some("ghost-add-skill"));
}

// ---- AC5.2 — unresolved supported placeholder is an assembly error ------

#[test]
fn assemble_standard_reports_unresolved_worktree_path_placeholder_as_assembly_failed() {
    // AC5.2's own example: {worktree_path} used by an included block in a
    // non-worktree dispatch. "metrics" is include=always, so it's present
    // regardless of worktree — the request's worktree stays null.
    let registry = sample_registry();
    let resolver = FakeResolver {
        files: BTreeMap::from([
            ("team/implementer.md", "PROFILE CONTENT"),
            ("skills/verify.md", "VERIFY SKILL CONTENT"),
            ("skills/rust.md", "RUST SKILL CONTENT"),
            (
                "skills/dispatch-metrics.md",
                "Run commands from {worktree_path}.",
            ),
        ]),
    };
    let mut request = sample_request();
    request.envelope.worktree = None;

    let err = assemble_standard(&request, &registry, &resolver).expect_err(
        "an always-included block referencing {worktree_path} in a non-worktree \
         dispatch must fail assembly, not silently emit the literal token",
    );
    assert_eq!(err.kind, ErrorKind::AssemblyFailed);
    assert_eq!(err.detail("section"), Some("block:metrics"));
    assert_eq!(err.detail("placeholder"), Some("{worktree_path}"));
}

#[test]
fn unresolved_task_id_placeholder_in_always_included_skill_is_intentional_assembly_failure() {
    // Intentional-gap note (spec 0001 Task 9 dispatch): {task_id}/
    // {worktree_path} substitute only when non-null and are safe ONLY
    // inside include=task/include=worktree blocks (filtered out when
    // null). An ALWAYS-included skill referencing {task_id} in a
    // no-task dispatch is a genuine, INTENDED assembly error under
    // AC5.2 — a registry-authoring mistake the spec wants surfaced
    // loudly, not a bug in this test or in assemble_standard.
    let registry = sample_registry();
    let resolver = FakeResolver {
        files: BTreeMap::from([
            ("team/implementer.md", "PROFILE CONTENT"),
            ("skills/verify.md", "Track progress against task {task_id}."),
            ("skills/rust.md", "RUST SKILL CONTENT"),
            ("skills/dispatch-metrics.md", "METRICS BLOCK CONTENT"),
        ]),
    };
    let mut request = sample_request();
    request.envelope.task_id = None;

    let err = assemble_standard(&request, &registry, &resolver).expect_err(
        "an always-included skill referencing {task_id} with no task_id must fail \
         assembly under AC5.2 — this is the intended behavior, not a bug",
    );
    assert_eq!(err.kind, ErrorKind::AssemblyFailed);
    assert_eq!(err.detail("section"), Some("skill:verify"));
    assert_eq!(err.detail("placeholder"), Some("{task_id}"));
}

// ---- AC5.3 — unsupported brace tokens pass through + warn ---------------

#[test]
fn unsupported_brace_tokens_finds_placeholder_shaped_tokens_outside_the_supported_set() {
    let content = "See {foo_bar} and {task_id} and {dispatch_id} for details.";
    let tokens = unsupported_brace_tokens(content);
    assert_eq!(
        tokens,
        vec!["{foo_bar}".to_string()],
        "supported placeholders ({{task_id}}, {{dispatch_id}}) must not be \
         reported; only the unsupported {{foo_bar}} should surface"
    );
}

#[test]
fn unsupported_brace_tokens_ignores_non_identifier_shaped_braces() {
    // Rust code / JSON braces legitimately appearing in skill content
    // must not be misreported as unresolved placeholders (R5: "skill
    // content legitimately contains braces").
    let content = "fn foo() { let x = Self {}; } and {\"key\": \"value\"} and { spaced }";
    assert!(
        unsupported_brace_tokens(content).is_empty(),
        "non-identifier-shaped brace content must not be treated as a \
         placeholder token"
    );
}

#[test]
fn unsupported_brace_tokens_deduplicates_within_one_call() {
    let content = "{ghost} appears twice: {ghost}.";
    let tokens = unsupported_brace_tokens(content);
    assert_eq!(
        tokens,
        vec!["{ghost}".to_string()],
        "a repeated unsupported token within one section must be reported once"
    );
}

#[test]
fn unsupported_brace_tokens_excludes_hyphen_and_dot_shaped_tokens() {
    // Deliberate v0 recall sacrifice (Task 9 Q1 ruling; upheld by Warden
    // review dispatch-1642 with a corrected cost accounting — see below).
    // The token shape `is_placeholder_ident` accepts is `{identifier}` —
    // ASCII alphanumeric + underscore only. A hyphenated or dotted
    // block-id-shaped typo like `{merge-msg}` or `{a.b}` is NOT warned;
    // widening this shape to accept `-`/`.` is a behavior change that
    // must break this test.
    //
    // The cost of this choice is NOT the false negative it looks like at
    // first glance. Typo-recall largely survives: every supported
    // placeholder is snake_case, and snake_case typos stay snake_case
    // (`{worktree_paths}`, `{wortree_path}`, `{dispatchid}` are all still
    // caught) — `{merge-msg}` requires confusing a block id for a
    // placeholder, a narrow slice of typo-space.
    //
    // The more consequential cost is the opposite direction: false
    // POSITIVES on code content. `is_ascii_alphanumeric()` accepts
    // digits, so `{0}`/`{1}` (Rust positional format args) and `{name}`
    // in `format!("{name}")` DO warn — real noise on Rust-focused skill
    // files that legitimately contain format strings. That noise is
    // accepted deliberately: false negatives degrade gracefully (the
    // token passes through verbatim, visible in the document — AC5.3's
    // pass-through is by design), while false positives train operators
    // to ignore warnings. AC5.3 itself justifies pass-through with
    // "skill content legitimately contains braces," so biasing toward
    // precision over recall here is the spec-aligned instinct.
    assert!(unsupported_brace_tokens("see {merge-msg} and {a.b}").is_empty());
    assert_eq!(
        unsupported_brace_tokens("see {merge_msg}"),
        vec!["{merge_msg}".to_string()]
    );
}

#[test]
fn unsupported_brace_token_survives_verbatim_and_is_recorded_as_a_warning() {
    let registry = sample_registry();
    let resolver = FakeResolver {
        files: BTreeMap::from([
            ("team/implementer.md", "PROFILE CONTENT"),
            (
                "skills/verify.md",
                "Configure via {some_unknown_token} in your environment.",
            ),
            ("skills/rust.md", "RUST SKILL CONTENT"),
            ("skills/dispatch-metrics.md", "METRICS BLOCK CONTENT"),
        ]),
    };
    let request = sample_request();

    let assembled = assemble_standard(&request, &registry, &resolver)
        .expect("an unsupported brace token must not fail assembly");

    // AC5.3 — passed through untouched.
    assert!(
        assembled
            .document
            .contains("Configure via {some_unknown_token} in your environment.")
    );
    // AC5.3 — surfaced in warnings for operator review.
    assert_eq!(assembled.warnings.len(), 1);
    assert!(assembled.warnings[0].contains("{some_unknown_token}"));
    assert!(assembled.warnings[0].contains("skill:verify"));
}

#[test]
fn warnings_stay_distinct_across_sections() {
    // Within-section dedup (the repeated {ghost} inside skill:verify
    // collapsing to one warning) is proved by
    // `unsupported_brace_tokens_deduplicates_within_one_call` — that
    // dedup happens inside `unsupported_brace_tokens` itself, before
    // `record_brace_warnings` ever sees the token list. This test proves
    // the distinct claim: the *same* token recurring in a *different*
    // section is not collapsed into the first section's warning.
    let registry = sample_registry();
    let resolver = FakeResolver {
        files: BTreeMap::from([
            ("team/implementer.md", "PROFILE CONTENT"),
            (
                "skills/verify.md",
                "{ghost} shows up twice in this skill: {ghost}.",
            ),
            ("skills/rust.md", "RUST SKILL CONTENT"),
            (
                "skills/dispatch-metrics.md",
                "{ghost} also appears in this block.",
            ),
        ]),
    };
    let request = sample_request();

    let assembled = assemble_standard(&request, &registry, &resolver)
        .expect("unsupported brace tokens must not fail assembly");

    assert_eq!(
        assembled.warnings.len(),
        2,
        "the repeated {{ghost}} within skill:verify collapses to one warning, \
         but the separate occurrence in block:metrics is a distinct warning: {:?}",
        assembled.warnings
    );
    assert!(
        assembled
            .warnings
            .iter()
            .any(|w| w.contains("skill:verify"))
    );
    assert!(
        assembled
            .warnings
            .iter()
            .any(|w| w.contains("block:metrics"))
    );
}

// ============================================================================
// Task 8 — R6 weight classes: light-weight profile section extraction
// (AC6.1), fixed skill lists bypassing the pattern mapping (R6), weight
// block-list intersection with `include` rules (R6), and the light-vs-
// standard size delta (AC6.2). Exercises the new `assemble` entry point
// exclusively — `assemble_standard` (Task 4/9, tests above) is untouched.
// ============================================================================

/// A profile fixture with four top-level XML-tagged sections (`role`,
/// `persona`, `principles`, `command-scope`) plus two NESTED-only tags
/// (`error-handling` inside `principles`, `allowed` inside
/// `command-scope`) — proves AC6.1's "top-level only" rule: a nested-only
/// tag must not be extractable even though its name is a genuine XML tag
/// somewhere in the file.
const PROFILE_WITH_SECTIONS: &str = "\
<role>
Forge — Backend Developer
</role>

<persona>
Thinks in types and constraints.
</persona>

<principles>
<error-handling>
- Errors are values, not exceptions to ignore.
</error-handling>
</principles>

<command-scope>
<allowed>
cargo, git
</allowed>
</command-scope>
";

/// Builds a `[weights.light]`-shaped [`WeightClass`], parameterized by
/// the three R6 axes so each test below only overrides the one it
/// exercises.
fn light_weight(
    profile_sections: AllOrList,
    skills: Option<Vec<String>>,
    blocks: AllOrList,
) -> WeightClass {
    WeightClass {
        profile_sections,
        skills,
        blocks,
    }
}

/// [`sample_registry`] plus a `"light"` weight class — the common base
/// for the Task 8 tests, each overriding only the axis it exercises.
fn registry_with_light(weight: WeightClass) -> Registry {
    let mut registry = sample_registry();
    registry.weights.insert("light".to_string(), weight);
    registry
}

/// [`full_resolver`] with `team/implementer.md` overridden to
/// [`PROFILE_WITH_SECTIONS`] — the common resolver for the
/// section-extraction tests.
fn resolver_with_sectioned_profile() -> FakeResolver {
    let mut files = full_resolver().files;
    files.insert("team/implementer.md", PROFILE_WITH_SECTIONS);
    FakeResolver { files }
}

// ---- AC6.1 — section extraction, in profile order -----------------------

#[test]
fn assemble_extracts_named_top_level_sections_in_profile_order() {
    // Weight lists "command-scope" BEFORE "role" — the REVERSE of their
    // order in PROFILE_WITH_SECTIONS — to prove extraction follows the
    // profile's order, not the weight class's list order (R6/AC6.1).
    let registry = registry_with_light(light_weight(
        AllOrList::List(vec!["command-scope".to_string(), "role".to_string()]),
        None,
        AllOrList::All("all".to_string()),
    ));
    let resolver = resolver_with_sectioned_profile();
    let mut request = sample_request();
    request.weight = "light".to_string();

    let assembled = assemble(&request, &registry, &resolver)
        .expect("assembly should succeed with both requested sections present");

    let role_pos = assembled
        .document
        .find("<role>")
        .expect("role section extracted");
    let scope_pos = assembled
        .document
        .find("<command-scope>")
        .expect("command-scope section extracted");
    assert!(
        role_pos < scope_pos,
        "AC6.1: extraction order must follow the PROFILE's order (role \
         before command-scope), not the weight class's list order \
         (command-scope, role)"
    );
    assert!(
        !assembled.document.contains("<persona>"),
        "a top-level section not named in profile_sections must not appear"
    );
}

#[test]
fn assemble_treats_nested_only_tag_as_absent_from_the_profile() {
    // "error-handling" is a genuine XML tag in PROFILE_WITH_SECTIONS, but
    // only ever nested inside <principles> — never top-level. AC6.1's
    // outermost-depth rule must treat it as absent, not extract
    // <principles>'s inner content.
    let registry = registry_with_light(light_weight(
        AllOrList::List(vec!["error-handling".to_string()]),
        None,
        AllOrList::All("all".to_string()),
    ));
    let resolver = resolver_with_sectioned_profile();
    let mut request = sample_request();
    request.weight = "light".to_string();

    let err = assemble(&request, &registry, &resolver).expect_err(
        "a tag that only ever appears nested (never top-level) must be treated as absent",
    );
    assert_eq!(err.kind, ErrorKind::AssemblyFailed);
    assert_eq!(err.detail("agent"), Some("implementer"));
    assert_eq!(err.detail("tag"), Some("error-handling"));
}

#[test]
fn assemble_reports_missing_profile_section_as_assembly_failed_naming_agent_and_tag() {
    let registry = registry_with_light(light_weight(
        AllOrList::List(vec!["ghost-section".to_string()]),
        None,
        AllOrList::All("all".to_string()),
    ));
    let resolver = resolver_with_sectioned_profile();
    let mut request = sample_request();
    request.weight = "light".to_string();

    let err = assemble(&request, &registry, &resolver).expect_err(
        "a section named in the weight class but absent from the profile must fail assembly",
    );
    assert_eq!(err.kind, ErrorKind::AssemblyFailed);
    assert_eq!(err.detail("agent"), Some("implementer"));
    assert_eq!(err.detail("tag"), Some("ghost-section"));
}

// ---- Adversarial extraction cases (dispatch-mandated coverage) ----------

#[test]
fn extraction_does_not_treat_a_tag_name_as_a_prefix_match() {
    // Profile has only <roles> (plural) — no genuine top-level <role>.
    // Requesting "role" must report it absent, never accidentally match
    // <roles>'s span via a prefix/substring comparison.
    let profile = "\
<roles>
Not the tag you are looking for.
</roles>
";
    let registry = registry_with_light(light_weight(
        AllOrList::List(vec!["role".to_string()]),
        None,
        AllOrList::All("all".to_string()),
    ));
    let mut files = full_resolver().files;
    files.insert("team/implementer.md", profile);
    let resolver = FakeResolver { files };
    let mut request = sample_request();
    request.weight = "light".to_string();

    let err = assemble(&request, &registry, &resolver)
        .expect_err("'role' must not match a '<roles>' tag by prefix");
    assert_eq!(err.kind, ErrorKind::AssemblyFailed);
    assert_eq!(err.detail("tag"), Some("role"));
}

#[test]
fn extraction_distinguishes_role_and_roles_when_both_are_present() {
    let profile = "\
<role>
The real role section.
</role>

<roles>
A differently-named section that must not be confused with role.
</roles>
";
    let registry = registry_with_light(light_weight(
        AllOrList::List(vec!["role".to_string()]),
        None,
        AllOrList::All("all".to_string()),
    ));
    let mut files = full_resolver().files;
    files.insert("team/implementer.md", profile);
    let resolver = FakeResolver { files };
    let mut request = sample_request();
    request.weight = "light".to_string();

    let assembled = assemble(&request, &registry, &resolver).expect(
        "the genuine <role> tag should be extracted even with a same-prefixed <roles> present",
    );
    assert!(assembled.document.contains("The real role section."));
    assert!(
        !assembled.document.contains("A differently-named section"),
        "the unrequested <roles> section must not be included"
    );
}

#[test]
fn extraction_ignores_tags_appearing_inside_a_fenced_code_block() {
    // A doc-style profile with an illustrative ```xml-fenced example of
    // <role>...</role> BEFORE its genuine, real <role> section (mirrors
    // skills/xml-profile.md's own convention-skeleton fence). The fenced
    // example must not be mistaken for the real section.
    let profile = "\
Some intro prose.

```xml
<role>
EXAMPLE PLACEHOLDER — not real content.
</role>
```

<role>
Forge — Backend Developer
</role>
";
    let registry = registry_with_light(light_weight(
        AllOrList::List(vec!["role".to_string()]),
        None,
        AllOrList::All("all".to_string()),
    ));
    let mut files = full_resolver().files;
    files.insert("team/implementer.md", profile);
    let resolver = FakeResolver { files };
    let mut request = sample_request();
    request.weight = "light".to_string();

    let assembled = assemble(&request, &registry, &resolver)
        .expect("the real <role> section outside the fence should be extracted");
    assert!(
        assembled.document.contains("Forge — Backend Developer"),
        "the genuine <role> section must be extracted"
    );
    assert!(
        !assembled.document.contains("EXAMPLE PLACEHOLDER"),
        "a <role> example inside a fenced code block must not be treated as \
         the real top-level section"
    );
}

#[test]
fn extraction_treats_an_unclosed_tag_as_absent() {
    let profile = "\
<role>
This role tag is never closed.

<persona>
A real, properly closed section.
</persona>
";
    let registry = registry_with_light(light_weight(
        AllOrList::List(vec!["role".to_string()]),
        None,
        AllOrList::All("all".to_string()),
    ));
    let mut files = full_resolver().files;
    files.insert("team/implementer.md", profile);
    let resolver = FakeResolver { files };
    let mut request = sample_request();
    request.weight = "light".to_string();

    let err = assemble(&request, &registry, &resolver)
        .expect_err("an unclosed tag must be treated as absent, not extracted or panicking");
    assert_eq!(err.kind, ErrorKind::AssemblyFailed);
    assert_eq!(err.detail("tag"), Some("role"));
}

#[test]
fn extraction_still_finds_a_later_properly_closed_section_after_an_earlier_unclosed_tag() {
    // The unclosed <role> above must not corrupt scanning of what comes
    // after it — <persona>, closed correctly, should still be found.
    let profile = "\
<role>
This role tag is never closed.

<persona>
A real, properly closed section.
</persona>
";
    let registry = registry_with_light(light_weight(
        AllOrList::List(vec!["persona".to_string()]),
        None,
        AllOrList::All("all".to_string()),
    ));
    let mut files = full_resolver().files;
    files.insert("team/implementer.md", profile);
    let resolver = FakeResolver { files };
    let mut request = sample_request();
    request.weight = "light".to_string();

    let assembled = assemble(&request, &registry, &resolver).expect(
        "a later well-formed section should still be found despite an earlier unclosed tag",
    );
    assert!(
        assembled
            .document
            .contains("A real, properly closed section.")
    );
}

#[test]
fn extraction_close_matching_ignores_a_same_name_tag_inside_a_fenced_block() {
    // find_matching_close must mirror find_top_level_sections' own fence
    // awareness. A genuine top-level <principles> section's verbatim
    // content contains a fenced block whose illustrative example itself
    // opens with a bare <principles> line — exactly the documentation
    // shape is_fence_delimiter's own doc comment describes. Without
    // fence-awareness in find_matching_close, that fenced open drives
    // depth 1->2, so the real </principles> only returns depth to 1 and
    // no close is ever found before the content ends — the whole section
    // reads as unclosed and assembly fails loudly. With the fix, the
    // fenced line is skipped and the real close is matched, with the
    // fenced content surviving verbatim inside the extracted span.
    let profile = "\
<principles>
Real guardrail text before the fence.

```text
<principles>
Illustrative fenced example — not a real nested section.
```

Real guardrail text after the fence.
</principles>
";
    let registry = registry_with_light(light_weight(
        AllOrList::List(vec!["principles".to_string()]),
        None,
        AllOrList::All("all".to_string()),
    ));
    let mut files = full_resolver().files;
    files.insert("team/implementer.md", profile);
    let resolver = FakeResolver { files };
    let mut request = sample_request();
    request.weight = "light".to_string();

    let assembled = assemble(&request, &registry, &resolver).expect(
        "a top-level section containing a fenced same-name tag must still be \
         found and extracted, not misread as unclosed",
    );

    // Pin the exact span, not just success: the fenced <principles> line
    // must survive verbatim, bounded by the real open and real close —
    // a bare .is_ok() would pass even if the span were mis-bounded.
    let expected_span = "\
<principles>
Real guardrail text before the fence.

```text
<principles>
Illustrative fenced example — not a real nested section.
```

Real guardrail text after the fence.
</principles>";
    assert!(
        assembled.document.contains(expected_span),
        "the extracted <principles> span must be byte-identical from the \
         real open tag through the real close tag, with the fenced \
         <principles> line surviving verbatim inside it"
    );
}

#[test]
fn extraction_close_matching_ignores_a_same_name_close_tag_inside_a_fenced_block() {
    // Companion to the fenced-open case above: a fenced </principles>
    // line must not be mistaken for the real close either. Without
    // fence-awareness, that fenced close would drive depth 1->0 early,
    // truncating the span before the real close — silently dropping
    // "Real guardrail text after the fence." With the fix, the fenced
    // close is skipped and the real close (and everything up to it)
    // survives in the extracted span.
    let profile = "\
<principles>
Real guardrail text before the fence.

```text
</principles>
```

Real guardrail text after the fence.
</principles>
";
    let registry = registry_with_light(light_weight(
        AllOrList::List(vec!["principles".to_string()]),
        None,
        AllOrList::All("all".to_string()),
    ));
    let mut files = full_resolver().files;
    files.insert("team/implementer.md", profile);
    let resolver = FakeResolver { files };
    let mut request = sample_request();
    request.weight = "light".to_string();

    let assembled = assemble(&request, &registry, &resolver).expect(
        "a top-level section containing a fenced same-name close tag must \
         still extract through to its real close",
    );

    let expected_span = "\
<principles>
Real guardrail text before the fence.

```text
</principles>
```

Real guardrail text after the fence.
</principles>";
    assert!(
        assembled.document.contains(expected_span),
        "the extracted <principles> span must reach the real close tag, \
         not truncate early at the fenced </principles> line"
    );
}

// ---- R6 — fixed skills list is a floor, not a cap (maintainer ruling ------
// ---- 2026-07-16) -----------------------------------------------------

#[test]
fn assemble_fixed_skills_list_replaces_only_the_pattern_mapping_term() {
    // The "implementation" pattern's skills are ["verify", "rust"] (see
    // sample_registry) — the weight's fixed list deliberately picks the
    // OPPOSITE order, ["rust", "verify"], so the exact-vector assertion
    // below pins that a fixed list REPLACES the pattern's own array (both
    // membership and order) as R5 step 2's pattern-mapping term.
    //
    // This test does NOT exercise skills_add/skills_remove — the
    // maintainer's 2026-07-16 ruling overturned an earlier reading of
    // this function that also ignored those two fields entirely (a fixed
    // list treated as the complete effective set). The ruling: the fixed
    // list is a FLOOR, not a cap — R5 step 2's other two terms still
    // apply on top of it. See the merge tests below for skills_add/
    // skills_remove coverage; this test isolates the narrower claim (the
    // fixed list, not the pattern's array, is the merge's base term).
    let registry = registry_with_light(light_weight(
        AllOrList::All("all".to_string()),
        Some(vec!["rust".to_string(), "verify".to_string()]),
        AllOrList::All("all".to_string()),
    ));
    let resolver = full_resolver();
    let mut request = sample_request();
    request.weight = "light".to_string();

    let assembled = assemble(&request, &registry, &resolver)
        .expect("fixed-skill weight should assemble cleanly");

    let skill_sections: Vec<&str> = assembled
        .components
        .iter()
        .map(|c| c.section.as_str())
        .filter(|s| s.starts_with("skill:"))
        .collect();
    assert_eq!(
        skill_sections,
        vec!["skill:rust", "skill:verify"],
        "a fixed skills list is the merge's base term IN LISTED ORDER \
         (rust before verify — the reverse of the pattern's own order) — \
         the pattern mapping's own array and order play no part when a \
         weight pins a fixed list"
    );
}

#[test]
fn assemble_light_weight_appends_skills_add_on_top_of_the_fixed_list() {
    // The maintainer's own worked example from the 2026-07-16 ruling:
    // "if you ask for light and then also add rust, that is what should
    // happen." The fixed list ["verify"] is a FLOOR, not the complete
    // effective set — skills_add still appends "rust" on top of it. The
    // overturned (whole-merge-bypass) reading would silently drop "rust"
    // and leave just ["verify"]; this test fails under that reading.
    let registry = registry_with_light(light_weight(
        AllOrList::All("all".to_string()),
        Some(vec!["verify".to_string()]),
        AllOrList::All("all".to_string()),
    ));
    let resolver = full_resolver();
    let mut request = sample_request();
    request.weight = "light".to_string();
    request.skills_add = vec!["rust".to_string()];

    let assembled = assemble(&request, &registry, &resolver)
        .expect("skills_add on top of a fixed list should assemble cleanly");

    let skill_sections: Vec<&str> = assembled
        .components
        .iter()
        .map(|c| c.section.as_str())
        .filter(|s| s.starts_with("skill:"))
        .collect();
    assert_eq!(
        skill_sections,
        vec!["skill:verify", "skill:rust"],
        "a weight's fixed skills list is a FLOOR, not a cap — skills_add \
         still appends on top of it (maintainer ruling 2026-07-16): \
         result must be exactly [verify, rust], not [verify] alone"
    );
}

#[test]
fn assemble_light_weight_dedups_skills_add_entry_already_in_the_fixed_list() {
    // "verify" is already in the fixed list at position 0 — re-naming it
    // via skills_add must not duplicate it or move it; dedup keeps the
    // FIRST occurrence (the fixed-list position), same rule as the
    // pattern-based merge's Gap-2 dedup.
    let registry = registry_with_light(light_weight(
        AllOrList::All("all".to_string()),
        Some(vec!["verify".to_string()]),
        AllOrList::All("all".to_string()),
    ));
    let resolver = full_resolver();
    let mut request = sample_request();
    request.weight = "light".to_string();
    request.skills_add = vec!["verify".to_string(), "rust".to_string()];

    let assembled = assemble(&request, &registry, &resolver)
        .expect("skills_add merge on top of a fixed list should assemble cleanly");

    let skill_sections: Vec<&str> = assembled
        .components
        .iter()
        .map(|c| c.section.as_str())
        .filter(|s| s.starts_with("skill:"))
        .collect();
    assert_eq!(
        skill_sections,
        vec!["skill:verify", "skill:rust"],
        "a skills_add entry already present in the fixed list must not be \
         duplicated — dedup keeps the fixed list's first occurrence"
    );
    assert_eq!(
        assembled.document.matches("VERIFY SKILL CONTENT").count(),
        1,
        "a skill present via both the fixed list and skills_add must be \
         included exactly once"
    );
}

#[test]
fn validate_request_accepts_skills_remove_of_a_fixed_list_member_under_a_weight() {
    // Regression test for the bug the maintainer's ruling exposed:
    // `validate_request` used to check `skills_remove` membership only
    // against the *pattern's* skills (["verify", "rust"] for
    // "implementation" — see sample_registry), regardless of
    // `request.weight`. "tdd" is a real registry skill, pinned into the
    // "light" weight's fixed list, but is NOT part of the
    // "implementation" pattern's own array nor skills_add — so the old
    // check would find it absent from its (wrong) base and reject a
    // request that the narrow ruling says is legitimate: the tool
    // refusing a request it can satisfy.
    let mut registry = registry_with_light(light_weight(
        AllOrList::All("all".to_string()),
        Some(vec!["verify".to_string(), "tdd".to_string()]),
        AllOrList::All("all".to_string()),
    ));
    registry.skills.insert(
        "tdd".to_string(),
        SkillEntry {
            path: "skills/tdd.md".to_string(),
        },
    );
    let mut request = sample_request();
    request.weight = "light".to_string();
    request.skills_remove = vec!["tdd".to_string()];

    validate_request(&request, &registry)
        .expect("skills_remove of a weight's own fixed-list member must be accepted, not rejected");
}

#[test]
fn assemble_light_weight_honors_skills_remove_of_a_fixed_list_member() {
    // Assembly-time counterpart to the validate_request test above —
    // proves the removal actually takes effect at assembly time, not
    // just that validation lets the request through.
    let mut registry = registry_with_light(light_weight(
        AllOrList::All("all".to_string()),
        Some(vec!["verify".to_string(), "tdd".to_string()]),
        AllOrList::All("all".to_string()),
    ));
    registry.skills.insert(
        "tdd".to_string(),
        SkillEntry {
            path: "skills/tdd.md".to_string(),
        },
    );
    let mut files = full_resolver().files;
    files.insert("skills/tdd.md", "TDD SKILL CONTENT");
    let resolver = FakeResolver { files };
    let mut request = sample_request();
    request.weight = "light".to_string();
    request.skills_remove = vec!["tdd".to_string()];

    let assembled = assemble(&request, &registry, &resolver)
        .expect("removing a fixed-list member via skills_remove should assemble cleanly");

    let skill_sections: Vec<&str> = assembled
        .components
        .iter()
        .map(|c| c.section.as_str())
        .filter(|s| s.starts_with("skill:"))
        .collect();
    assert_eq!(
        skill_sections,
        vec!["skill:verify"],
        "skills_remove must drop a fixed-list member from the effective set"
    );
    assert!(!assembled.document.contains("TDD SKILL CONTENT"));
}

#[test]
fn assemble_reports_dangling_fixed_skill_as_config_invalid() {
    let registry = registry_with_light(light_weight(
        AllOrList::All("all".to_string()),
        Some(vec!["ghost-skill".to_string()]),
        AllOrList::All("all".to_string()),
    ));
    let resolver = full_resolver();
    let mut request = sample_request();
    request.weight = "light".to_string();

    let err = assemble(&request, &registry, &resolver)
        .expect_err("a fixed-skill list referencing an undeclared skill should fail assembly");
    assert_eq!(err.kind, ErrorKind::ConfigInvalid);
    assert_eq!(err.detail("weight"), Some("light"));
    assert_eq!(err.detail("skill"), Some("ghost-skill"));
}

// ---- R6 — weight block list intersects with `include` rules --------------

#[test]
fn assemble_block_list_still_respects_worktree_condition_in_non_worktree_dispatch() {
    // Weight lists BOTH "metrics" (always) and "merge-msg" (worktree) —
    // R6: "a weight blocks list intersects with the include rules (a
    // listed block still respects worktree/task conditions)". A
    // non-worktree dispatch must still drop merge-msg even though it's
    // explicitly named in the weight's block list.
    let registry = registry_with_light(light_weight(
        AllOrList::All("all".to_string()),
        None,
        AllOrList::List(vec!["metrics".to_string(), "merge-msg".to_string()]),
    ));
    let resolver = full_resolver();
    let mut request = sample_request();
    request.weight = "light".to_string();
    request.envelope.worktree = None;

    let assembled = assemble(&request, &registry, &resolver)
        .expect("block-list weight should assemble cleanly in a non-worktree dispatch");

    assert!(assembled.document.contains("METRICS BLOCK CONTENT"));
    assert!(
        !assembled.document.contains("MERGE MSG BLOCK CONTENT"),
        "a worktree-conditioned block named in the weight's block list must \
         still be dropped when the dispatch has no worktree — intersection, \
         not replacement, of the include rules"
    );
}

#[test]
fn assemble_block_list_excludes_a_block_not_named_even_when_include_condition_is_satisfied() {
    // The other half of "intersects": "task-tracking" satisfies its
    // include=task condition (task_id is set) but is NOT in the weight's
    // block list — it must still be excluded.
    let registry = registry_with_light(light_weight(
        AllOrList::All("all".to_string()),
        None,
        AllOrList::List(vec!["metrics".to_string()]),
    ));
    let resolver = full_resolver();
    let mut request = sample_request();
    request.weight = "light".to_string();
    request.envelope.task_id = Some(7);

    let assembled = assemble(&request, &registry, &resolver)
        .expect("block-list weight should assemble cleanly");

    assert!(assembled.document.contains("METRICS BLOCK CONTENT"));
    assert!(
        !assembled.document.contains("TASK TRACKING BLOCK CONTENT"),
        "a block satisfying its include condition but absent from the \
         weight's block list must still be excluded"
    );
}

// ---- AC6.2 — light-vs-standard size delta is observable -------------------

#[test]
fn light_dispatch_produces_a_smaller_document_than_standard() {
    let mut registry = sample_registry();
    registry.weights.insert(
        "light".to_string(),
        light_weight(
            AllOrList::List(vec!["role".to_string()]),
            Some(vec!["verify".to_string()]),
            AllOrList::List(vec!["metrics".to_string()]),
        ),
    );
    let resolver = resolver_with_sectioned_profile();

    let mut standard_request = sample_request();
    standard_request.weight = "standard".to_string();
    let standard = assemble(&standard_request, &registry, &resolver)
        .expect("standard weight should assemble cleanly");

    let mut light_request = sample_request();
    light_request.weight = "light".to_string();
    let light = assemble(&light_request, &registry, &resolver)
        .expect("light weight should assemble cleanly");

    let standard_bytes: u64 = standard.components.iter().map(|c| c.bytes).sum();
    let light_bytes: u64 = light.components.iter().map(|c| c.bytes).sum();
    assert!(
        light_bytes < standard_bytes,
        "AC6.2: a light dispatch must be observably smaller than standard \
         in the reported size accounting — standard={standard_bytes} light={light_bytes}"
    );
    // AC6.2's other half: the summary (Summary.weight, wired in
    // cmd/dispcli/main.rs) reports which weight class applied — proven at
    // the `dispcli-core` layer by the `request.weight` field the caller
    // already echoes verbatim; not re-tested here since that plumbing
    // predates Task 8 and is unchanged by it.
}

// ---- Defense-in-depth + equivalence with assemble_standard ----------------

#[test]
fn assemble_reports_unknown_weight_as_request_invalid() {
    let registry = sample_registry();
    let resolver = full_resolver();
    let mut request = sample_request();
    request.weight = "ghost-weight".to_string();

    let err = assemble(&request, &registry, &resolver)
        .expect_err("an unknown weight id should fail assembly");
    assert_eq!(err.kind, ErrorKind::RequestInvalid);
    assert_eq!(err.detail("field"), Some("weight"));
    assert_eq!(err.detail("value"), Some("ghost-weight"));
}

#[test]
fn assemble_matches_assemble_standard_for_the_trivial_weight_shape() {
    // The generalized `assemble` seam must be a strict superset of
    // `assemble_standard`'s behavior: given the literal
    // `"all"`/`None`/`"all"` weight-class shape (`sample_registry`'s
    // "standard" weight), output must be byte-identical.
    let registry = sample_registry();
    let resolver = full_resolver();
    let mut request = sample_request();
    request.weight = "standard".to_string();

    let via_assemble = assemble(&request, &registry, &resolver)
        .expect("assemble should succeed for the standard shape");
    let via_assemble_standard = assemble_standard(&request, &registry, &resolver)
        .expect("assemble_standard should succeed");

    assert_eq!(via_assemble.document, via_assemble_standard.document);
    assert_eq!(via_assemble.components, via_assemble_standard.components);
    assert_eq!(via_assemble.warnings, via_assemble_standard.warnings);
}
