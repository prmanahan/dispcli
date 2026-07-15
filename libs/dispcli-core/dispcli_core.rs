//! dispcli-core — IO-free dispatch-envelope construction.
//!
//! This crate is the long-lived core. It must remain IO-free: no
//! `std::fs`, no `std::process`, no stdin/stdout. Inputs and outputs are
//! plain Rust structs; IO is provided by callers — `dispcli-io` for the
//! native CLI binary, host functions for the future mnemra WASM plugin.
//!
//! Task 1 scope: the dispatch **request** (R1), **registry** (R2),
//! **envelope** (R4), and **summary** (R8) data types, plus string-in
//! parsing for the request and registry. No validation beyond what
//! serde's own shape/enum checking gives for free, no assembly logic —
//! those land in later tasks.
//!
//! Task 2 scope: the `ContentResolver`/`DocumentSink` resolver traits
//! (R3, the WASM seam) and the R8 error taxonomy (`Error`, `ErrorKind`,
//! `Detail`) — the uniform failure currency every fallible function in
//! this crate returns. `parse_request`/`parse_registry` now map their
//! underlying `serde_json`/`toml` parse errors into `Error`
//! (`RequestInvalid`/`ConfigInvalid`) instead of returning the raw
//! library error types. No resolver/sink implementations (Task 3) and
//! no R7 field-path validation (Task 6) land here.
//! See `docs/specs/0001-envelope-assembly.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Returns the crate version, sourced from `Cargo.toml` at build time.
///
/// Used by the v0 scaffold binary to prove cross-crate wiring works
/// end-to-end. Safe to delete once the real public surface lands.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// ============================================================================
// Closed enums (R1 "closed vocabularies" note; R2 AC2.3; R7 mode/tier rows)
// ============================================================================

/// Dispatch tier — caller judgment, recorded and echoed in the summary,
/// never branches assembly behavior in v0 (reserved for v1 metrics
/// emission per R7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    T1,
    T2,
    T3,
}

/// Claude Code permission mode for a dispatch. Closed to exactly these
/// four values (R7) — `"plan"` is a real permission mode elsewhere in the
/// harness but is deliberately excluded here; rejecting it with the
/// dedicated "plan mode is not dispatchable" message is R7 validation
/// logic (a later task), not this type. Here it just needs to fail to
/// deserialize like any other unrecognized value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    Default,
    AcceptEdits,
    BypassPermissions,
    DontAsk,
}

/// Template-block inclusion condition (R2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Include {
    Always,
    Worktree,
    Task,
}

// ============================================================================
// R1 — Dispatch request
// ============================================================================

/// A caller-supplied capability/reason pair overriding an agent's command
/// scope (`command_scope_add` / `command_scope_subtract`, R1). `reason`
/// being required and non-empty is an R7 validation rule for a later
/// task; this type only fixes the shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeOverride {
    pub capability: String,
    pub reason: String,
}

/// The `envelope` facts nested inside a dispatch request (R1). Distinct
/// from [`Envelope`] (R4): this is the raw caller input before
/// `agent_id` is derived from `request.agent` and `report_path` is
/// defaulted (AC4.2) — that construction is assembly logic for a later
/// task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvelopeRequest {
    pub dispatch_id: i64,
    #[serde(default)]
    pub task_id: Option<i64>,
    #[serde(default)]
    pub spec_id: Option<String>,
    #[serde(default)]
    pub spec_version: Option<String>,
    pub parent_commit: String,
    pub repo: String,
    #[serde(default)]
    pub worktree: Option<String>,
    pub branch: String,
    #[serde(default)]
    pub report_path: Option<String>,
    #[serde(default)]
    pub deadline_minutes: Option<i64>,
    #[serde(default)]
    pub command_scope_subtract: Vec<ScopeOverride>,
    #[serde(default)]
    pub command_scope_add: Vec<ScopeOverride>,
    #[serde(default)]
    pub touch_scope: Vec<String>,
    #[serde(default)]
    pub forbid_scope: Vec<String>,
    #[serde(default)]
    pub verify: Vec<String>,
}

/// A dispatch request (R1) — what the orchestrator wants to run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchRequest {
    pub agent: String,
    pub task_pattern: String,
    pub tier: Tier,
    #[serde(default = "default_weight")]
    pub weight: String,
    #[serde(default)]
    pub mode_override: Option<PermissionMode>,
    #[serde(default)]
    pub skills_add: Vec<String>,
    #[serde(default)]
    pub skills_remove: Vec<String>,
    pub task_body: String,
    pub envelope: EnvelopeRequest,
}

fn default_weight() -> String {
    "standard".to_string()
}

