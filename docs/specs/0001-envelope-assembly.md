# Spec 0001 — Envelope assembly

| | |
|---|---|
| **Status** | Accepted — 2026-07-12 · Amended (gap resolutions, R2/R5/R6/R7/R8) — 2026-07-15 · Amended (AC5.2 scope, R7 `branch` rule, R5 fixed-skills clarity pin) — 2026-07-17 · Amended (AC6.3 weight-class block reachability) — 2026-07-18 |
| **Scope** | v0: dispatch-request input schema, registry config, resolver traits, envelope + prompt construction, CLI output contract |
| **Out of scope** | Worktree execution, cost-metric emission, post-dispatch verification, WASM/plugin frontend (all v1+) |

## Overview

`dispcli assemble` turns a **dispatch request** (what the orchestrator wants
to run) plus a **registry** (what agents, skills, and template blocks exist)
into a **dispatch document**: a single file containing a YAML envelope header
followed by the fully composed agent prompt, ready for the orchestrator to
fire verbatim. A JSON summary on stdout tells the caller everything it needs
to act — where the document landed, what permission mode to use, what
working directory applies, and what worktree commands to run first.

dispcli decides nothing that requires judgment. Tier/model selection,
agent choice, scope derivation, and skill overrides are caller inputs;
dispcli validates, resolves, assembles, and reports. Judgment stays with
the orchestrator.

```
dispatch request (JSON)──┐
                         ├──> [validate] -> [resolve via traits] -> [assemble]
registry (TOML) ─────────┘                                             │
                                              dispatch document (file) ┘
                                              + JSON summary (stdout)
```

Crate placement follows the repo invariant: all types, validation, and
assembly logic live in `libs/dispcli-core` (IO-free); filesystem resolution
and file writes live in `libs/dispcli-io`; `cmd/dispcli` parses args and
wires the two.

---

## R1 — Dispatch request schema

The CLI accepts a JSON dispatch request from a file path or stdin (`-`).

```jsonc
{
  "agent": "implementer",            // registry agent id — required
  "task_pattern": "implementation",  // registry pattern id — required
  "tier": "t2",                      // caller judgment, recorded not derived — required
  "weight": "standard",              // registry weight-class id; default "standard"
  "mode_override": null,             // permission mode; null = agent's registry default
  "skills_add": [],                  // skill ids beyond the pattern mapping
  "skills_remove": [],               // skill ids to drop from the pattern mapping
  "task_body": "…markdown…",         // the task description — required, non-empty

  "envelope": {                      // facts for the YAML header — see R4
    "dispatch_id": 42,               // required
    "task_id": 7,                    // int or null
    "spec_id": "docs/specs/0001-envelope-assembly.md",  // string or null
    "spec_version": "<40-hex sha or null>",
    "parent_commit": "<40-hex sha>", // required
    "repo": "/abs/path/to/repo",     // required, absolute
    "worktree": "/abs/path or null",
    "branch": "feature-x",           // required
    "report_path": null,             // null = derived default, see R4
    "deadline_minutes": null,
    "command_scope_subtract": [],    // [{"capability": "...", "reason": "..."}]
    "command_scope_add": [],         // same shape; reason required on every entry
    "touch_scope": [],               // glob patterns
    "forbid_scope": [],              // glob patterns
    "verify": []                     // recipe-name entries — validated per R7
  }
}
```

**Open vs. closed vocabularies.** `agent`, `task_pattern`, `weight`, skill
ids, and block ids are *open*: they are registry keys with no compiled-in
meaning — the code's only behavior is the registry lookup, and an unknown
value is an error naming it. Renaming or adding one is a registry edit,
never a code change. The *closed* vocabularies — compiled enums the code
branches on — are exactly: `tier` (`t1` | `t2` | `t3`), permission modes
(R7), block `include` conditions (R2), and the `"all"` sentinels in weight
classes (R2).

**Acceptance criteria**

- AC1.1 — A request missing a required field, or with an unknown `agent`,
  `task_pattern`, `weight`, or skill id, is rejected with an error naming
  the field and the offending value. No partial output is produced.
- AC1.2 — Unknown top-level or `envelope` keys are rejected (deny-unknown
  parse), so a typo'd field cannot silently no-op.
