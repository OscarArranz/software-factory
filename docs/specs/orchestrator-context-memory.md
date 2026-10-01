# Orchestrator conversation context and memory

- **Status:** Implemented
- **Last updated:** 2026-10-01

## Context and problem

Project orchestration currently includes the full conversation, every plan and
task, and all task activity in every agent prompt. This makes each turn larger
as a user explores ideas and requests features over time, even though the
complete transcript needs to remain available.

## Goals

- Keep one continuous project conversation while bounding each orchestrator
  prompt.
- Preserve important user constraints, decisions, unresolved questions, and
  speculative ideas across conversation compaction.
- Retrieve relevant older discussion when a topic returns.
- Keep conversation memory separate from authoritative plans, approvals, and
  task execution state.

## Out of scope

- Deleting or rewriting the user's stored transcript.
- Creating separate feature threads or changing the user's chat workflow.
- Vector databases, external memory services, or changing task authorization.

## Expected behavior

The full project conversation remains stored. The orchestrator receives a
bounded context containing the current project snapshot, a durable structured
memory, recent messages, relevant older excerpts, and concise current task
state. Older messages are summarized in bounded batches when they fall outside
the recent window. The memory records decisions, open questions and ideas
separately, and is updated atomically without changing project workspace state.

When a user returns to an older topic, relevant transcript excerpts are
retrieved by textual relevance and included with their message IDs. If the
reference cannot be resolved from the available context, the orchestrator asks
a clarifying question. The transcript remains available for future retrieval.

Memory is advisory context only. It cannot approve, queue, or alter a plan or
task. Only the existing exact `confirm <plan-id>` command authorizes execution.
Invalid summaries or concurrent checkpoint updates do not replace saved memory.

## Requirements

- **R1:** Preserve the complete project transcript and task records in storage.
- **R2:** Keep orchestration prompts at or below 64,000 Unicode characters,
  including their instructions and serialized context. Never truncate the
  current user message or omit exact plan/task state needed for authorization.
- **R3:** Include at most the 20 most recent conversation messages directly;
  summarize older uncheckpointed messages in batches of at most 16 messages
  and 24,000 message-content characters.
- **R4:** Persist a structured memory containing a summary, decisions, open
  questions, and ideas, with source message IDs for each fact and an
  8,000-character serialized limit. Preserve the covered-through message ID
  and use compare-and-swap revisions.
- **R5:** Retrieve at most 8 older message excerpts, each no longer than 1,200
  characters, using textual relevance to the current user message. Include
  source message IDs and never replace authoritative project state with
  retrieved text.
- **R6:** Include current actionable and unresolved task state plus concise
  recent and relevant completed-task summaries without serializing all
  completed task activity or historical plans into every prompt.
- **R7:** Invalid, oversized, or stale summary output must not replace the
  existing memory checkpoint. Memory content never authorizes task execution.

## Acceptance criteria

- **AC1:** Given a long transcript and task history, when an orchestration
  prompt is built, then the prompt stays within 64,000 characters while the
  full transcript remains stored.
- **AC2:** Given older discussion containing decisions, open questions, and
  ideas, when it is compacted and the project is reopened, then the validated
  memory persists and remains available to later turns.
- **AC3:** Given a user message that refers to an older topic, when matching
  transcript messages exist, then a bounded set of relevant excerpts with
  source IDs is included in context.
- **AC4:** Given malformed or stale compaction output, when the update is
  attempted, then the previous memory checkpoint is unchanged.
- **AC5:** Given discussion or memory that mentions a plan, when orchestration
  responds, then no task can execute without exact plan confirmation.
- **AC6:** Given concurrent task state updates, when conversation memory is
  saved, then task and approval state are not overwritten.

## Technical considerations

Use a separate SQLite table and revision for conversation memory. Summarize
bounded transcript batches with a strictly validated agent response. Validate
memory citations against the preserved transcript. Build
prompts from curated context sections, retrieve older messages using local text
matching, and enforce the final serialized prompt limit before invocation.
Character budgets are deterministic input-size limits, not estimates of model
tokenization or a guarantee about repository context read by the agent.

## Open questions

- None for the initial implementation. Relevance retrieval uses local textual
  matching; semantic retrieval can be considered if observed usage shows it is
  insufficient.

## Verification

Verified with `cargo fmt --check`, `cargo test --workspace` (61 tests), and
`cargo clippy --workspace --all-targets --all-features -- -D warnings`.
Verified with a fixture agent that a project chat compacts old messages,
persists the cited memory checkpoint, retrieves a related older discussion,
and still requires exact plan confirmation. Unit tests exercise the prompt
character limit, full current-message inclusion, completed-task filtering,
source citation validation, stale checkpoint rejection, and transcript
preservation. Live model-generated memory compaction was not exercised.
