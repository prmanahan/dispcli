# Repository layout

`dispcli` is split into three crates with a strict separation of concerns:

| Crate | Role |
|---|---|
| `dispcli-core` | IO-free dispatch-envelope logic — types, traits, assembly. |
| `dispcli-io` | Native IO adapters implementing `dispcli-core`'s traits (filesystem skill resolution, envelope writes, etc.). |
| `cmd/dispcli` | The thin binary: parses args, wires `dispcli-io` adapters into `dispcli-core`, emits output. |

The IO-free boundary on `dispcli-core` is deliberate — it's what lets the
same core logic run behind a future WebAssembly frontend without a rewrite.

## Docs split

- `docs/specs/` — the feature specs (source of truth)
- `docs/src/` — this rendered book

## Full layout

For the complete as-built directory tree, layout conventions, and lint
discipline, see the repository's
[`CLAUDE.md`](https://github.com/mnemra/dispcli/blob/main/CLAUDE.md) — it's
the authoritative source and is kept current as the repo evolves.