- AC1.3 — The request parses identically from a file path and from stdin.

## R2 — Registry config

The registry is a TOML file describing the orchestrator's inventory. It is
the portability boundary: adopters describe their own agents/skills here;
nothing in dispcli hardcodes a particular team.

```toml
[registry]
skills_root = "skills"            # paths below resolve relative to this file's directory

[agents.implementer]
profile = "team/implementer.md"
default_mode = "bypassPermissions"   # permission mode the orchestrator should use
worktree_required = true             # whether this agent's work needs an isolated worktree

[agents.researcher]
profile = "team/researcher.md"
default_mode = "default"
worktree_required = false

[skills.rust]
path = "skills/rust.md"

[skills.verify]
path = "skills/verify.md"

[patterns.implementation]
skills = ["verify", "rust", "tdd"]   # ordered; order is preserved in assembly

[blocks]                              # template blocks, in assembly order (R5)
order = ["metrics", "completion-report", "merge-msg", "task-tracking", "working-dir", "scope-boundaries"]

[blocks.metrics]
path = "skills/dispatch-metrics.md"
include = "always"

[blocks.merge-msg]
path = "skills/dispatch-merge-msg.md"
include = "worktree"                  # only when the dispatch uses a worktree

[blocks.task-tracking]
path = "skills/dispatch-task-tracking.md"
include = "task"                      # only when envelope.task_id is non-null

[weights.standard]
profile_sections = "all"
blocks = "all"

[weights.light]
profile_sections = ["role", "persona", "command-scope"]  # XML-tagged sections to extract
skills = ["verify"]                   # fixed floor: replaces the pattern's skills array only; skills_add/remove still apply (R5 step 2)
blocks = ["metrics", "working-dir", "scope-boundaries"]
```

**Acceptance criteria**

- AC2.1 — A registry referencing a file that cannot be resolved fails at
  assembly time with an error naming the registry key and the path.
- AC2.2 — Every id referenced by a pattern, weight class, or block `order`
  entry must be declared in the registry; dangling references are a
  config error reported with the referencing and missing ids.
- AC2.3 — `include` values are a closed enum (`always`, `worktree`,
  `task`); anything else is a config error.
- AC2.4 — Registry parsing lives in `dispcli-core` operating on a string;
  only the file read is in `dispcli-io`.
- AC2.5 — **`skills_root` is reserved in v0** (gap resolution 2026-07-15):
  all path resolution is relative to the registry file's directory (R3);
  `skills_root` is **not** applied as a prefix. Adopters set each entry's
  `path` relative to the registry dir. Reserved for a future rooting
  convention (see Deferred).

## R3 — Resolver traits (IO boundary)

`dispcli-core` defines the IO boundary as traits; it never touches the
filesystem.

- `ContentResolver` — given a registry-declared path (profile, skill,
  template block), return its content as a string. The native
  implementation in `dispcli-io` reads from the filesystem rooted at the
  registry file's directory. The future WASM host implements the same
  trait over host functions.
- `DocumentSink` — given the output path and the assembled document,
  persist it. Native implementation writes the file (creating parent
  directories); summary emission stays in `cmd/dispcli`.

**Acceptance criteria**

- AC3.1 — `dispcli-core` compiles with no `std::fs`, `std::process`, or
  stdio usage (existing CI lint surface enforces; the spec makes it a
  requirement, not a convention).
- AC3.2 — Core assembly is testable with in-memory resolver/sink fakes —
  the core integration tests exercise full assembly without touching disk.
- AC3.3 — Resolution failures carry the registry id, the resolved path,
  and the underlying cause; core maps them into the error taxonomy (R8).

## R4 — Envelope construction

The document's first block is a YAML frontmatter envelope built from
`request.envelope`. Schema (envelope v1):

```yaml
---
dispatch_id: <int>
task_id: <int | null>
agent_id: <string>               # request.agent, verbatim
spec_id: <string | null>
spec_version: <string | null>
parent_commit: <40-hex sha>
repo: <absolute path>
worktree: <absolute path | null>
branch: <string>
report_path: <absolute path>
deadline_minutes: <int | null>
command_scope_subtract: []
command_scope_add: []
touch_scope: []
forbid_scope: []
verify: []
---
```

