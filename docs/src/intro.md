# dispcli

`dispcli` constructs the inputs an orchestrator needs to dispatch a specialized agent: which skills to inject, what scope to draw, what mode to use, what worktree commands to run. The orchestrator (today: a CLI caller; tomorrow: an mnemra plugin host) consumes the result and fires the actual agent invocation.

This documentation covers the internal architecture, design decisions, and developer guides for contributors and integrators.

## About this site

This docs site is built with [mdBook](https://rust-lang.github.io/mdBook/) and includes:

- **Architecture Decision Records (ADRs)** — the reasoning behind key design choices
- **Specifications** — the authoritative feature contracts (what "done" looks like)
- **Developer guides** — how to build, extend, and contribute

## Repository

Source code lives at: [https://github.com/mnemra/dispcli](https://github.com/mnemra/dispcli)

For a project overview and quick-start instructions, see the [README](https://github.com/mnemra/dispcli/blob/main/README.md) in the repository root.
