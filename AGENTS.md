# AI Agent Guide

## Project context

This repository is the starting point for a software factory. Its purpose,
users, capabilities, and constraints have not yet been defined. Do not infer
product requirements from the repository name or present ideas as agreed
decisions.

## Source of truth and SDD workflow

- Read the relevant specifications in `docs/specs/` before changing product
  behavior.
- Approved specifications are the source of truth for scope and acceptance
  criteria. If no specification exists for a change, draft a proposal using
  `docs/specs/templates/feature.md`.
- Keep open questions and pending decisions visible. Do not resolve them through
  silent assumptions.
- Implement a proposal when the user has authorized its scope. If the request
  leaves important behavior ambiguous, clarify the specification before
  implementation.
- When implementing, update the specification to reflect the result and any
  agreed deviations. Do not mark an acceptance criterion as met unless it has
  been verified.
- Keep scope small and avoid architectural decisions or dependencies that are
  not needed for a documented need.

## Working conventions

- Keep changes consistent with the project's style and structure. Source code
  lives in `src/`; the project uses Rust 2024 and Cargo.
- Write all documentation and code in English by default, including identifiers,
  comments, and user-facing text, unless the user explicitly requests another
  language.
- Do not edit `Cargo.lock` manually or include generated artifacts from
  `target/`.
- After every project build, install or refresh the executable for terminal use
  by running `cargo install --path . --force`. Ensure Cargo's binary directory
  (by default, `$HOME/.cargo/bin`) is on `PATH` so the `sf` executable can be
  run from any working directory.
- Before completing code changes, run the relevant checks:

  ```sh
  cargo fmt --check
  cargo test
  cargo clippy --all-targets --all-features -- -D warnings
  ```

- In the final summary, state what changed, which decisions or questions remain
  open, and which checks you ran.
