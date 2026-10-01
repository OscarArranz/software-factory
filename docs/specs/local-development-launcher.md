# Local development launcher

- **Status:** Implemented
- **Last updated:** 2026-10-01

## Context and problem

Developers currently need separate terminal sessions to start the project
builder API and web UI. A single command should make it convenient to run both
while developing.

## Goals

- Launch the REST API and web UI together from the repository root.
- Keep the existing API and Trunk development-server behavior.

## Out of scope

- Serving the compiled web UI from the API process.
- Installing or downloading Trunk or the WebAssembly target automatically.

## Expected behavior

From the software-factory repository root, `sf dev` starts the API on
`127.0.0.1:3000` and runs `trunk serve` for `crates/web-ui` on
`127.0.0.1:8080`. API host and port can be set with `--host` and `--port`; the
web UI port can be set with `--web-ui-port`. The UI is built to use the API
origin selected by `--host` and `--port`; an explicit `SF_API_BASE_URL` takes
precedence. Stopping either server stops the combined command. Ctrl+C stops
both.

Trunk and the `wasm32-unknown-unknown` Rust target must already be installed. If
the command is run outside the repository root, or Trunk cannot be started, it
reports an actionable error.

## Requirements

- **R1:** `sf dev` must start `sf serve` and Trunk's web UI development server
  together, using the defaults `127.0.0.1:3000` and `127.0.0.1:8080`.
- **R2:** The API `--host` and `--port` options and the web UI `--web-ui-port`
  option must configure their respective servers. The web UI must use the
  configured API origin unless `SF_API_BASE_URL` is explicitly set.
- **R3:** If either server exits or the user presses Ctrl+C, the combined command
  must stop the other server.
- **R4:** Missing repository UI files or an unavailable Trunk executable must
  produce a useful error.

## Acceptance criteria

- **AC1:** Given the required tools are installed and `sf dev` runs from the
  repository root, then the UI is available on port 8080 and the API health
  endpoint is available on port 3000 at the same time.
- **AC2:** Given API or web UI port options, when `sf dev` starts, then each
  server uses its configured port and the web UI uses the configured API origin
  unless `SF_API_BASE_URL` is explicitly set.
- **AC3:** Given either server exits or Ctrl+C is pressed, when the command
  stops, then its sibling server is stopped as well.
- **AC4:** Given `sf dev` is run outside the repository root or Trunk is not
  installed, then it exits with an actionable error.

## Technical considerations

Run Trunk with `crates/web-ui` as its working directory and host the API using
the existing server implementation. Use asynchronous process management so
either server's exit can be observed without blocking the other.

## Open questions

- None.

## Verification

Verified with `cargo fmt --check`, `cargo test` (41 tests), and
`cargo clippy --all-targets --all-features -- -D warnings`. Installed with
`cargo install --path . --force`. A smoke test ran `sf dev` with custom ports,
confirmed the API health endpoint and web UI returned successfully at the same
time, and verified Ctrl+C stopped both servers. Also verified actionable errors
when run outside the repository root and when Trunk is unavailable.
