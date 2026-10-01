# Software Factory

This repository is the starting point for developing a software factory. Its
broader purpose, users, scope, and constraints will be defined incrementally
through specifications before capabilities are implemented.

## Current status

- Executable project using Rust 2024 and Cargo.
- `sf serve` starts the local project-builder HTTP API on `127.0.0.1:3000` by
  default. It stores data in `$HOME/sf` and launches OpenCode planning and
  implementation agents when requested.
- The multi-crate Cargo workspace includes a Leptos CSR web client at
  `crates/web-ui` and shared API types in `crates/api-types`.
- The architect collects functional/non-functional requirements and acceptance
  criteria. Created projects have an orchestrator chat and a read-only kanban.
- The [`sf` terminal command](docs/specs/terminal-command.md),
  [project builder API](docs/specs/project-builder-api.md),
  [web UI](docs/specs/builder-web-ui.md),
  [local development launcher](docs/specs/local-development-launcher.md), and
  [project orchestration](docs/specs/project-orchestration.md) are implemented.
  See
  [`docs/specs/`](docs/specs/) for the workflow and template.

## Development

Requires Rust and Cargo.

```sh
cargo run -- serve
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

### Run the web UI

Install the WebAssembly target and Trunk once:

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
```

From the repository root, start both the REST API and web client together:

```sh
sf dev
```

Open `http://127.0.0.1:8080`. The API is available at
`http://127.0.0.1:3000`. Set `SF_API_BASE_URL` when building with Trunk to target
another API origin. API bind settings and the UI port can be overridden with
`sf dev --host <host> --port <port> --web-ui-port <port>`. Trunk and the
`wasm32-unknown-unknown` Rust target must be installed. To run the services
individually, start `sf serve` and run `trunk serve` from `crates/web-ui` in
separate terminals.

To create an optimized static bundle, run `trunk build --release` from
`crates/web-ui`; the generated `dist/` directory is ignored by Git.

### Build and install for terminal testing

After every build, install or refresh the executable so it can be tested from
the terminal in any working directory:

```sh
cargo build
cargo install --path . --force
```

Cargo installs executables into `$CARGO_HOME/bin`, which defaults to
`$HOME/.cargo/bin`. Make sure that directory is on `PATH`, then verify and run
the command from any directory:

```sh
command -v sf
sf serve
```

The API defaults to `http://127.0.0.1:3000`; override its bind address with
`sf serve --host <host> --port <port>`. Browser requests are allowed from HTTP
localhost and loopback origins. The API contract currently includes:

- `POST /api/sessions`, `GET /api/sessions`, and `GET /api/sessions/{id}`
- `POST /api/sessions/{id}/messages`
- `PUT`/`DELETE /api/sessions/{id}/requirements/{requirement_id}/pin`
- `DELETE /api/sessions/{id}/requirements/{requirement_id}`
- `POST /api/sessions/{id}/deleted-requirements/{requirement_id}/restore`
- `POST /api/sessions/{id}/projects`, `GET /api/projects`, and
  `GET /api/projects/{id}`
- `GET /api/projects/{id}/workspace` (conversation, plans and tasks)
- `POST /api/projects/{id}/messages` (orchestration and plan confirmation)

### Project changes and local Git execution

Open a completed project from **Projects**, describe a change to the orchestrator,
and review its proposed tasks and verification commands. Use **Confirm plan in
chat**, or send `confirm <plan-id>`, to authorize that exact plan version.
Task content and status are agent-managed; the kanban has no editing controls.

Independent tasks run in parallel (two per project by default). Configure the
limit from 1 to 16 with `SF_IMPLEMENTER_CONCURRENCY`. Dependencies wait until
their predecessors have merged. Every implementer starts in an isolated branch
and worktree from current `main`; the executor verifies changes, commits them,
serializes integration, incorporates newer main changes, verifies again and
merges locally. Successfully merged worktrees are removed. Failed or conflicting
work is retained for investigation and follow-up through an approved plan.
No remote Git operations are used. Git must be installed.

Existing requirements remain unclassified until refined by the architect; pinned
ones must be unpinned before refinement. Existing projects without Git are
initialized on the first orchestrator request. Existing dirty repositories must
be committed or stashed before execution. Existing branches/files are preserved.

OpenCode agent runs are limited to 600 seconds by default. Set
`SF_AGENT_TIMEOUT_SECS` before starting `sf serve` to configure a different
timeout. Timed-out builds and builds that produce no files are reported as
failed rather than remaining in the running state. Interrupted task execution is
reconciled with Git on startup: merged commits are recognized and worktrees
cleaned, while unresolved work is blocked and retained. Interrupted initial
creation that has not merged, and older completed records without files, is
marked failed. Approved tasks still queued resume after startup.

## Specification-driven development (SDD)

Before implementing a capability, document the problem to solve, its scope, and
how it will be verified. Proposals and their statuses are managed in
[`docs/specs/README.md`](docs/specs/README.md). Guidance for agents and
contributors is in [`AGENTS.md`](AGENTS.md).

All documentation and code in this repository must be written in English unless
the user explicitly requests otherwise. This includes code identifiers,
comments, and user-facing text.
