# dispcli — contributor notes

Auto-loaded at session start. Codifies the conventions an agent or human
contributor must follow when changing this repo.

## Architecture

Three crates, strict separation:

| Crate | Role | IO |
|---|---|---|
| `libs/dispcli-core` | Dispatch-envelope construction. Types, traits, assembly logic. | **IO-free.** No `std::fs`, `std::process`, stdin/stdout. |
| `libs/dispcli-io` | Native IO adapters implementing core traits (filesystem skill resolution, envelope writes, etc.). | Native only. |
| `cmd/dispcli` | Binary. Parses args, wires `dispcli-io` adapters into `dispcli-core` logic, emits output. | Thin. |

**The IO-free invariant on `dispcli-core` is load-bearing.** The eventual
mnemra plugin frontend is a WebAssembly Component Model module — it
re-implements the IO adapters via WIT host functions, leaving `dispcli-core`
untouched. Any `std::fs` / `std::process` / stdin call inside `dispcli-core`
breaks the WASM port. Use a trait + caller-provided implementation instead.

## Repository layout

As-built tree:

```
cmd/dispcli/            # the binary
├── main.rs
└── tests/integration.rs
libs/dispcli-core/       # IO-free dispatch-envelope logic
├── dispcli_core.rs
└── tests/integration.rs
libs/dispcli-io/         # native IO adapters (implements dispcli-core traits)
├── dispcli_io.rs
└── tests/integration.rs
docs/specs/               # feature specs — README.md + NNNN-slug.md
docs/src/                 # mdBook source (intro.md, SUMMARY.md, adrs/;
                           # mermaid + d2 preprocessors)
Cargo.toml                # workspace manifest
justfile, clippy.toml, deny.toml, .github/
```

`docs/specs/` holds the authoritative feature specs; `docs/src/` is the
mdBook that renders them for contributors.

**Current status:** scaffold + spec-driven. Spec 0001 (envelope assembly)
defines the first feature; implementation has not started yet — don't go
looking for code that isn't there.

## Layout conventions

- **No `src/` directories.** Workspace layout:
  - `cmd/{name}/main.rs` — project binaries (the things you ship)
  - `libs/{crate}/{crate_name}.rs` — project libraries
  - `tools/{name}/main.rs` — optional location for build-time / dev tools (none present today; convention reserved)
- Workspace members listed in the root `Cargo.toml`. Adding a crate means
  editing that list, creating the directory with `Cargo.toml` + entrypoint,
  and confirming `[lints] workspace = true` is set on the new member (see
  carve-out for `tools/` below).

## Lint discipline

All clippy lints are declared in `[workspace.lints.clippy]` in the root
`Cargo.toml`. Every crate in `cmd/` or `libs/` **must** include
`[lints] workspace = true` or it silently misses the strict set. Crates in
`tools/` **may** omit it when their source uses pragmatic patterns that
don't satisfy the strict set — the omission must be documented with a
comment in the crate's `Cargo.toml` explaining why.

Categories enforced (full list in root `Cargo.toml`):

- **Don't panic** — no `unwrap`, `expect`, `panic`, `todo`, `unreachable`
  in production paths. Test code is exempt via `clippy.toml` carve-outs.
- **Don't fail silently** — results must be handled or explicitly discarded.
- **Don't do bad async** — no locks held across `.await`.
- **Memory safety** — all `unsafe` blocks must be documented.
- **Suppression discipline** — use `#[expect(..., reason = "…")]`, never
  bare `#[allow(...)]`. The lint set itself enforces this.

## Build and check

```
just check     # fmt + clippy + test
just test      # full test run
just coverage  # llvm-cov, used by CI
```

CI runs `just check` and `just coverage`; both must pass.

## Specs drive code

This repo follows spec-first development. `docs/specs/` holds the
authoritative description of each feature. Don't write implementation that
isn't covered by a spec — the spec is the agreed surface, and implementers
get latitude on the *how* but not the *what*.
