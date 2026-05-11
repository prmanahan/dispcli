# Architecture Decision Records

This section tracks the architectural decisions made in dispcli using the [MADR](https://adr.github.io/madr/) (Markdown Architectural Decision Records) format.

## What is an ADR?

An Architecture Decision Record (ADR) captures a single architectural decision along with its context and rationale. ADRs are immutable once accepted — they are not edited to reflect changing opinions, but superseded by newer ADRs when decisions evolve.

## ADR Lifecycle

Each ADR moves through the following statuses:

| Status | Meaning |
|--------|---------|
| `proposed` | Under discussion; not yet accepted |
| `rejected` | Discussed and explicitly declined |
| `accepted` | The current decision in effect |
| `deprecated` | Was accepted; no longer applies (context changed) |
| `superseded` | Replaced by a newer ADR; see `superseded_by` field |

## Using the Template

To record a new ADR, copy [`template.md`](template.md) to a numbered file (e.g., `adrs/0001-topic.md`), fill in all frontmatter fields, and write the body sections.

## Current ADRs

None yet. ADRs will be added as architectural decisions are made and ratified.
