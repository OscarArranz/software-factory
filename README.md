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
- The [`sf` terminal command](docs/specs/terminal-command.md),
  [project builder API](docs/specs/project-builder-api.md), and
  [web UI](docs/specs/builder-web-ui.md) are implemented. See
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

Run `sf serve` in one terminal and the web client in another:

```sh
cd crates/web-ui
trunk serve
```

Open `http://127.0.0.1:8080`. The client uses
`http://127.0.0.1:3000` for the API by default. Set `SF_API_BASE_URL` when
building with Trunk to target another API origin.

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

OpenCode agent runs are limited to 600 seconds by default. Set
`SF_AGENT_TIMEOUT_SECS` before starting `sf serve` to configure a different
timeout. Timed-out builds and builds that produce no files are reported as
failed rather than remaining in the running state. Jobs left queued or running
when the service stops, and older completed records without files, are marked
failed on the next startup.

## Specification-driven development (SDD)

Before implementing a capability, document the problem to solve, its scope, and
how it will be verified. Proposals and their statuses are managed in
[`docs/specs/README.md`](docs/specs/README.md). Guidance for agents and
contributors is in [`AGENTS.md`](AGENTS.md).

All documentation and code in this repository must be written in English unless
the user explicitly requests otherwise. This includes code identifiers,
comments, and user-facing text.