**Acceptance criteria**

- AC4.1 — Every schema key is always emitted; absent optional values are
  emitted as explicit `null`, never omitted. Consumers rely on schema
  stability.
- AC4.2 — When `report_path` is null in the request, it defaults to
  `{worktree or repo}/scratch/dispatch-{dispatch_id}-report.md`.
- AC4.3 — Field order matches the schema above byte-for-byte modulo
  values, so envelope diffs between dispatches are line-stable.
- AC4.4 — The envelope is the first block of the document, before any
  profile or skill content (machine-parseable without scanning prose).

## R5 — Prompt assembly order and placeholder substitution

The document body after the envelope is assembled in fixed order:

1. Agent profile (sections per weight class — R6)
2. Skills — the pattern's `skills` array in its declared order, then
   `skills_add` in array order, minus `skills_remove`; a skill appearing
   more than once is emitted **once at its first occurrence** (dedup keeps
   first position) — gap resolution 2026-07-15, pinning "in registry order".
   A weight class with a fixed `skills` list bypasses **the pattern mapping
   only**: its list replaces the pattern's `skills` array (the first term
   above), and then `skills_add` and `skills_remove` still apply on top, same
   dedup-keeps-first. The fixed list is a **floor** (a tunable starting set),
   not a cap — a `light` dispatch can still be extended via `skills_add`.
   Clarity pin (2026-07-17): the wording already supported this narrow
   reading; the pin makes it explicit rather than inferred from "the pattern
   mapping" naming the first of the three terms.
3. Template blocks — registry `blocks.order`, filtered by `include` rules
4. Task body, verbatim

**Section joining (gap resolution 2026-07-15).** Consecutive assembled
components (envelope, profile, each skill, each block, task body) are joined
by a single blank line (`\n\n`). Each separator's bytes are attributed to
the **preceding** component in the size accounting (R8), so components sum
exactly to the document byte length. The `\n\n` joining is part of the
output contract — adopters' goldens depend on it.

Placeholder substitution applies to skill and template-block content (not
the profile, not the task body). Supported placeholders:

| Placeholder | Source |
|---|---|
| `{dispatch_id}` | envelope |
| `{task_id}` | envelope (substituted only when non-null) |
| `{agent_name}` | request `agent` |
| `{project_path}` | envelope `repo` |
| `{worktree_path}` | envelope `worktree` (substituted only when non-null) |
| `{branch}` | envelope `branch` |
| `{report_path}` | envelope `report_path` (post-default) |

**Acceptance criteria**

- AC5.1 — Output section order is exactly envelope → profile → skills →
  blocks → task body for every weight class.
- AC5.2 — Any `{placeholder}` from the supported set remaining unsubstituted
  in a substituted section — skills or template blocks, per R5's substitution
  scope (e.g. `{worktree_path}` used by an included block in a non-worktree
  dispatch) — is an assembly error, not a warning. A supported placeholder
  left in the profile or task body, which R5 passes through verbatim, is
  **not** an error and **not** an AC5.3 warning (AC5.3 fires only on
  unsupported tokens); this silent passthrough is an accepted v0 hole.
- AC5.3 — Brace tokens outside the supported set are passed through
  untouched (skill content legitimately contains braces) and listed in
  the summary's `warnings` array for operator review.
- AC5.4 — A skill appearing via both the pattern mapping and `skills_add`
  is included once; `skills_remove` of a skill not present is a request
  error (AC1.1 naming rule applies).

## R6 — Weight classes

Weight classes scale prompt mass to task size without changing assembly
order. The registry defines them (R2); the request selects one.

- `profile_sections = "all"` includes the profile verbatim.
- A section list (e.g. `["role", "persona", "command-scope"]`) extracts
  only those XML-tagged top-level sections (`<role>…</role>` etc.) from
  the profile, in profile order.
- `blocks = "all"` applies the registry `include` rules; a block list
  intersects with those rules (a listed block still respects `worktree`/
  `task` conditions). Every id in a block list must also appear in
  `blocks.order` (amendment 2026-07-18): `blocks.order` is the sole source
  of iteration order, so a listed-but-unordered block is unreachable —
  silently dropped rather than assembled.