/// Parse a dispatch request from its JSON string form (R1). No file read
/// — the CLI resolves `--request <path|->` to a string before calling
/// this; `dispcli-core` never touches a filesystem or stdin.
///
/// # Errors
/// Returns a [`RequestInvalid`](ErrorKind::RequestInvalid) [`Error`] on
/// malformed JSON, a missing required field, an unknown top-level or
/// `envelope` key (`deny_unknown_fields`), or a value outside a closed
/// enum (`tier`, `mode_override`) — the message is the underlying
/// `serde_json` error's `Display`. Field-path-aware validation errors
/// (R7) are a later task.
pub fn parse_request(input: &str) -> Result<DispatchRequest, Error> {
    serde_json::from_str(input).map_err(Error::from)
}

// ============================================================================
// R2 — Registry config
// ============================================================================

/// `[registry]` table — top-level registry metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryMeta {
    /// Reserved in v0 (AC2.5) — not applied as a path prefix. All path
    /// resolution is relative to the registry file's directory.
    pub skills_root: String,
}

/// `[agents.<id>]` — one registry-declared agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEntry {
    pub profile: String,
    pub default_mode: PermissionMode,
    pub worktree_required: bool,
}

/// `[skills.<id>]` — one registry-declared skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillEntry {
    pub path: String,
}

/// `[patterns.<id>]` — the ordered skill set a `task_pattern` maps to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternEntry {
    pub skills: Vec<String>,
}

/// `[blocks.<id>]` — one template block (R2, R5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockEntry {
    pub path: String,
    pub include: Include,
}

/// `[blocks]` — the `order` array plus every `[blocks.<id>]` sub-table.
/// TOML represents both as keys of the same `blocks` table, so the
/// per-block entries are captured via `#[serde(flatten)]` into a map
/// keyed by block id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlocksSection {
    pub order: Vec<String>,
    #[serde(flatten)]
    pub blocks: BTreeMap<String, BlockEntry>,
}

/// Either the literal sentinel `"all"` or an explicit list of ids
/// (`profile_sections` / `blocks` in a `[weights.<id>]` table, R6).
/// Confirming the string variant is literally `"all"` — as opposed to an
/// unrecognized string — is R6 validation logic for a later task; this
/// type only fixes the shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AllOrList {
    All(String),
    List(Vec<String>),
}

/// `[weights.<id>]` — a weight class (R6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeightClass {
    pub profile_sections: AllOrList,
    #[serde(default)]
    pub skills: Option<Vec<String>>,
    pub blocks: AllOrList,
}

/// The registry (R2) — the orchestrator's inventory of agents, skills,
/// patterns, template blocks, and weight classes. The portability
/// boundary: adopters describe their own inventory here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    pub registry: RegistryMeta,
    #[serde(default)]
    pub agents: BTreeMap<String, AgentEntry>,
    #[serde(default)]
    pub skills: BTreeMap<String, SkillEntry>,
    #[serde(default)]
    pub patterns: BTreeMap<String, PatternEntry>,
    pub blocks: BlocksSection,
    #[serde(default)]
    pub weights: BTreeMap<String, WeightClass>,
}

/// Parse a registry from its TOML string form (R2). No file read —
/// `dispcli-io` resolves `--config <path>` to a string before calling
/// this (AC2.4).
///
/// # Errors
/// Returns a [`ConfigInvalid`](ErrorKind::ConfigInvalid) [`Error`] on
/// malformed TOML, a missing required field, or a value outside a
/// closed enum (`default_mode`, `include`) — the message is the
/// underlying `toml` error's `Display`.
pub fn parse_registry(input: &str) -> Result<Registry, Error> {
    toml::from_str(input).map_err(Error::from)
}

// ============================================================================
// R8 — Error taxonomy (defined here, ahead of its spec-numbering slot,
// because the R3 resolver traits immediately below return `Error` —
// this keeps the file readable top-to-bottom; item order has no
// compile-time effect in Rust)
// ============================================================================

/// One of the six kinds in the R8 error taxonomy. Every fallible
/// function in `dispcli-core` resolves to exactly one of these — see
/// [`ErrorKind::exit_code`] for the kind→exit-code mapping the CLI uses
/// to set its process exit status (AC8.2). Treated as a closed
/// vocabulary (R1 closed-vocabulary note) — same non-`#[non_exhaustive]`
/// treatment as [`Tier`]/[`PermissionMode`]/[`Include`]: the R8 table is
/// a fixed six-entry set, not one adopters extend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Bad CLI flags/arguments — `cmd/dispcli` only, never emitted by
    /// this crate.
    Usage,
    /// R1/R7 request-side failures.
    RequestInvalid,
    /// R2 registry failures.
    ConfigInvalid,
    /// R3 content resolution failures.
    ResolutionFailed,
    /// R5/R6 assembly failures (unresolved placeholder, missing section).
    AssemblyFailed,
    /// `DocumentSink` write failures.
    IoFailed,
}

