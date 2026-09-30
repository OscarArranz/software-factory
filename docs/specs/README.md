# Specifications

This directory contains the specifications that guide development. Current
capabilities include the `sf` executable and the project builder API.

## Workflow

1. Create one document per capability in `docs/specs/` using
   [`templates/feature.md`](templates/feature.md). Use a short `kebab-case`
   filename that describes the capability without implying more scope than has
   been agreed.
2. Keep the proposal in `Proposed` status while important scope or criteria
   remain undecided. Record unknowns under “Open questions.”
3. Change the status to `Approved` once the scope and acceptance criteria are
   agreed. Implement against that version of the specification.
4. After the work is complete and the criteria have been verified, change the
   status to `Implemented`. If an agreed decision changes during development,
   update the specification first and record the change.
5. If a proposal is no longer needed, mark it `Rejected` rather than deleting
   it; briefly note why.

An explicit request may authorize implementation of a proposal. If decisions
that would significantly change behavior or scope are missing, leave them open
and ask for clarification before treating them as requirements.

## Specifications

- [Short terminal command (`sf`)](terminal-command.md) — Implemented
- [Project builder API](project-builder-api.md) — Implemented
- [Project builder web UI](builder-web-ui.md) — Implemented
- [Run an AI agent](agent-run.md) — Rejected (superseded by the project builder API)
