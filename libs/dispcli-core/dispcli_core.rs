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
//! library error types.
//!
//! Task 6 scope: [`validate_request`] — the R1 AC1.1 + R7 request-side
//! validation stage, collecting every instance of the first failing
//! class with a field-path + offending-value [`Error::all_details`] pair
//! per instance. R2/AC2.1-2.3 registry self-consistency validation
//! (dangling references, closed `include` enum, resolvable paths) is
//! [`validate_registry`] (Task 7, below). See
//! `docs/specs/0001-envelope-assembly.md`.
//!
//! Task 7 scope: [`validate_registry`] — R2 AC2.2 registry
//! self-consistency validation, the registry-only sibling of
//! `validate_request`: every id a pattern, weight class, or
//! `blocks.order` entry references must be declared in the registry,
//! collected into one combined `config_invalid` error naming every
//! dangling reference (not source-by-source). AC2.3 (closed `include`
//! enum) and AC2.1 (resolvable paths) need no new code here — see
//! `validate_registry`'s own doc comment for why.

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
/// harness but is deliberately excluded here, so it always fails to
/// deserialize like any other unrecognized value; this type's job is only
/// to make `"plan"` unrepresentable. The dedicated "plan mode is not
/// dispatchable" message + structured field/value detail (R7) is produced
/// one layer up, by [`parse_request`]/[`parse_registry`] re-inspecting the
/// raw input on a parse failure (Task 6) — see
/// [`enrich_plan_mode_request_error`]/[`enrich_plan_mode_registry_error`].
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
/// scope (`command_scope_add` / `command_scope_subtract`, R1). Both fields
/// being *non-empty* (not just present) is an R7 rule enforced by
/// [`validate_request`] (Task 6) — this type only fixes the shape, so an
/// empty string still parses here and is caught one layer up.
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
/// `serde_json` error's `Display`, except for `mode_override: "plan"`
/// specifically, which gets the dedicated R7 message + structured
/// `field`/`value` detail (see [`enrich_plan_mode_request_error`]).
/// Field-path-aware validation for every other R1/R7 rule is
/// [`validate_request`] (Task 6) — this function only covers what
/// `serde_json`'s own parse enforces.
pub fn parse_request(input: &str) -> Result<DispatchRequest, Error> {
    serde_json::from_str(input).map_err(|err| enrich_plan_mode_request_error(input, err))
}