impl ErrorKind {
    /// The process exit code this kind maps to (R8 table, AC8.2).
    #[must_use]
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorKind::Usage => 2,
            ErrorKind::RequestInvalid => 3,
            ErrorKind::ConfigInvalid => 4,
            ErrorKind::ResolutionFailed => 5,
            ErrorKind::AssemblyFailed => 6,
            ErrorKind::IoFailed => 7,
        }
    }
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ErrorKind::Usage => "usage",
            ErrorKind::RequestInvalid => "request_invalid",
            ErrorKind::ConfigInvalid => "config_invalid",
            ErrorKind::ResolutionFailed => "resolution_failed",
            ErrorKind::AssemblyFailed => "assembly_failed",
            ErrorKind::IoFailed => "io_failed",
        };
        f.write_str(s)
    }
}

/// One `key`/`value` entry in an [`Error`]'s `details` — structured
/// context beyond the human-readable `message` (e.g. a
/// `resolution_failed` error carries `id`, `path`, and `cause` entries,
/// AC3.3). Ordered, append-only via [`Error::with_detail`] — callers
/// look values up by name ([`Error::detail`]), not by position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    pub key: String,
    pub value: String,
}

impl Detail {
    #[must_use]
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Detail {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// The uniform error shape every `dispcli-core` failure path returns
/// (R8): a `kind` closed to the six [`ErrorKind`] values, a
/// human-readable `message`, and structured `details` a caller
/// (`cmd/dispcli`) renders into the
/// `{"error": {"kind", "message", "details": [...]}}` stderr payload.
/// This is the sole error currency the crate returns on any production
/// path — no panics (AC8.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
    pub details: Vec<Detail>,
}

impl Error {
    /// Build an error of `kind` with `message` and no details.
    #[must_use]
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Error {
            kind,
            message: message.into(),
            details: Vec::new(),
        }
    }

    /// Append one `key`/`value` detail, builder-style.
    #[must_use]
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.push(Detail::new(key, value));
        self
    }

    /// The value of the first detail recorded under `key`, if any.
    #[must_use]
    pub fn detail(&self, key: &str) -> Option<&str> {
        self.details
            .iter()
            .find(|d| d.key == key)
            .map(|d| d.value.as_str())
    }

    /// Build a `resolution_failed` error carrying the registry `id`,
    /// the resolved `path`, and the underlying `cause` (AC3.3). The
    /// native `dispcli-io` resolver (Task 3) is the primary caller —
    /// nothing in `dispcli-core` invokes this yet.
    #[must_use]
    pub fn resolution_failed(
        id: impl Into<String>,
        path: impl Into<String>,
        cause: impl Into<String>,
    ) -> Self {
        let id = id.into();
        let path = path.into();
        let cause = cause.into();
        Error::new(
            ErrorKind::ResolutionFailed,
            format!("failed to resolve '{id}' at '{path}': {cause}"),
        )
        .with_detail("id", id)
        .with_detail("path", path)
        .with_detail("cause", cause)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    /// Maps a request-parse failure into `request_invalid` (R1/AC1.1
    /// territory). The message is the raw `serde_json` `Display`; no
    /// field-path extraction — that's R7 validation (Task 6).
    fn from(err: serde_json::Error) -> Self {
        Error::new(ErrorKind::RequestInvalid, err.to_string())
    }
}

impl From<toml::de::Error> for Error {
    /// Maps a registry-parse failure into `config_invalid` (R2/AC2.1
    /// territory). Same minimal-mapping rationale as the `serde_json`
    /// impl above.
    fn from(err: toml::de::Error) -> Self {
        Error::new(ErrorKind::ConfigInvalid, err.to_string())
    }
}

// ============================================================================
// R3 — Resolver traits (IO boundary)
// ============================================================================

/// Registry-declared content source — the WASM seam (R3). Given the
/// registry key `id` a path came from (e.g. `"rust"` for
/// `[skills.rust]`) and its declared `path`, returns the file's content
/// as a string. The native implementation (`dispcli-io`, Task 3) reads
/// from the filesystem rooted at the registry file's directory; the
/// future WASM host implements the same trait over host functions. No
/// concrete filesystem type crosses this boundary — only
/// borrowed/owned strings (AC3.1's IO-free invariant extends to this
/// trait's signature, not just this crate's own code).
pub trait ContentResolver {
    /// Resolve `path` (declared under registry key `id`) to its content.
    ///
    /// # Errors
    /// A [`ResolutionFailed`](ErrorKind::ResolutionFailed) [`Error`]
    /// when the path cannot be resolved — implementations populate
    /// `id`, `path`, and the underlying cause via
    /// [`Error::resolution_failed`] (AC3.3).
    fn resolve(&self, id: &str, path: &str) -> Result<String, Error>;
}

