---
dispatch_id: 42
task_id: null
agent_id: implementer
spec_id: null
spec_version: null
parent_commit: deadbeefdeadbeefdeadbeefdeadbeefdeadbeef
repo: /fixtures/repo
worktree: null
branch: feature-t5-golden
report_path: /fixtures/repo/scratch/dispatch-42-report.md
deadline_minutes: null
command_scope_subtract: []
command_scope_add: []
touch_scope: []
forbid_scope: []
verify: []
---

<role>
Implementer agent for dispcli fixture testing.
</role>

## Core skill

Keep fixture content short, deterministic, and free of placeholders.

## Metrics block

Record dispatch metrics via the brain CLI after each phase.

Fixture task: run `dispcli assemble` against the happy-path registry and request, and confirm the assembled document matches the golden byte-for-byte.