**Acceptance criteria**

- AC6.1 — Section extraction matches top-level XML tags only; a tag named
  in the weight class but absent from the profile is an assembly error
  naming agent and tag. **Extraction is span-based** (gap resolution
  2026-07-15): "top-level" means outermost nesting depth, and sections are
  matched as the outermost `<tag>…</tag>` spans by line/position — profiles
  are markdown-with-XML, not well-formed single-root XML, so an XML parser
  MUST NOT be used.
- AC6.2 — The summary reports which weight class applied and the resulting
  component sizes (R8), so the operator can confirm a light dispatch
  actually came out light.
- AC6.3 — A block id named in a weight class's `blocks` list but absent
  from `blocks.order` is a config error naming the weight class and the
  unreachable id. **Declaration is not sufficient** (amendment
  2026-07-18): a `[blocks.<id>]` table satisfies AC2.2's declaration
  requirement while still being unreachable if `order` omits it, so AC2.2
  does not cover this case. Mirrors AC6.1's treatment of a
  `profile_sections` tag named in the weight class but absent from the
  profile — both are weight-class references to something unreachable,
  and both fail loudly rather than silently.

## R7 — Validation rules

All validation happens before any output is written; first failure class
reported with every instance of that class (no fail-fast-on-first-error
within a class).