/// Assembled-document persistence — the other half of the WASM seam
/// (R3). Given the output `path` and the assembled `document`,
/// persists it. The native implementation (`dispcli-io`, Task 3) writes
/// the file, creating parent directories; summary emission stays in
/// `cmd/dispcli`. No concrete filesystem type crosses this boundary.
pub trait DocumentSink {
    /// Persist `document` at `path`.
    ///
    /// # Errors
    /// An [`IoFailed`](ErrorKind::IoFailed) [`Error`] when the write
    /// fails.
    fn write(&self, path: &str, document: &str) -> Result<(), Error>;
}

// ============================================================================
// R4 — Envelope (assembled document header)
// ============================================================================

/// The fully-assembled envelope (R4) — every schema key always present,
/// `agent_id` derived from `request.agent`, `report_path` defaulted per
/// AC4.2. Constructing one from a [`DispatchRequest`] is assembly logic
/// for a later task; this type only fixes the shape.
///
/// Emission to the document's YAML frontmatter is hand-rolled (see the
/// plan's architectural invariants) — generic YAML serializers don't
/// reliably guarantee AC4.1's explicit-`null`-never-omitted behavior or
/// AC4.3's byte-stable field order, so this type is never run through a
/// YAML crate anywhere in the codebase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub dispatch_id: i64,
    pub task_id: Option<i64>,
    pub agent_id: String,
    pub spec_id: Option<String>,
    pub spec_version: Option<String>,
    pub parent_commit: String,
    pub repo: String,
    pub worktree: Option<String>,
    pub branch: String,
    pub report_path: String,
    pub deadline_minutes: Option<i64>,
    pub command_scope_subtract: Vec<ScopeOverride>,
    pub command_scope_add: Vec<ScopeOverride>,
    pub touch_scope: Vec<String>,
    pub forbid_scope: Vec<String>,
    pub verify: Vec<String>,
}

impl Envelope {
    /// Constructs the envelope (R4) from a request's `agent` and nested
    /// `envelope` facts. `agent_id` is `request.agent` verbatim. When
    /// `envelope.report_path` is null, it defaults to
    /// `{worktree or repo}/scratch/dispatch-{dispatch_id}-report.md`
    /// (AC4.2) — worktree takes precedence over repo when present.
    #[must_use]
    pub fn from_request(request: &DispatchRequest) -> Self {
        let env = &request.envelope;
        let working_dir = env.worktree.as_deref().unwrap_or(env.repo.as_str());
        let report_path = env.report_path.clone().unwrap_or_else(|| {
            format!(
                "{working_dir}/scratch/dispatch-{}-report.md",
                env.dispatch_id
            )
        });
        Envelope {
            dispatch_id: env.dispatch_id,
            task_id: env.task_id,
            agent_id: request.agent.clone(),
            spec_id: env.spec_id.clone(),
            spec_version: env.spec_version.clone(),
            parent_commit: env.parent_commit.clone(),
            repo: env.repo.clone(),
            worktree: env.worktree.clone(),
            branch: env.branch.clone(),
            report_path,
            deadline_minutes: env.deadline_minutes,
            command_scope_subtract: env.command_scope_subtract.clone(),
            command_scope_add: env.command_scope_add.clone(),
            touch_scope: env.touch_scope.clone(),
            forbid_scope: env.forbid_scope.clone(),
            verify: env.verify.clone(),
        }
    }

