# Run an AI agent

- **Status:** Implemented
- **Last updated:** 2026-09-30

## Context and problem

The `sf` command needs to pass user requests to a coding agent. OpenCode is the
first supported agent, while the design must allow other agent integrations to
be added later.

## Goals

- Run an OpenCode request with `sf run <message>`.
- Use GPT-6 Luna with the `xhigh` model variant.
- Keep agent invocation behind a common interface so other agents can be added.

## Out of scope

- Selecting between multiple agents from the command line.
- Integrating agents other than OpenCode.
- Managing OpenCode installation, provider credentials, or permissions.

## Expected behavior

`sf run <message>` passes the message to OpenCode's non-interactive `run`
command with model `openai/gpt-6-luna` and variant `xhigh`. Multiple message
arguments are joined with spaces. The child process runs in the current working
directory and inherits the terminal's standard input, output, and error.

If OpenCode cannot be started, `sf` reports the error and exits unsuccessfully.
If OpenCode exits unsuccessfully, `sf` returns its exit code when available.
Invoking `sf` without a command or with a help flag displays usage information;
an unknown command or a missing message reports usage and exits unsuccessfully.

## Requirements

- **R1:** The CLI must provide `sf run <message>`.
- **R2:** Agent invocation must use a common interface separate from the CLI.
- **R3:** The OpenCode adapter must invoke `opencode run` with model
  `openai/gpt-6-luna` and variant `xhigh`, passing the message as an argument
  without shell interpolation.
- **R4:** The OpenCode process must inherit the current directory and standard
  streams, and `sf` must report start failures and propagate unsuccessful exit
  status.

## Acceptance criteria

- **AC1:** Given a message, when `sf run` is dispatched, then the OpenCode
  adapter receives the complete message and invokes the expected model and
  variant.
- **AC2:** Given OpenCode is unavailable, when `sf run` is invoked, then `sf`
  reports the start failure and exits unsuccessfully.
- **AC3:** Given an unknown command or missing message, when `sf` is invoked,
  then it displays a useful usage error and exits unsuccessfully.
- **AC4:** Given no command or a help flag, when `sf` is invoked, then it
  displays usage and exits successfully.

## Technical considerations

Implement an agent trait in a dedicated `agents` module and keep OpenCode's
process invocation in its own adapter. Avoid a CLI dependency unless needed.

## Open questions

- [ ] What minimum OpenCode CLI version should be documented? The installed
  version here is 1.18.33 and its `opencode run --help` does not list
  `--variant`, which this feature requires for `xhigh`.

## Verification

Verified CLI dispatch and OpenCode command construction with `cargo test` (5
tests). `cargo fmt --check` and
`cargo clippy --all-targets --all-features -- -D warnings` passed. Installed
with `cargo install --path . --force`; from `/tmp`, `sf --help` succeeded,
`sf unknown` exited with status 2, and `sf run "verification prompt"` with
OpenCode removed from `PATH` reported the start failure and exited with status
1. A live model request was not sent. The installed OpenCode CLI does not list
`--variant` in its help, so execution against that installed version remains
unverified.
