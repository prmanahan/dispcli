# Specifications

`dispcli` follows spec-first development: a spec defines *what done looks
like* before any implementation begins. Contributors get latitude on *how*
to build a feature, but not on *what* the feature does — that's fixed by
the spec.

Specs live in the repository at `docs/specs/` and are the agreed surface
between design and implementation. This section of the book renders them
for reading; the files under `docs/specs/` remain the source of truth.

## Current specs

| Spec | Summary |
|------|---------|
| [0001 — Envelope assembly](0001-envelope-assembly.md) | Turns a dispatch request and a registry into a dispatch document (envelope + composed prompt) |