    /// Renders this envelope as the document's YAML frontmatter block
    /// (R4) — hand-rolled string output, never a generic serializer (see
    /// the plan's architectural invariants: AC4.1's explicit-`null`
    /// and AC4.3's byte-stable field order aren't reliably guaranteed by
    /// one). Every key is always emitted; absent optionals render as the
    /// explicit token `null` (AC4.1); empty arrays render as `[]`; field
    /// order matches the R4 schema byte-for-byte modulo values (AC4.3).
    /// The returned string starts with `---` and ends with `---` (no
    /// trailing newline) — callers join it with the rest of the document
    /// via the standard inter-section separator so it becomes the first
    /// block (AC4.4).
    #[must_use]
    pub fn to_yaml_string(&self) -> String {
        let mut out = String::from("---\n");
        out.push_str(&format!("dispatch_id: {}\n", self.dispatch_id));
        out.push_str(&format!("task_id: {}\n", yaml_opt_int(self.task_id)));
        out.push_str(&format!(
            "agent_id: {}\n",
            yaml_scalar_string(&self.agent_id)
        ));
        out.push_str(&format!(
            "spec_id: {}\n",
            yaml_opt_string(self.spec_id.as_deref())
        ));
        out.push_str(&format!(
            "spec_version: {}\n",
            yaml_opt_string(self.spec_version.as_deref())
        ));
        out.push_str(&format!(
            "parent_commit: {}\n",
            yaml_scalar_string(&self.parent_commit)
        ));
        out.push_str(&format!("repo: {}\n", yaml_scalar_string(&self.repo)));
        out.push_str(&format!(
            "worktree: {}\n",
            yaml_opt_string(self.worktree.as_deref())
        ));
        out.push_str(&format!("branch: {}\n", yaml_scalar_string(&self.branch)));
        out.push_str(&format!(
            "report_path: {}\n",
            yaml_scalar_string(&self.report_path)
        ));
        out.push_str(&format!(
            "deadline_minutes: {}\n",
            yaml_opt_int(self.deadline_minutes)
        ));
        out.push_str(&format!(
            "command_scope_subtract: {}\n",
            yaml_scope_override_array(&self.command_scope_subtract)
        ));
        out.push_str(&format!(
            "command_scope_add: {}\n",
            yaml_scope_override_array(&self.command_scope_add)
        ));
        out.push_str(&format!(
            "touch_scope: {}\n",
            yaml_string_array(&self.touch_scope)
        ));
        out.push_str(&format!(
            "forbid_scope: {}\n",
            yaml_string_array(&self.forbid_scope)
        ));
        out.push_str(&format!("verify: {}\n", yaml_string_array(&self.verify)));
        out.push_str("---");
        out
    }
}

/// Renders `s` as a YAML double-quoted scalar by borrowing `serde_json`'s
/// string escaping — JSON string syntax is a valid subset of YAML's
/// double-quoted flow-scalar syntax (YAML 1.2: "JSON is a subset of
/// YAML"), so this reuses a well-tested escaper without running the
/// envelope itself through a YAML crate (the hand-rolled invariant
/// documented on [`Envelope::to_yaml_string`]). Always quotes — the
/// unconditional form used for flow-sequence elements ([`yaml_string_array`],
/// [`yaml_scope_override_array`]); top-level scalar fields go through
/// [`yaml_scalar_string`] instead, which quotes only when required.
fn yaml_quoted_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| {
        // Unreachable in practice — serializing a `&str` to JSON does not
        // fail — but a hand-escaped fallback keeps this panic-free per
        // the workspace no-panic lint set rather than reaching for
        // `.unwrap()`/`.expect()`.
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    })
}

/// True when `s` cannot be emitted as a bare YAML plain scalar without
/// changing its meaning or breaking the parse: empty, leading/trailing
/// whitespace, an embedded control character (`\n`, `\r`, and the rest of
/// the C0/C1 set — a bare plain scalar cannot carry one without splitting
/// the document across an extra line and corrupting the field order), a
/// reserved literal (`null`/`true`/`false`/`yes`/`no`/`on`/`off` in any
/// case — bare would parse as a non-string type), something that parses
/// as a number, a leading YAML indicator character, an embedded `": "` or
/// trailing `:` (mapping-key ambiguity), or an embedded `" #"` (starts a
/// comment mid-scalar). Conservative by design: quoting a string that
/// didn't strictly need it is only a style mismatch against the reference
/// format; leaving one bare that did need it is a correctness bug (a
/// mis-parsed envelope).
fn yaml_needs_quoting(s: &str) -> bool {
    if s.is_empty() || s.trim() != s {
        return true;
    }
    if s.chars().any(char::is_control) {
        return true;
    }
    let lower = s.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "null" | "~" | "true" | "false" | "yes" | "no" | "on" | "off"
    ) {
        return true;
    }
    if s.parse::<f64>().is_ok() {
        return true;
    }
    let starts_with_indicator = s
        .chars()
        .next()
        .is_some_and(|c| "-?:,[]{}#&*!|>'\"%@`".contains(c));
    if starts_with_indicator {
        return true;
    }
    s.contains(": ") || s.ends_with(':') || s.contains(" #")
}

/// Renders `s` as a bare YAML plain scalar when that's unambiguous
/// ([`yaml_needs_quoting`] is false), falling back to a double-quoted
/// scalar otherwise. This is the reference dispatch-envelope convention
/// this crate formalizes — `agent_id: Forge`, `repo: /abs/path` render
/// bare; only flow-sequence elements are quoted
/// (`touch_scope: ["libs/dispcli-core/**"]`, see [`yaml_string_array`]).
fn yaml_scalar_string(s: &str) -> String {
    if yaml_needs_quoting(s) {
        yaml_quoted_string(s)
    } else {
        s.to_string()
    }
}

