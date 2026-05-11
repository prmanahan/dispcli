---
status: "proposed"
date: "YYYY-MM-DD"
decision-makers: []
consulted: []
informed: []
supersedes: null
superseded_by: null
---

# ADR-NNNN: Title

## Status

`proposed` | `rejected` | `accepted` | `deprecated` | `superseded`

## Context and Problem Statement

Describe the context and the problem you are solving. What forces or constraints led to this decision point?

## Decision Drivers

- Driver 1: e.g., operational simplicity
- Driver 2: e.g., alignment with existing toolchain
- Driver 3: e.g., security posture

## Considered Options

1. **Option A** — brief label
2. **Option B** — brief label
3. **Option C** — brief label

## Decision Outcome

Chosen option: **Option A**, because [brief rationale].

### Consequences

**Good:**
- Consequence 1
- Consequence 2

**Bad / Trade-offs:**
- Trade-off 1

## Pros and Cons of the Options

### Option A

- Pro: ...
- Pro: ...
- Con: ...

### Option B

- Pro: ...
- Con: ...
- Con: ...

### Option C

- Pro: ...
- Con: ...

## More Information

Links, references, related ADRs, or follow-up work items.

---

### Frontmatter Field Reference

**`superseded_by`** — Use this field when this ADR has been replaced by a newer decision. Set it to the identifier of the superseding ADR (e.g., `superseded_by: "0007-new-decision.md"`). When this field is non-null, this ADR's `status` MUST also be set to `superseded`. The superseding ADR SHOULD reference this one in its `supersedes` field, creating a bidirectional link. Do not delete or edit an accepted ADR to reflect new thinking — create a new ADR that supersedes it instead. The original record preserves the historical context and reasoning that led to the prior decision.
