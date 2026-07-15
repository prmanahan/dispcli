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