/// Renders an `Option<i64>` as a bare YAML integer or the explicit `null`
/// token (AC4.1) — never omitted.
fn yaml_opt_int(n: Option<i64>) -> String {
    match n {
        Some(v) => v.to_string(),
        None => "null".to_string(),
    }
}

/// Renders an `Option<&str>` as a scalar (bare when safe, else
/// double-quoted — [`yaml_scalar_string`]) or the explicit `null` token
/// (AC4.1) — never omitted.
fn yaml_opt_string(s: Option<&str>) -> String {
    match s {
        Some(v) => yaml_scalar_string(v),
        None => "null".to_string(),
    }
}

/// Renders a string list as a YAML flow sequence — `[]` when empty
/// (AC4.1), `["a", "b"]` otherwise. Elements are always double-quoted
/// (unlike top-level scalars) — matches the reference envelope format's
/// `touch_scope: ["libs/dispcli-core/**"]` convention.
fn yaml_string_array(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_string();
    }
    let rendered: Vec<String> = items.iter().map(|s| yaml_quoted_string(s)).collect();
    format!("[{}]", rendered.join(", "))
}

/// Renders a [`ScopeOverride`] list as a YAML flow sequence of flow
/// mappings — `[]` when empty (AC4.1),
/// `[{capability: "...", reason: "..."}]` otherwise.
fn yaml_scope_override_array(items: &[ScopeOverride]) -> String {
    if items.is_empty() {
        return "[]".to_string();
    }
    let rendered: Vec<String> = items
        .iter()
        .map(|o| {
            format!(
                "{{capability: {}, reason: {}}}",
                yaml_quoted_string(&o.capability),
                yaml_quoted_string(&o.reason)
            )
        })
        .collect();
    format!("[{}]", rendered.join(", "))
}

// ============================================================================
// R8 — Output contract: summary
// ============================================================================

/// One entry in `size.components` (R8) — bytes attributed to one
/// assembled document section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentSize {
    pub section: String,
    pub bytes: u64,
}

/// `size` (R8) — component-by-component byte accounting. Components
/// summing to `total_bytes` (AC8.1) is enforced by the assembler in a
/// later task, not by this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SizeSummary {
    pub total_bytes: u64,
    pub components: Vec<ComponentSize>,
}

/// `worktree` (R8) — worktree requirement and the argv commands the
/// caller should run first. `commands` is always present and empty when
/// no worktree applies (AC8.3) — never null/omitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeSummary {
    pub required: bool,
    pub path: Option<String>,
    pub commands: Vec<Vec<String>>,
}

/// The JSON summary (R8) emitted to stdout on success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub document_path: String,
    pub agent: String,
    pub tier: Tier,
    pub weight: String,
    pub mode: PermissionMode,
    pub working_dir: String,
    pub worktree: WorktreeSummary,
    pub size: SizeSummary,
    pub verify_recipes: Vec<String>,
    pub warnings: Vec<String>,
}

// ============================================================================
// R5 — Prompt assembly (standard weight)
//
// Task 4 scope: the `standard` weight class only (`profile_sections =
// "all"`, `blocks = "all"` — R6). Light weight (fixed skill list, XML
// section extraction) is Task 8; a public `assemble` entry point that
// dispatches on `request.weight` between this function and a future
// `assemble_light` is that task's seam to add, not this one's.
//
// Also out of scope here (see the later tasks that own them): R7
// request/registry validation (Task 6/7), placeholder-hardening —
// AC5.2's unresolved-placeholder error and AC5.3's unsupported-brace
// warnings (Task 9), and the full `Summary`/byte-accounting output
// contract (Task 5/10). This module only proves the assembly *shape* —
// envelope-first, fixed body order, `include`-filtered blocks,
// best-effort placeholder substitution, and the Gap-4 byte accounting —
// against in-memory `ContentResolver` fakes (AC3.2).
// ============================================================================

/// The inter-section separator (Gap-4, resolved 2026-07-15): consecutive
/// assembled components — envelope, profile, each skill, each block, task
/// body — are joined by a single blank line. Separator bytes are
/// attributed to the *preceding* component in [`AssembledDocument::components`]
/// (never the following one), so components sum exactly to the document's
/// byte length (AC8.1). This is part of the output contract — adopters'
/// goldens depend on it — so it stays a private implementation constant
/// rather than a knob callers can vary.
const SECTION_SEPARATOR: &str = "\n\n";

