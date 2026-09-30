# Short terminal command (`sf`)

- **Status:** Implemented
- **Last updated:** 2026-09-30

## Context and problem

The project executable should have a short command name that is convenient to
invoke from a terminal.

## Goals

- Make the installed executable invocable as `sf` from any working directory.

## Out of scope

- Defining additional product capabilities or changing the application's
  behavior.

## Expected behavior

When installed and when Cargo's binary directory is on `PATH`, invoking `sf`
starts the project executable regardless of the current working directory.

## Requirements

- **R1:** Cargo must build and install the executable under the command name
  `sf`.

## Acceptance criteria

- **AC1:** Given the project is installed and Cargo's binary directory is on
  `PATH`, when `sf` is invoked from outside the repository, then the installed
  executable starts and exits successfully.

## Technical considerations

Configure the Cargo binary target name without changing the package name.

## Open questions

- None.

## Verification

Verified with `cargo fmt --check`, `cargo test`, and
`cargo clippy --all-targets --all-features -- -D warnings`. Installed with
`cargo install --path . --force`, then ran `command -v sf && sf` from outside
the repository successfully.
