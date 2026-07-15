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
//! those land in later tasks. See `docs/specs/0001-envelope-assembly.md`.

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
/// Returns the underlying `serde_json` error on malformed JSON, a
/// missing required field, an unknown top-level or `envelope` key
/// (`deny_unknown_fields`), or a value outside a closed enum (`tier`,
/// `mode_override`). Mapping this into the R8 error taxonomy is a later
/// task.
pub fn parse_request(input: &str) -> Result<DispatchRequest, serde_json::Error> {
    serde_json::from_str(input)
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
/// Returns the underlying `toml` error on malformed TOML, a missing
/// required field, or a value outside a closed enum (`default_mode`,
/// `include`).
pub fn parse_registry(input: &str) -> Result<Registry, toml::de::Error> {
    toml::from_str(input)
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
