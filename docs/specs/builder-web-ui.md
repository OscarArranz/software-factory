# Project builder web UI

- **Status:** Implemented
- **Last updated:** 2026-09-30

## Context and problem

The project builder API has no user interface. Users need a web application to
converse with the planning agent, review the agreed architecture and
requirements, and launch and monitor project creation.

## Goals

- Provide a responsive browser UI built with Leptos CSR.
- Use the existing `sf serve` HTTP API for all persisted state and agent actions.
- Present the initial composer and subsequent builder workspace in the visual
  direction of the supplied dark-header, blue-gradient reference.
- Keep the API contract shared between the server and web client.

## Out of scope

- Changing the builder API's behavior or persistence model.
- Server-side rendering or serving the compiled UI from `sf serve`.
- A native desktop client.

## Expected behavior

The [project orchestration specification](project-orchestration.md) extends this
baseline with categorized requirements, acceptance criteria, open questions,
project detail/chat, plan confirmation and a read-only task kanban.

The web application is built and served separately with Trunk. In development,
it connects to `http://127.0.0.1:3000` by default while `sf serve` runs in a
separate process. The API base URL can be configured at build time.

The welcome screen provides a project-planning prompt composer and recent
sessions. Sending the first prompt creates a session when needed and sends the
message to the planning API. The builder workspace displays the conversation
and the current architecture, stack, active requirements, and deleted
requirements. Users can pin or unpin active requirements, delete them, and
explicitly restore deleted requirements.

Project creation is available when a session has an architecture and at least
one active requirement. The user supplies the project name. The UI submits the
request, displays the queued/running/completed/failed state, and refreshes the
project record while work is in progress. API and network errors are visible to
the user, and actions are disabled while their request is pending.

## Requirements

- **R1:** The Cargo repository must be a multi-crate workspace containing the
  existing `sf` server, a shared API-types crate, and a Leptos web UI crate.
- **R2:** The web application must be a Leptos client-side rendered WebAssembly
  app built and served with Trunk, separate from `sf serve`.
- **R3:** The frontend must use shared request/response types and the existing
  API for sessions, messages, requirement actions, and projects.
- **R4:** The builder view must display chat, architecture/stack, active
  requirements and deleted requirements, including the existing pin, delete,
  unpin, and restore operations.
- **R5:** The user must be able to submit a named project from a valid plan and
  observe its persisted execution status.
- **R6:** The UI must follow the supplied dark navigation and blue-gradient
  visual direction and remain usable on narrow screens.
- **R7:** The default API origin must be `http://127.0.0.1:3000`, with a
  build-time override for other deployments.
- **R8:** Pressing Enter in the message composer must send the message;
  Shift+Enter must insert a newline instead.
- **R9:** The user's message must appear in the conversation immediately while
  the planning agent is responding.
- **R10:** Pinning and unpinning a requirement must update its visible state and
  action label when the API returns the updated session.

## Acceptance criteria

- **AC1:** Given `sf serve` and Trunk are running, when the web app loads, then
  it lists existing sessions and can create a new one.
- **AC2:** Given a session, when the user sends a message, then the conversation
  and planning panel reflect the API's updated session.
- **AC3:** Given active or deleted requirements, when the user pins, unpins,
  deletes, or restores one, then the UI reflects the returned session state.
- **AC4:** Given a session with architecture and active requirements, when the
  user enters a project name and starts creation, then the UI displays project
  status through a terminal state.
- **AC5:** Given an API/network error, when a UI operation fails, then an
  actionable error is displayed and pending controls recover.
- **AC6:** Given a narrow viewport, when the builder is displayed, then the
  conversation and planning panel remain accessible without horizontal
  overflow.
- **AC7:** Given the workspace, when Rust checks and the Trunk WebAssembly build
  run, then the server, shared crate, and frontend compile successfully.
- **AC8:** Given text in the composer, when the user presses Enter, then the
  message is submitted; when the user presses Shift+Enter, then a newline is
  inserted without submitting.
- **AC9:** Given a planning session, when the user submits a message, then the
  user message appears before the agent response completes.
- **AC10:** Given an active requirement, when the user pins or unpins it and the
  API succeeds, then the pin state and button label reflect the returned value.

## Technical considerations

Keep the root package as the native server and set it as the workspace's default
member so existing `cargo run` and `cargo install --path . --force` workflows
remain focused on `sf`. Place browser-only dependencies behind a wasm32 target
dependency so native workspace tests do not require DOM support. Share API DTOs
through a small serde-only crate. Use Trunk for CSR development and static
output; add its generated `dist` directory to `.gitignore`.

## Open questions

- None.

## Verification

Verified with `cargo fmt --check`, `cargo test --workspace` (40 tests), and
`cargo clippy --workspace --all-targets --all-features -- -D warnings`. Built the
Leptos CSR application with `trunk build` for `wasm32-unknown-unknown`. A local
smoke test started `sf serve` and `trunk serve` separately, fetched the UI and
API health endpoint, and verified that the Trunk localhost origin passes API
CORS for `localhost.` and `127.0.0.1` origins, including JSON preflight headers.
UI interaction tests cover Enter/Shift+Enter, optimistic user-message insertion,
and pin-state row updates. The existing API tests cover session, requirement,
and project lifecycle behavior; interactive browser automation was not run.