/// On a request-parse failure, re-inspects the raw JSON for
/// `mode_override == "plan"` (R7: `plan` is rejected with a dedicated
/// "plan mode is not dispatchable" message + field/value detail).
/// `mode_override` is typed `Option<PermissionMode>` — a closed 4-variant
/// enum with no `Plan` variant (deliberately; see [`PermissionMode`]'s doc
/// comment) — so `"plan"` can never survive into a live [`DispatchRequest`]
/// for [`validate_request`] to inspect post-parse. This function is the
/// only point in the pipeline that still has the raw offending string,
/// which is why the dedicated message is produced here rather than in
/// `validate_request`. Any other parse failure (including a request where
/// `mode_override` legitimately isn't `"plan"`) falls through to the
/// standard `serde_json`-message mapping, unchanged.
fn enrich_plan_mode_request_error(input: &str, err: serde_json::Error) -> Error {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(input)
        && value
            .get("mode_override")
            .and_then(serde_json::Value::as_str)
            == Some("plan")
    {
        return class_error(
            ErrorKind::RequestInvalid,
            "plan mode is not dispatchable",
            vec![("mode_override".to_string(), "plan".to_string())],
        );
    }
    Error::from(err)
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
/// underlying `toml` error's `Display`, except for `default_mode = "plan"`
/// specifically, which gets the dedicated R7 message + structured
/// `field`/`value` details (see [`enrich_plan_mode_registry_error`]).
pub fn parse_registry(input: &str) -> Result<Registry, Error> {
    toml::from_str(input).map_err(|err| enrich_plan_mode_registry_error(input, err))
}

/// On a registry-parse failure, re-inspects the raw TOML for any
/// `[agents.<id>]` table with `default_mode = "plan"` — same rationale as
/// [`enrich_plan_mode_request_error`] (`default_mode` is the same closed
/// [`PermissionMode`] enum, so `"plan"` can never survive into a live
/// [`AgentEntry`]). Collects **every** offending agent id (R7's
/// collect-all-in-class rule), field path `agents.<id>.default_mode`.
/// Any other parse failure, or a registry where no agent's `default_mode`
/// is `"plan"`, falls through to the standard mapping unchanged.
fn enrich_plan_mode_registry_error(input: &str, err: toml::de::Error) -> Error {
    if let Ok(value) = input.parse::<toml::Value>()
        && let Some(agents) = value.get("agents").and_then(toml::Value::as_table)
    {
        let offenders: Vec<(String, String)> = agents
            .iter()
            .filter(|(_, agent)| {
                agent.get("default_mode").and_then(toml::Value::as_str) == Some("plan")
            })
            .map(|(id, _)| (format!("agents.{id}.default_mode"), "plan".to_string()))
            .collect();
        if !offenders.is_empty() {
            return class_error(
                ErrorKind::ConfigInvalid,
                "plan mode is not dispatchable",
                offenders,
            );
        }
    }
    Error::from(err)
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
///
/// `message` is **display-only** — its exact wording is not a contract
/// and is not guaranteed stable across call sites that reject the same
/// underlying condition. E.g. [`validate_request`]'s AC1.1 rejection
/// ("request references one or more unknown registry ids") and
/// `resolve_profile`'s defense-in-depth copy of that same rejection
/// ("unknown agent '{id}'") word it differently while agreeing on `kind`
/// (`request_invalid`) and the `field`/`value` `details` pair. The
/// machine-readable contract a caller matches on is `kind` +
/// [`Error::detail`]/[`Error::all_details`] — never `message`. Divergent
/// wording between call sites is intentional, not a bug to reconcile.
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

    /// Every value recorded under `key`, in append order — the
    /// multi-instance counterpart to [`Error::detail`] (which returns only
    /// the first match). [`validate_request`]'s collect-all-in-class
    /// output (R7 preamble, AC7.2 — "first failure class reported with
    /// every instance of that class") repeats `"field"`/`"value"` detail
    /// pairs, one pair per violating instance; pair up `all_details("field")`
    /// and `all_details("value")` by position to recover every instance,
    /// not just the first.
    #[must_use]
    pub fn all_details(&self, key: &str) -> Vec<&str> {
        self.details
            .iter()
            .filter(|d| d.key == key)
            .map(|d| d.value.as_str())
            .collect()
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
    /// territory). The message is the raw `serde_json` `Display` — no
    /// structured field-path detail; that's [`validate_request`]'s job
    /// (Task 6) for the rules it covers, applied to an already-parsed
    /// request. This is strictly the fallback for shape/type failures
    /// `serde_json` itself catches during parsing (missing field, wrong
    /// type, unknown key) — [`parse_request`] special-cases exactly one
    /// of those (`mode_override: "plan"`) ahead of this mapping; see
    /// [`enrich_plan_mode_request_error`].
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
    /// `touch_scope`/`forbid_scope` entries get the R7 trailing-slash
    /// normalization (`path/` -> `path/**`) unconditionally, so the
    /// normalization is observable in the emitted envelope regardless of
    /// whether [`validate_request`] ran first (AC7.3).
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
            touch_scope: env
                .touch_scope
                .iter()
                .map(|p| normalize_scope_glob(p))
                .collect(),
            forbid_scope: env
                .forbid_scope
                .iter()
                .map(|p| normalize_scope_glob(p))
                .collect(),
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
// R1 + R7 — Request-side validation (Task 6)
//
// `validate_request` is the request-side validation stage the spec's
// pipeline diagram calls `[validate]` — it runs against an already-parsed
// `DispatchRequest`/`Registry` pair, before `assemble_standard` (R5) is
// ever called. Wiring it into `cmd/dispcli`'s pipeline ahead of
// `assemble_standard` is a later task's job (this crate exposes the
// function; it does not call itself, and never writes any output — "no
// partial output on failure" (AC1.1) falls out of the caller only
// proceeding on `Ok`). R2/AC2.1-2.3 registry self-consistency checks
// (dangling references, closed `include` enum, resolvable paths) are
// Task 7's job — this function only validates a request AGAINST a given
// registry's already-declared ids.
//
// Two R7 rules — the mode-value and tier-value closed enums — are
// enforced entirely at parse time, not here: `PermissionMode`/`Tier` are
// already closed enums (Task 1), so an invalid value (e.g. `"plan"`) can
// never survive into a live `DispatchRequest`/`AgentEntry` for this
// function to inspect — "define errors out of existence" (the type makes
// the invalid state unrepresentable; skills/rust.md `<design-heuristics>`).
// The dedicated "plan mode is not dispatchable" message and its
// structured field/value detail are produced by `parse_request`/
// `parse_registry` re-inspecting the raw input on a parse failure — see
// `enrich_plan_mode_request_error`/`enrich_plan_mode_registry_error` above.
// ============================================================================

/// True when `s` is exactly 40 lowercase hex characters (R7:
/// `parent_commit` / `spec_version` when non-null). Deliberately exact —
/// a 39- or 41-char value, or any uppercase hex digit, is rejected: "40
/// lower-hex" is not case-insensitive or length-tolerant.
fn is_valid_sha40(s: &str) -> bool {
    s.len() == 40 && s.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

/// True when `s` is an absolute path (R7: `repo` / `worktree` /
/// `report_path` when non-null). Pure syntactic check — `Path::is_absolute`
/// touches no filesystem, so this stays within the crate's IO-free
/// invariant (AC3.1).
fn is_absolute_path(s: &str) -> bool {
    std::path::Path::new(s).is_absolute()
}

/// R7 trailing-slash scope-glob normalization: `path/` becomes `path/**`;
/// anything else passes through unchanged. Shared by [`validate_request`]
/// (validates that the *normalized* form compiles as a glob) and
/// [`Envelope::from_request`] (AC7.3 — the normalization must be
/// observable in the emitted envelope, not merely accepted by validation).
fn normalize_scope_glob(pattern: &str) -> String {
    match pattern.strip_suffix('/') {
        Some(prefix) => format!("{prefix}/**"),
        None => pattern.to_string(),
    }
}

/// Shell metacharacters a `verify` entry may not contain (R7) — checked
/// after `just `-prefix stripping, before the whitespace split.
const VERIFY_SHELL_METACHARACTERS: [char; 11] =
    ['&', '|', ';', '>', '<', '`', '$', '(', ')', '\n', '\r'];

/// One parsed `verify` entry (R7): the recipe name plus any trailing args.
/// Not yet wired into [`Summary`]'s `verify_recipes` — that wiring is a
/// later task's job; this is the parsing seam it will call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedVerifyEntry {
    pub recipe: String,
    pub args: Vec<String>,
}

/// Parses one `verify` entry (R7): trims whitespace, strips a leading
/// `just ` token, rejects an empty result, rejects any entry containing a
/// [`VERIFY_SHELL_METACHARACTERS`] character, then whitespace-splits into
/// recipe + args — `"just check"` and `"check"` both parse to recipe
/// `"check"`. dispcli does not confirm the recipe exists (R7) — that
/// requires running the target project's tooling.
///
/// # Errors
/// A human-readable reason (empty-after-trim, or the metacharacter
/// rejection) — [`validate_request`] is the caller that wraps this into a
/// field-path-aware `request_invalid` [`Error`]; this function stays a
/// pure parser with no `Error` dependency of its own.
pub fn parse_verify_entry(raw: &str) -> Result<ParsedVerifyEntry, String> {
    let trimmed = raw.trim();
    let stripped = match trimmed.strip_prefix("just ") {
        Some(rest) => rest.trim_start(),
        None => trimmed,
    };
    if stripped.is_empty() {
        return Err(format!("verify entry '{raw}' is empty after trimming"));
    }
    if stripped
        .chars()
        .any(|c| VERIFY_SHELL_METACHARACTERS.contains(&c))
    {
        return Err(format!(
            "verify entry '{raw}' contains a disallowed shell metacharacter"
        ));
    }
    let mut parts = stripped.split_whitespace();
    let recipe = match parts.next() {
        Some(r) => r.to_string(),
        // Unreachable: `stripped` is non-empty (checked above), so
        // `split_whitespace` always yields at least one item. Matched
        // explicitly (rather than `.next().expect(...)`) to stay
        // unwrap/expect-free per the workspace no-panic lint set.
        None => return Err(format!("verify entry '{raw}' is empty after trimming")),
    };
    Ok(ParsedVerifyEntry {
        recipe,
        args: parts.map(str::to_string).collect(),
    })
}

/// Gap-3 (resolved 2026-07-15) — **tractable reading only, v0**: returns
/// one warning per normalized glob string present in *both* `touch_scope`
/// and `forbid_scope` (literal-duplicate detection after R7 trailing-slash
/// normalization). This is deliberately NOT general glob-intersection over
/// differing patterns (e.g. `libs/**` vs `libs/foo.rs` would NOT warn
/// here) — that is explicitly deferred to v1+ (spec Deferred section).
/// The overlap-warning *behavior itself* is marked **pending** the spec
/// author's ruling on literal-dup vs true glob-intersection — do not
/// extend this function toward intersection matching.
#[must_use]
pub fn scope_overlap_warnings(touch_scope: &[String], forbid_scope: &[String]) -> Vec<String> {
    let normalized_touch: std::collections::BTreeSet<String> = touch_scope
        .iter()
        .map(|p| normalize_scope_glob(p))
        .collect();
    forbid_scope
        .iter()
        .map(|p| normalize_scope_glob(p))
        .filter(|p| normalized_touch.contains(p))
        .map(|p| {
            format!(
                "scope pattern '{p}' appears in both touch_scope and forbid_scope \
                 (forbid wins downstream)"
            )
        })
        .collect()
}

/// Builds an [`Error`] of `kind` reporting every instance of one failing
/// validation class (R7 preamble / AC7.2: "first failure class reported
/// with every instance of that class"). Each `(field_path,
/// offending_value)` pair becomes a `"field"`/`"value"` detail pair, in
/// order — recover all of them via [`Error::all_details`], not just the
/// first (that's what [`Error::detail`] would give you).
fn class_error(
    kind: ErrorKind,
    message: impl Into<String>,
    instances: Vec<(String, String)>,
) -> Error {
    let mut err = Error::new(kind, message);
    for (field, value) in instances {
        err = err.with_detail("field", field).with_detail("value", value);
    }
    err
}

/// Validates `request` against `registry` (R1 AC1.1 unknown-id checks +
/// every R7 request-side rule). Runs each validation class in the order
/// below, returning the **first** class with any violation — but
/// reporting **every instance** within that class (R7's no-fail-fast-
/// within-a-class rule, AC7.2). A caller re-runs this after fixing the
/// reported class to discover the next one; it does not attempt to
/// surface every class in a single call.
///
/// Two R7 rules are deliberately absent from this function's body: the
/// mode-value and tier-value closed enums are enforced by the type system
/// at parse time (see the module-level note above this section) — there
/// is nothing left for this function to check for those two rules.
///
/// # Errors
/// A `request_invalid` [`Error`] from the first failing class, carrying
/// one `"field"`/`"value"` detail pair per violating instance
/// (recoverable via [`Error::all_details`]).
pub fn validate_request(request: &DispatchRequest, registry: &Registry) -> Result<(), Error> {
    // R1 AC1.1 — unknown agent / task_pattern / weight / skill id.
    let mut unknown_ids = Vec::new();
    if !registry.agents.contains_key(&request.agent) {
        unknown_ids.push(("agent".to_string(), request.agent.clone()));
    }
    if !registry.patterns.contains_key(&request.task_pattern) {
        unknown_ids.push(("task_pattern".to_string(), request.task_pattern.clone()));
    }
    if !registry.weights.contains_key(&request.weight) {
        unknown_ids.push(("weight".to_string(), request.weight.clone()));
    }
    for (i, skill_id) in request.skills_add.iter().enumerate() {
        if !registry.skills.contains_key(skill_id) {
            unknown_ids.push((format!("skills_add[{i}]"), skill_id.clone()));
        }
    }
    for (i, skill_id) in request.skills_remove.iter().enumerate() {
        if !registry.skills.contains_key(skill_id) {
            unknown_ids.push((format!("skills_remove[{i}]"), skill_id.clone()));
        }
    }
    if !unknown_ids.is_empty() {
        return Err(class_error(
            ErrorKind::RequestInvalid,
            "request references one or more unknown registry ids",
            unknown_ids,
        ));
    }

    // R7 — parent_commit / spec_version: 40-char lower-hex when non-null.
    let env = &request.envelope;
    let mut bad_shas = Vec::new();
    if !is_valid_sha40(&env.parent_commit) {
        bad_shas.push((
            "envelope.parent_commit".to_string(),
            env.parent_commit.clone(),
        ));
    }
    if let Some(spec_version) = &env.spec_version
        && !is_valid_sha40(spec_version)
    {
        bad_shas.push(("envelope.spec_version".to_string(), spec_version.clone()));
    }
    if !bad_shas.is_empty() {
        return Err(class_error(
            ErrorKind::RequestInvalid,
            "parent_commit/spec_version must be 40-char lowercase hex",
            bad_shas,
        ));
    }

    // R7 — repo / worktree / report_path: absolute when non-null. Checked
    // on the raw request fields (an `Option` left `None` is skipped, never
    // defaulted first) — the *defaulted* `Envelope.report_path` is always
    // absolute whenever `repo` is, which would make this rule untestable.
    let mut bad_paths = Vec::new();
    if !is_absolute_path(&env.repo) {
        bad_paths.push(("envelope.repo".to_string(), env.repo.clone()));
    }
    if let Some(worktree) = &env.worktree
        && !is_absolute_path(worktree)
    {
        bad_paths.push(("envelope.worktree".to_string(), worktree.clone()));
    }
    if let Some(report_path) = &env.report_path
        && !is_absolute_path(report_path)
    {
        bad_paths.push(("envelope.report_path".to_string(), report_path.clone()));
    }
    if !bad_paths.is_empty() {
        return Err(class_error(
            ErrorKind::RequestInvalid,
            "repo/worktree/report_path must be absolute paths",
            bad_paths,
        ));
    }

    // R7 — verify entries: each entry must parse per `parse_verify_entry`.
    let mut bad_verify = Vec::new();
    for (i, entry) in env.verify.iter().enumerate() {
        if parse_verify_entry(entry).is_err() {
            bad_verify.push((format!("envelope.verify[{i}]"), entry.clone()));
        }
    }
    if !bad_verify.is_empty() {
        return Err(class_error(
            ErrorKind::RequestInvalid,
            "one or more verify entries failed to parse",
            bad_verify,
        ));
    }

    // R7 — command_scope_subtract / command_scope_add: capability + reason
    // both required and non-empty. One combined class across both arrays
    // (a bad entry in either reports alongside the other).
    let mut bad_overrides = Vec::new();
    for (list_name, overrides) in [
        ("command_scope_subtract", &env.command_scope_subtract),
        ("command_scope_add", &env.command_scope_add),
    ] {
        for (i, entry) in overrides.iter().enumerate() {
            if entry.capability.trim().is_empty() {
                bad_overrides.push((
                    format!("envelope.{list_name}[{i}].capability"),
                    entry.capability.clone(),
                ));
            }
            if entry.reason.trim().is_empty() {
                bad_overrides.push((
                    format!("envelope.{list_name}[{i}].reason"),
                    entry.reason.clone(),
                ));
            }
        }
    }
    if !bad_overrides.is_empty() {
        return Err(class_error(
            ErrorKind::RequestInvalid,
            "one or more command_scope entries are missing capability or reason",
            bad_overrides,
        ));
    }

    // R7 — touch_scope / forbid_scope: each entry compiles as a glob (the
    // *normalized* form — AC7.3 trailing-slash normalization). One
    // combined class across both arrays.
    let mut bad_globs = Vec::new();
    for (list_name, patterns) in [
        ("touch_scope", &env.touch_scope),
        ("forbid_scope", &env.forbid_scope),
    ] {
        for (i, pattern) in patterns.iter().enumerate() {
            let normalized = normalize_scope_glob(pattern);
            if globset::Glob::new(&normalized).is_err() {
                bad_globs.push((format!("envelope.{list_name}[{i}]"), pattern.clone()));
            }
        }
    }
    if !bad_globs.is_empty() {
        return Err(class_error(
            ErrorKind::RequestInvalid,
            "one or more touch_scope/forbid_scope entries do not compile as a glob",
            bad_globs,
        ));
    }

    Ok(())
}

// ============================================================================
// R2 AC2.2 — Registry self-consistency validation (Task 7)
//
// `validate_registry` is a standalone pre-flight check, the registry-only
// sibling of `validate_request` (Task 6) — same collect-all-in-class
// `class_error` idiom, same relationship to the pipeline: wiring it into
// `cmd/dispcli` ahead of `assemble_standard` is a later task's job (this
// crate exposes the function; it does not call itself). `resolve_skills`/
// `resolve_blocks` (Task 4, R5) already carry their own ad hoc dangling-
// reference guards as assembly-time defense-in-depth, with their own
// pre-existing detail-key conventions (`"pattern"`/`"skill"`, `"block"`)
// — left untouched here; `validate_registry` is the new authoritative
// pre-flight check with its own `"field"`/`"value"` convention, matching
// `validate_request`'s. Divergent detail keys between call sites for the
// same underlying condition is the same intentional non-reconciliation
// the `Error` doc comment already covers for `message` wording.
// ============================================================================

/// Validates `registry` for self-consistency (R2 AC2.2): every id
/// referenced by a `[patterns.<id>]`'s `skills`, a `[weights.<id>]`'s
/// `skills` or `blocks` (list form), or `blocks.order` must be declared
/// in the registry. One combined class across all four reference sources
/// — the same "spans multiple sources, one semantic rule" treatment
/// [`validate_request`] gives its unknown-id class (agent/task_pattern/
/// weight/skills_add/skills_remove all collected together, Task 6):
/// every dangling reference across the whole registry is collected and
/// reported together in one call, not source-by-source.
///
/// The weight-class `"all"` sentinel ([`AllOrList::All`]) is a closed
/// vocabulary *value* (R1 "open vs. closed vocabularies" note, R2),
/// never an id to resolve — only [`AllOrList::List`]'s entries are
/// checked against declared ids. A weight class's `profile_sections` is
/// never checked here: its entries are profile-internal XML tag names
/// (R6), not registry-declared ids — matching a tag against the profile
/// is AC6.1's job, and needs the resolved profile content this IO-free
/// function never has.
///
/// Two other R2 acceptance criteria are deliberately absent from this
/// function's body:
/// - **AC2.3** (closed `include` enum) needs no runtime check: `Include`
///   is already a closed 3-variant enum (Task 1) enforced by
///   [`parse_registry`] at parse time — an invalid value can never
///   survive into a live [`Registry`] for this function to inspect, the
///   same "define errors out of existence" treatment `Tier`/
///   `PermissionMode` get in [`validate_request`].
/// - **AC2.1** (an unresolvable file path) is not a self-consistency
///   property of the registry alone — confirming a path resolves needs
///   an actual read, which this IO-free function never performs. It's
///   enforced by [`ContentResolver::resolve`] (native: `dispcli-io`'s
///   `FsContentResolver`, Task 3) at assembly time.
///
/// # Errors
/// A `config_invalid` [`Error`] carrying one `"field"`/`"value"` detail
/// pair per dangling reference, collected in patterns → `blocks.order`
/// → weights order (recoverable via [`Error::all_details`]).
pub fn validate_registry(registry: &Registry) -> Result<(), Error> {
    let mut dangling = Vec::new();

    for (pattern_id, pattern) in &registry.patterns {
        for (i, skill_id) in pattern.skills.iter().enumerate() {
            if !registry.skills.contains_key(skill_id) {
                dangling.push((
                    format!("patterns.{pattern_id}.skills[{i}]"),
                    skill_id.clone(),
                ));
            }
        }
    }

    for (i, block_id) in registry.blocks.order.iter().enumerate() {
        if !registry.blocks.blocks.contains_key(block_id) {
            dangling.push((format!("blocks.order[{i}]"), block_id.clone()));
        }
    }

    for (weight_id, weight) in &registry.weights {
        if let Some(skills) = &weight.skills {
            for (i, skill_id) in skills.iter().enumerate() {
                if !registry.skills.contains_key(skill_id) {
                    dangling.push((format!("weights.{weight_id}.skills[{i}]"), skill_id.clone()));
                }
            }
        }
        if let AllOrList::List(block_ids) = &weight.blocks {
            for (i, block_id) in block_ids.iter().enumerate() {
                if !registry.blocks.blocks.contains_key(block_id) {
                    dangling.push((format!("weights.{weight_id}.blocks[{i}]"), block_id.clone()));
                }
            }
        }
    }

    if !dangling.is_empty() {
        return Err(class_error(
            ErrorKind::ConfigInvalid,
            "registry references one or more undeclared ids",
            dangling,
        ));
    }

    Ok(())
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
/// `request_invalid` when `agent_id` has no registry entry — the same
/// AC1.1 "unknown agent" rejection [`validate_request`] (Task 6) reports
/// earlier in the pipeline, with the same `field`/`value` detail shape.
/// This defense-in-depth copy guarantees assembly still fails loudly
/// (rather than panicking) when `assemble_standard` is called directly,
/// without `validate_request` having run first.
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
/// unknown `agent` or `task_pattern` (AC1.1 — same rejection
/// [`validate_request`] (Task 6) reports earlier in the pipeline, with
/// the same field/value detail shape); a
/// [`ConfigInvalid`](ErrorKind::ConfigInvalid) [`Error`] for a dangling
/// registry reference (AC2.2 — full registry self-consistency validation
/// is a separate, later task); whatever `resolver.resolve` returns
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
