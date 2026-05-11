# prmanahan/dispcli

[![CI](https://github.com/prmanahan/dispcli/actions/workflows/ci.yml/badge.svg)](https://github.com/prmanahan/dispcli/actions/workflows/ci.yml)
[![Coverage](https://codecov.io/gh/prmanahan/dispcli/branch/main/graph/badge.svg)](https://codecov.io/gh/prmanahan/dispcli)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Dispatch-envelope assembler for agent orchestration.

> **Status: v0 scaffold.** Implementation is spec-pending — see
> `docs/specs/`. The public surface in this v0 is a placeholder so build,
> lint, and CI surfaces are exercisable end-to-end. Real CLI lands with
> the first spec.

---

## What this is

`dispcli` constructs the inputs an orchestrator needs to dispatch a
specialized agent: which skills to inject, what scope to draw, what mode
to use, what worktree commands to run. The orchestrator (today: a CLI
caller; tomorrow: an mnemra plugin host) consumes the result and fires
the actual agent invocation.

Two frontends share one core:

- **Native CLI** (this repo) — for use today against a local skills
  directory and scratch path.
- **WebAssembly Component Model plugin** (future, lives in mnemra) —
  re-implements IO via WIT host functions, leaving the core untouched.

The three-crate layout (`libs/dispcli-core` IO-free, `libs/dispcli-io`
native adapters, `cmd/dispcli` binary) is the mechanism that makes the
WASM port a re-wire, not a rewrite. See `CLAUDE.md` for the architectural
rules.

## What this is NOT

- A general-purpose CLI generator.
- An agent runtime — it builds the input; something else runs the agent.
- An MCP server. (The WASM plugin frontend, when it lands, is *inside*
  mnemra, not a stdio MCP wrapper around this binary.)

---

## Quick start

```bash
git clone https://github.com/prmanahan/dispcli
cd dispcli
just check   # fmt + clippy + test
just build
./target/debug/dispcli --help
```

---

## What's included (v0 scaffold)

| Component | Detail |
|-----------|--------|
| `cmd/dispcli/` | Binary scaffold — reports core version, no real surface yet |
| `libs/dispcli-core/` | IO-free crate — types, traits, envelope assembly (spec-pending) |
| `libs/dispcli-io/` | Native IO adapters — filesystem skill resolver, scratch writer (spec-pending) |
| `docs/specs/` | Spec directory — drives all implementation |
| `justfile` | Standard recipes: `check`, `fmt`, `test`, `coverage`, `build`, `release` |
| `.cargo/config.toml` | `lld` linker for faster dev builds |
| `deny.toml` | License policy (Green/Yellow/Red) |
| `clippy.toml` | Test carve-outs (allow `unwrap`/`panic` in tests) |
| `.github/workflows/ci.yml` | CI: `just check` + `just coverage` + Codecov upload |

---

## Contributing

Read `CLAUDE.md` first — it codifies the layout rules, lint discipline,
and the IO-free invariant on `dispcli-core`. Specs in `docs/specs/`
define the *what*; implementers pick the *how* within those bounds.

---

## License

MIT — see [LICENSE](LICENSE).