/// One named, byte-accounted section of the assembled document body,
/// pre-join. Not `pub` — [`AssembledDocument`] is the public shape
/// Task 5 wires the CLI to; this is [`join_sections`]'s working unit.
struct Section {
    /// The `size.components[].section` name this content will be
    /// reported under (R8) — `"envelope"`, `"profile:{agent}"`,
    /// `"skill:{id}"`, `"block:{id}"`, or `"task_body"`. Composing these
    /// into the actual `Summary`/`SizeSummary` is Task 5/10's job; this
    /// module only produces the `(name, bytes)` pairs.
    name: String,
    content: String,
}

/// The result of [`assemble_standard`] — the joined document plus its
/// per-section byte accounting (Gap-4). `components` is not yet a
/// [`SizeSummary`] (no `total_bytes` field, no JSON emission) — building
/// the full R8 summary from this is Task 5/10's job; this is the
/// assembly-side seam that task wires the CLI to.
#[derive(Debug)]
pub struct AssembledDocument {
    pub document: String,
    pub components: Vec<ComponentSize>,
}

/// Assembles the standard-weight document body from `sections` in order,
/// joining consecutive components with [`SECTION_SEPARATOR`] and
/// attributing each separator's bytes to the *preceding* component
/// (Gap-4). The last component carries no trailing separator.
fn join_sections(sections: Vec<Section>) -> AssembledDocument {
    let mut document = String::new();
    let mut components = Vec::with_capacity(sections.len());
    let last_index = sections.len().saturating_sub(1);
    for (i, section) in sections.into_iter().enumerate() {
        document.push_str(&section.content);
        // Widening cast (usize -> u64): never lossy on any target this
        // workspace builds for.
        let mut bytes = section.content.len() as u64;
        if i != last_index {
            document.push_str(SECTION_SEPARATOR);
            bytes += SECTION_SEPARATOR.len() as u64;
        }
        components.push(ComponentSize {
            section: section.name,
            bytes,
        });
    }
    AssembledDocument {
        document,
        components,
    }
}

/// Resolves the agent's profile content (R5 step 1). Returns
/// `request_invalid` when `agent_id` has no registry entry — this
/// previews the AC1.1 "unknown agent" rejection Task 6 formalizes with
/// field-path detail; here it only guarantees assembly fails loudly
/// rather than panicking on a fixture Task 6 hasn't validated yet.
fn resolve_profile(
    agent_id: &str,
    registry: &Registry,
    resolver: &dyn ContentResolver,
) -> Result<String, Error> {
    let agent = registry.agents.get(agent_id).ok_or_else(|| {
        Error::new(
            ErrorKind::RequestInvalid,
            format!("unknown agent '{agent_id}'"),
        )
        .with_detail("field", "agent")
        .with_detail("value", agent_id)
    })?;
    resolver.resolve(agent_id, &agent.profile)
}

/// Resolves the pattern's skill contents in declared order (R5 step 2,
/// standard-weight slice: pattern order only — merging in `skills_add`
/// minus `skills_remove` with dedup-keeps-first is Gap-2, deferred to
/// Task 9). Returns `request_invalid` for an unknown `task_pattern`
/// (previews AC1.1, see [`resolve_profile`]) and `config_invalid` for a
/// pattern skill id with no registry entry (a dangling reference, AC2.2 —
/// registry-internal, not caller input).
fn resolve_skills(
    task_pattern: &str,
    registry: &Registry,
    resolver: &dyn ContentResolver,
) -> Result<Vec<(String, String)>, Error> {
    let pattern = registry.patterns.get(task_pattern).ok_or_else(|| {
        Error::new(
            ErrorKind::RequestInvalid,
            format!("unknown task_pattern '{task_pattern}'"),
        )
        .with_detail("field", "task_pattern")
        .with_detail("value", task_pattern)
    })?;
    let mut resolved = Vec::with_capacity(pattern.skills.len());
    for skill_id in &pattern.skills {
        let skill = registry.skills.get(skill_id).ok_or_else(|| {
            Error::new(
                ErrorKind::ConfigInvalid,
                format!("pattern '{task_pattern}' references unknown skill '{skill_id}'"),
            )
            .with_detail("pattern", task_pattern)
            .with_detail("skill", skill_id)
        })?;
        let content = resolver.resolve(skill_id, &skill.path)?;
        resolved.push((skill_id.clone(), content));
    }
    Ok(resolved)
}