| Rule | Detail |
|---|---|
| `parent_commit`, `spec_version` | 40-char lower-hex when non-null. Short SHAs rejected — truncated identity invites reconstruction errors. |
| `repo`, `worktree`, `report_path` | Absolute paths when non-null. |
| `branch` | Required, non-empty, and a valid git ref name (`git check-ref-format` semantics): no control characters or spaces, no `..`, no leading `-`, no trailing `.lock`, and none of `~ ^ : ? * [`. `branch` reaches skill/block content verbatim via `{branch}` substitution — downstream of the parse boundary, with no escaping layer — so it is constrained at validation, not escaped at emission. |
| `verify` entries | Trim whitespace; strip a leading `just ` token; reject empty results; reject any entry containing shell metacharacters (`& \| ; > < \` $ ( ) \n \r`); whitespace-split into recipe + args. dispcli does not confirm the recipe exists (that requires running the target project's tooling) — the summary carries the parsed recipe names so the caller can. |
| `command_scope_subtract` / `_add` entries | Both `capability` and `reason` required and non-empty. No reason to give means the override should not exist. |
| Scope globs | Each `touch_scope` / `forbid_scope` entry must compile as a glob pattern. A trailing-slash entry (`path/`) is normalized to `path/**` before emission, mirroring downstream enforcement semantics. **Overlap detection (v0, gap resolution 2026-07-15):** an *identical normalized pattern string* present in both `touch_scope` and `forbid_scope` emits a warning (forbid wins downstream); general glob-intersection over differing patterns is deferred to v1+. |
| Mode values | `mode_override` and registry `default_mode` are a closed enum: `default`, `acceptEdits`, `bypassPermissions`, `dontAsk`. `plan` is rejected with an error stating plan mode is not dispatchable. |
| `tier` | Closed enum: `t1`, `t2`, `t3`. Recorded and echoed in the summary; never branches assembly behavior in v0 (reserved for v1 metrics emission). |

**Acceptance criteria**

- AC7.1 — Each rule above has at least one rejection test and one
  acceptance test at the boundary (e.g. 39- and 41-char SHAs, `just check`
  vs `check`, glob `path/` → `path/**`, an invalid vs. valid `branch`).
- AC7.2 — Validation errors identify the field path
  (`envelope.verify[1]`) and the offending value.
- AC7.3 — Trailing-slash normalization is observable in the emitted
  envelope (the document contains `path/**`, not `path/`).

## R8 — Output contract and error taxonomy

On success the document is written via `DocumentSink` and a single JSON
object goes to stdout:

```jsonc
{
  "document_path": "/abs/path/to/output.md",
  "agent": "implementer",
  "tier": "t2",
  "weight": "standard",
  "mode": "bypassPermissions",        // override if given, else registry default
  "working_dir": "/abs/worktree-or-repo",
  "worktree": {
    "required": true,                  // registry flag, or false when envelope.worktree is null
    "path": "/abs/worktree-path",
    "commands": [                      // argv arrays, no shell strings — empty when not required
      ["git", "-C", "/abs/repo", "worktree", "add", "/abs/worktree-path", "-b", "feature-x"]
    ]
  },
  "size": {
    "total_bytes": 18432,
    "components": [                    // one entry per assembled section, in order
      {"section": "envelope", "bytes": 812},
      {"section": "profile:implementer", "bytes": 3604},
      {"section": "skill:verify", "bytes": 3998}
    ]
  },
  "verify_recipes": ["check"],         // parsed recipe names for caller-side existence checks
  "warnings": []
}
```

Errors print a JSON object to stderr — `{"error": {"kind", "message", "details": [...]}}`
— and nothing to stdout.

| `kind` | Exit code | Meaning |
|---|---|---|
| `usage` | 2 | bad flags/arguments |
| `request_invalid` | 3 | R1/R7 request-side failures |
| `config_invalid` | 4 | R2 registry failures |
| `resolution_failed` | 5 | R3 content resolution failures |
| `assembly_failed` | 6 | R5/R6 failures (unresolved placeholder, missing section) |
| `io_failed` | 7 | sink write failures |

**Acceptance criteria**

- AC8.1 — Success emits exactly one JSON object on stdout and exits 0;
  size components sum to `total_bytes` and match the written document's
  byte length. Inter-section separator (`\n\n`, R5) bytes are attributed to
  the preceding component, so the sum is exact.
- AC8.2 — Every error kind maps to its exit code; stdout stays empty on
  any failure (machine callers can trust stdout = success payload).
- AC8.3 — `worktree.commands` is present and empty (not null/omitted)
  when no worktree applies; command entries are argv arrays, never shell
  strings.
- AC8.4 — Production paths return structured errors; no panics (existing
  lint discipline enforces; spec makes it acceptance-tested via malformed
  inputs).

## R9 — CLI surface

```
dispcli assemble --request <path|-> [--config <path>] [--out <path>]
```

- `--request` — dispatch request JSON; `-` reads stdin. Required.
- `--config` — registry TOML. Default: `$DISPCLI_CONFIG` if set, else
  `dispcli.toml` in the current directory.
- `--out` — document output path. Default:
  `{working_dir}/scratch/dispatch-{dispatch_id}-prompt.md`.
- `dispcli --version` continues to report the core version (existing
  scaffold behavior).

**Acceptance criteria**

- AC9.1 — `dispcli assemble` end-to-end integration test: fixture
  registry + request in, document + summary out, content equality against
  a golden file.
- AC9.2 — Config precedence is flag > env > default, covered by tests.
- AC9.3 — `--help` documents every flag and the exit-code table.

## Test expectations

- Core (`dispcli-core`): unit + integration tests over in-memory fakes —
  assembly order, weight-class extraction, placeholder substitution,
  every R7 rule at its boundary, error taxonomy mapping. No filesystem.
- IO (`dispcli-io`): resolver tests against fixture directories (missing
  file, relative-root resolution); sink tests for parent-directory
  creation.
- Binary (`cmd/dispcli`): golden-file end-to-end (AC9.1), stdin request,
  exit-code coverage per error kind, stdout-purity check on failures.

## Deferred (v1+)

Worktree execution (v0 emits the commands, never runs them), cost-metric
emission, post-dispatch git verification, recipe-existence checking
against the target project, profile/skill content linting, general
glob-intersection scope-overlap detection (v0 warns only on
identical-pattern duplicates — R7), a `skills_root` rooting convention
(reserved — R2/AC2.5), path-resolution subtree confinement (`..`/symlink
escape blocking via canonicalize-then-verify — deferred under the v0
trusted-registry trust model per R3; revisit if the registry becomes an
untrusted / multi-tenant input; surfaced by the Warden Task 3 review
2026-07-15), and the WASM Component Model frontend. The `ContentResolver`/`DocumentSink` traits are
the seam the WASM port re-implements; nothing in this spec may assume a
filesystem beyond `dispcli-io`.
