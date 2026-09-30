# Software Factory

This repository is the starting point for developing a software factory. Its
broader purpose, users, scope, and constraints will be defined incrementally
through specifications before capabilities are implemented.

## Current status

- Executable project using Rust 2024 and Cargo.
- `sf run <message>` sends a request to OpenCode using GPT-6 Luna with the
  `xhigh` variant (requires an OpenCode CLI that supports `--variant`).
- The [`sf` terminal command](docs/specs/terminal-command.md) and
  [agent runner](docs/specs/agent-run.md) specifications are implemented. See
  [`docs/specs/`](docs/specs/) for the workflow and template.

## Development

Requires Rust and Cargo.

```sh
cargo run
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

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
sf
```

## Specification-driven development (SDD)

Before implementing a capability, document the problem to solve, its scope, and
how it will be verified. Proposals and their statuses are managed in
[`docs/specs/README.md`](docs/specs/README.md). Guidance for agents and
contributors is in [`AGENTS.md`](AGENTS.md).

All documentation and code in this repository must be written in English unless
the user explicitly requests otherwise. This includes code identifiers,
comments, and user-facing text.