/// Resolves template-block contents per registry `blocks.order`, filtered
/// by `include` (R5 step 3, R2 `include` rules): `always` unconditionally,
/// `worktree` only when `envelope.worktree` is non-null, `task` only when
/// `envelope.task_id` is non-null. Returns `config_invalid` for a
/// `blocks.order` entry with no matching `[blocks.<id>]` table (a
/// dangling reference, AC2.2).
fn resolve_blocks(
    registry: &Registry,
    envelope: &Envelope,
    resolver: &dyn ContentResolver,
) -> Result<Vec<(String, String)>, Error> {
    let mut resolved = Vec::new();
    for block_id in &registry.blocks.order {
        let block = registry.blocks.blocks.get(block_id).ok_or_else(|| {
            Error::new(
                ErrorKind::ConfigInvalid,
                format!("blocks.order references unknown block '{block_id}'"),
            )
            .with_detail("block", block_id)
        })?;
        let included = match block.include {
            Include::Always => true,
            Include::Worktree => envelope.worktree.is_some(),
            Include::Task => envelope.task_id.is_some(),
        };
        if included {
            let content = resolver.resolve(block_id, &block.path)?;
            resolved.push((block_id.clone(), content));
        }
    }
    Ok(resolved)
}

/// Applies best-effort supported-placeholder substitution (R5 placeholder
/// table) to skill/template-block content. Substitution is unconditional
/// text replacement — a placeholder not present in `content` is a no-op,
/// and (Task 4 scope) a placeholder left unsubstituted is not itself an
/// error here. AC5.2's "unresolved placeholder is an assembly error" and
/// AC5.3's "unsupported braces pass through + warn" are placeholder
/// *hardening*, deferred to Task 9 — this function only performs the
/// substitution the R5 table names.
fn substitute_placeholders(
    content: &str,
    request: &DispatchRequest,
    envelope: &Envelope,
) -> String {
    let mut out = content.replace("{dispatch_id}", &envelope.dispatch_id.to_string());
    if let Some(task_id) = envelope.task_id {
        out = out.replace("{task_id}", &task_id.to_string());
    }
    out = out.replace("{agent_name}", &request.agent);
    out = out.replace("{project_path}", &envelope.repo);
    if let Some(worktree) = envelope.worktree.as_deref() {
        out = out.replace("{worktree_path}", worktree);
    }
    out = out.replace("{branch}", &envelope.branch);
    out = out.replace("{report_path}", &envelope.report_path);
    out
}

/// Assembles a standard-weight dispatch document (R4 envelope + R5 body)
/// from `request` and `registry`, resolving profile/skill/block content
/// via `resolver` — no filesystem access in this crate (AC3.1/AC3.2).
///
/// Body order is exactly envelope → profile → skills → blocks → task body
/// (AC5.1); the envelope is the first block (AC4.4). Skills are the
/// pattern's declared order (standard-weight slice of R5 step 2 — see
/// [`resolve_skills`]); blocks are `blocks.order` filtered by `include`
/// (R5 step 3, see [`resolve_blocks`]); the task body is
/// `request.task_body` verbatim, never placeholder-substituted. Skill and
/// block content gets best-effort supported-placeholder substitution
/// (R5 placeholder table); the profile and task body do not.
///
/// # Errors
/// A [`RequestInvalid`](ErrorKind::RequestInvalid) [`Error`] for an
/// unknown `agent` or `task_pattern` (previews AC1.1 — full field-path
/// validation is Task 6); a [`ConfigInvalid`](ErrorKind::ConfigInvalid)
/// [`Error`] for a dangling registry reference (AC2.2 — full registry
/// validation is Task 7); whatever `resolver.resolve` returns
/// (typically [`ResolutionFailed`](ErrorKind::ResolutionFailed), AC3.3)
/// on a content-resolution failure.
pub fn assemble_standard(
    request: &DispatchRequest,
    registry: &Registry,
    resolver: &dyn ContentResolver,
) -> Result<AssembledDocument, Error> {
    let envelope = Envelope::from_request(request);

    let mut sections = vec![Section {
        name: "envelope".to_string(),
        content: envelope.to_yaml_string(),
    }];

    let profile_content = resolve_profile(&request.agent, registry, resolver)?;
    sections.push(Section {
        name: format!("profile:{}", request.agent),
        content: profile_content,
    });

    for (skill_id, content) in resolve_skills(&request.task_pattern, registry, resolver)? {
        let content = substitute_placeholders(&content, request, &envelope);
        sections.push(Section {
            name: format!("skill:{skill_id}"),
            content,
        });
    }

    for (block_id, content) in resolve_blocks(registry, &envelope, resolver)? {
        let content = substitute_placeholders(&content, request, &envelope);
        sections.push(Section {
            name: format!("block:{block_id}"),
            content,
        });
    }

    sections.push(Section {
        name: "task_body".to_string(),
        content: request.task_body.clone(),
    });

    Ok(join_sections(sections))
}
