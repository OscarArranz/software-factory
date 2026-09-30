# Project builder API

- **Status:** Implemented
- **Last updated:** 2026-09-30

## Context and problem

Software Factory needs a backend that lets an external client collaborate with a
planning agent, review and manage the resulting architecture and requirements,
and ask an implementation agent to create the agreed project. Web and native
clients should be able to use the same application capabilities.

## Goals

- Expose the builder through `sf serve` and a transport-independent application
  layer.
- Persist conversations, planning state, and created-project records in SQLite.
- Let the user pin and delete requirements without allowing later agent turns to
  alter pinned requirements or accidentally recreate deleted ones.
- Create a project from the agreed architecture, stack, and requirements under
  `$HOME/sf/projects`.
- Keep OpenCode behind an agent adapter and validate all structured planning
  output before changing canonical state.

## Out of scope

- Building or designing a web or native UI.
- Supporting agent providers other than OpenCode.
- Remote database hosting or multi-user access control.

## Expected behavior

`sf serve` starts an HTTP API, listening on `127.0.0.1` by default. Host and
port can be configured when starting the server. The previous `sf run` command
is removed. Browser CORS requests are allowed only from HTTP localhost and
loopback origins, so local web clients can use the API without enabling remote
web origins by default.

Clients can create, list, and retrieve planning sessions, send chat messages,
view the conversation, architecture, active requirements, and deleted
requirements, and manage requirement pinning. The planning agent receives the
session history, current architecture and requirements, and deleted-requirement
tombstones, then returns a conversational response and typed
architecture/requirement changes.
OpenCode's JSON event output is transport framing, not a content schema, so the
adapter extracts the final assistant text and the application strictly parses
and validates its JSON envelope before applying any changes.

An agent turn may add or update unpinned requirements and propose architecture
changes. It cannot pin, unpin, or delete requirements. Pinning, unpinning,
deletion, and restoration are user-only API operations. A deleted requirement
is retained as a tombstone and included in later agent context. Restoring one
requires an explicit user action. Requirement changes are applied atomically;
invalid or stale agent output must not partially mutate session state. The
server rejects duplicate requirements, including normalized text duplicates of
deleted requirements.

The API uses JSON. Creating a session returns `201`; sending a message waits for
the planning agent and returns the updated session. Project creation returns
`202` with a queued project record; clients can poll its project endpoint for
`queued`, `running`, `completed`, or `failed` status. Invalid requests return a
client error, a conflicting or stale state returns `409`, and unavailable or
invalid agent output returns `502` without applying planning changes.

When a client asks to create a project, it supplies an explicit project name.
The builder records the project, creates a directory under
`$HOME/sf/projects/<name>`, and starts an implementation agent in that
directory with a snapshot of the agreed architecture, stack, and requirements.
An existing destination is never overwritten. Project state and execution
outcome are available through the API.

## Requirements

- **R1:** The executable must provide `sf serve`, defaulting to `127.0.0.1`,
  on port `3000`, with configurable listening host and port; it must no longer
  provide `sf run`.
- **R2:** The HTTP API and application services must support multiple client
  types through a shared contract and persist sessions and project records in
  SQLite under `$HOME/sf`.
- **R3:** A session must persist user/agent messages, architecture and stack,
  active requirements, and deleted-requirement tombstones.
- **R4:** Agent planning responses must use a versioned, strictly validated JSON
  envelope containing user-facing text and typed proposed changes. Invalid
  output must not change architecture or requirements.
- **R5:** Agent changes must never alter pinned requirements, change pin state,
  delete requirements, reuse deleted IDs, or recreate a deleted requirement
  with normalized duplicate text. User actions alone may pin, unpin, delete, or
  explicitly restore requirements.
- **R6:** When a client requests project creation with a name, the builder must
  create the project under `$HOME/sf/projects`, persist its status, and invoke
  the implementation agent with the agreed planning snapshot. Existing paths
  must not be overwritten.
- **R7:** Browser CORS must permit HTTP localhost/loopback origins and reject
  non-loopback origins by default.
- **R8:** OpenCode agent processes must have a finite configurable timeout. A
  timeout must set the project to `failed` with a useful error instead of
  leaving it `running` indefinitely.
- **R9:** A project must not be marked `completed` unless the implementation
  agent created at least one project file outside `.git`.
- **R10:** When the service starts, project jobs left `queued` or `running` by
  an earlier service process and previously completed projects with no generated
  files must be marked `failed` with an explanatory error.

## Acceptance criteria

- **AC1:** Given no server flags, when `sf serve` starts, then it binds to
  `127.0.0.1`; when host/port are specified, then it uses those values.
- **AC2:** Given a new session, when the client creates it and later retrieves
  it, then its messages, architecture, requirements, and tombstones persist.
- **AC3:** Given a pinned requirement, when an agent proposes changing it, then
  the turn is rejected without changing the requirement; after the user
  unpins it, an otherwise valid change may be applied.
- **AC4:** Given a deleted requirement, when an agent proposes its ID or a
  normalized duplicate, then the proposal is rejected; when the user explicitly
  restores it, it becomes active again.
- **AC5:** Given malformed, unknown-version, or otherwise invalid agent JSON,
  when a chat turn completes, then no proposed architecture or requirement
  changes are applied.
- **AC6:** Given a project name and valid planning state, when the client
  requests project creation, then the implementation agent receives that
  snapshot and the project record reports its execution status.
- **AC7:** Given the requested destination already exists, when project
  creation is requested, then the existing directory is left untouched and the
  API reports a conflict.
- **AC8:** Given the new CLI, when `sf run` is invoked, then it is reported as
  an unknown command; `sf --help` documents `sf serve`.
- **AC9:** Given a browser preflight from a loopback origin, when it requests an
  API method, then the API returns the appropriate CORS headers; a non-loopback
  origin receives no allow-origin header.
- **AC10:** Given an implementation agent that times out or exits successfully
  without creating files, when project execution ends, then the project is
  marked `failed` with an explanatory error.
- **AC11:** Given a project job was `queued` or `running` before the service
  stopped, or a completed project has no generated files, when the service
  starts again, then that record is marked `failed`; completed projects with
  files remain completed.

## Technical considerations

Keep application/domain operations independent of HTTP handlers. Use SQLite
transactions and session revisions so agent output is validated against current
state before atomic application. The database schema uses SQLite's
`user_version`. Generate requirement IDs in the application, not in the model.
Capture OpenCode stdout and stderr separately; `--format json` provides JSON
events but does not constrain model content. Use Rust's typed JSON
deserialization plus domain validation and fail closed if output is invalid.
Run the planner with OpenCode's read-only plan agent. Run the implementation
agent with the new project directory as its working directory.

## Open questions

- None.

## Verification

Verified with `cargo fmt --check`, `cargo test` (36 tests), and
`cargo clippy --all-targets --all-features -- -D warnings`. Installed with
`cargo install --path . --force`. A smoke test started `sf serve` with a
temporary home directory, fetched `/api/health`, and created a persisted
session. Verified `sf --help` and confirmed the removed `sf run` command exits
unsuccessfully. API tests cover session persistence, agent response validation,
pin/unpin and tombstone behavior, project creation/status, destination conflicts,
and loopback-only browser CORS. OpenCode CLI arguments and JSON-event parsing
were tested with fixtures. A live OpenCode smoke run created a file in a
temporary project directory under `$HOME/sf/projects`.
