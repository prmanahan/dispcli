# Specs

Specifications drive implementation in this repo: a spec defines *what done
looks like*, an implementer (human or agent) decides *how*.

## Layout

```
docs/specs/
├── README.md           # this file
└── <NNNN>-<slug>.md    # one spec per feature, numbered sequentially
```

## v0 scope (planned)

The first spec — yet to be written — defines envelope assembly:

- Envelope JSON input schema (agent, pattern, task context, scope overrides)
- `SkillResolver` trait in `dispcli-core` (IO-free — implementations live
  in `dispcli-io` or the future WASM host)
- Envelope construction logic (skills lookup, scope assembly, prompt
  composition)
- CLI output contract: envelope written to a path, JSON summary on stdout
  (path, mode, working dir, worktree commands)

Deferred to v1+: worktree exec, cost-metric emission, post-dispatch git
verification, MCP/WASM frontend.
