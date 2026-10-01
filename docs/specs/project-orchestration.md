# Architect requirements and project orchestration

- **Status:** Implemented
- **Last updated:** 2026-09-30

## Context and problem

Planning currently collects undifferentiated requirement text. Created projects
have no conversation or task execution workflow.

## Goals

- Collect functional and non-functional requirements with acceptance criteria.
- Expose persistent project orchestration conversations and read-only task boards.
- Execute approved tasks concurrently in isolated local Git worktrees.

## Out of scope

- Git remotes and manual task editing or status transitions.

## Expected behavior

The architect clarifies goals, users, scope, constraints and measurable quality
attributes without inventing decisions. Questions remain visible. Existing
requirements remain unclassified until explicitly refined; pinned requirements
cannot be reclassified by the agent.

Users open a created project and discuss changes with its read-only orchestrator.
The orchestrator proposes versioned plans containing tasks, acceptance criteria,
verification commands and dependencies. An explicit chat confirmation authorizes
the current plan version. Independent tasks execute concurrently with a
configurable limit; dependent tasks wait for completed predecessors. Integration
is serialized per project. Worktrees originate from main, incorporate main
updates and rerun verification before merging. Successful merges are followed by
worktree removal. Failures retain worktrees and explain their cause.

New projects receive local Git/main initialization and use a worktree for initial
implementation. Existing projects and requirements are preserved. Preparing an
existing repository never overwrites dirty files or existing branches. Interrupted
jobs are reconciled against Git; ambiguous outcomes are blocked rather than
blindly repeated.

## Requirements

- **R1:** Persist requirement categories, acceptance criteria and open questions;
  preserve them through pinning, deletion, restoration and snapshots.
- **R2:** Persist project conversations, versioned plans, tasks and Git outcomes.
- **R3:** Only explicitly confirmed plan versions may enqueue implementation.
- **R4:** Claim tasks atomically, honor dependencies and limit parallel execution.
- **R5:** Execute changes and checks in branches/worktrees based on main; commit,
  integrate with serialized merges and remove worktrees after successful merges.
- **R6:** Expose task reads only through public HTTP endpoints.
- **R7:** Leptos displays categorized requirements, project chat, pending plans,
  task detail and a responsive read-only kanban refreshed while execution runs.
- **R8:** Preserve existing data and reconcile interrupted jobs on startup.
- **R9:** The project workspace must prioritize project context and task progress,
  avoid exposing internal identifiers as primary content, and keep every kanban
  state legible and navigable at desktop and mobile widths.

## Acceptance criteria

- **AC1:** Typed architect output persists categories, criteria and questions;
  invalid output or pinned changes apply no partial changes.
- **AC2:** Migrated legacy requirements retain IDs/text/pins and are unclassified.
- **AC3:** Proposed tasks do not run until their exact plan version is confirmed;
  duplicate confirmation cannot execute tasks twice.
- **AC4:** Independent tasks can run concurrently; dependent tasks run only after
  predecessors are merged. Every branch starts from main.
- **AC5:** Verification failure or integration conflict is visible and preserves
  work; completed tasks are committed, merged and have no worktree.
- **AC6:** Restart reconciliation never blindly repeats an already merged task.
- **AC7:** Browser users can open projects, converse, confirm plans and observe
  task progress without task-editing or drag/drop controls.
- **AC8:** Given a project with tasks in any status, the board presents a distinct
  readable lane per status, retains horizontal access on narrow screens, and
  distinguishes actionable plan approval from informational metadata.

## Technical considerations

Keep shared serde DTOs, SQLite transactions and strict agent envelopes. Use local
Git subprocesses with argument arrays. Default concurrency is two; configure via
SF_IMPLEMENTER_CONCURRENCY. Poll project workspace state from Leptos. Confirmation
uses the literal chat command `confirm <plan-id>` so authorization is deterministic
and version-specific rather than inferred by a model.

## Open questions

- None blocking implementation. Automatic retries are not authorized; retry or
  follow-up scope is discussed with the orchestrator in a new approved plan.

## Verification

Verified with `cargo fmt --check`, `cargo test --workspace` (52 tests),
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo clippy -p software-factory-web-ui --target wasm32-unknown-unknown -- -D warnings`,
and `trunk build`. Refreshed the terminal executable with
`cargo install --path . --force` and verified that `sf` is on PATH.

- **AC1–AC2:** Domain/store/API tests validate typed responses, atomic application,
  pinned protection, categories/criteria through deletion/restoration/snapshots,
  persistent questions and migration of a schema-version-1 database.
- **AC3:** Tests verify exact confirmation, stale/superseded plan rejection,
  repeated-proposal deduplication and no execution before authorization.
- **AC4–AC5:** Real temporary Git repositories and a barrier-based implementation
  agent verify concurrent execution, atomic claims, dependency ordering, main
  integration, verification failure, conflict preservation and worktree cleanup.
- **AC6:** Restart tests verify already merged commits are recognized and cleaned,
  while unmerged work is blocked instead of rerun.
- **AC7:** A headless Chromium smoke test with HTTP fixtures verifies categorized
  requirements and questions, project navigation, optimistic chat, Enter submit,
  explicit confirmation, task detail, polled completion and a 390px mobile layout
  without horizontal overflow or JavaScript exceptions.
- **AC8:** The same browser review verifies a separate horizontally scrollable
  lane for all eight states at desktop and mobile widths, no global mobile
  overflow, human-readable plan approval in chat, and task details nested below
  concise card summaries without exposing plan IDs as primary content.

A service smoke test runs the installed `sf` with a fixture OpenCode executable
through its actual JSON event adapter and HTTP API: planning, initial creation,
orchestration, confirmation, background scheduling, commits, local merges and
worktree removal all complete successfully. Live model-generated planning and
implementation were not exercised; browser and service smoke fixtures were run
from `/tmp/opencode` rather than adding browser dependencies to the project.
