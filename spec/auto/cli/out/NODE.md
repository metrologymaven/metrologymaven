---
id: AUTO.CLI.OUT
title: JSON and human output with exit codes
level: 3
parent: AUTO.CLI
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-cli]
built_pr: 128
satisfies: [REQ-AUTO.CLI-002, REQ-AUTO.CLI-005]
covers: []
depends_on: [AUTO.SURF.ENV]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.CLI.OUT: JSON and human output with exit codes

## Purpose

Renders the shared envelope for the terminal: exactly one envelope on stdout under `--json`, a readable summary otherwise, everything else on stderr, and one documented exit code per outcome so shell scripts and CI can branch without parsing text.

## Scope

**In:**
- `--json` rendering and stdout discipline
- Human-readable rendering of envelopes
- The exit-code table and its generated documentation page

**Out:**
- Progress display (`AUTO.CLI.JOB`)
- The envelope itself (`AUTO.SURF.ENV`)

## Requirements

- **REQ-AUTO.CLI.OUT-001** When `--json` is given, the CLI shall write exactly one envelope as one line of RFC 8785 JSON followed by a newline to stdout, and shall write logs, progress and warnings only to stderr.
- **REQ-AUTO.CLI.OUT-002** The CLI shall map outcomes to exit codes as: 0 committed, computed or dry_run; 1 internal error; 2 usage error; 3 refused by side-effect policy; 4 command failed; 5 cancelled; 6 `not_run` timeout; 7 `no_reply` timeout; 8 request filed for a human.
- **REQ-AUTO.CLI.OUT-003** The exit-code table shall be defined once in code and the CLI reference page listing it shall be generated from that definition.
- **REQ-AUTO.CLI.OUT-004** Without `--json`, the CLI shall print the outcome, the journal entry hash when committed and the error code and message when failed.

## Acceptance criteria

- **AC-1** Given fixture command `demo.show_thing@1` (R0) on the fake bus, when `fid demo show-thing --json` runs, then stdout is exactly one line that parses as an envelope with outcome `computed`, and exit code is 0.
- **AC-2** Given the fake bus answering `policy_refused`, then exit code is 3 and stdout under `--json` holds one envelope with outcome `failed` and code `policy_refused`.
- **AC-3** Given fake-bus scripts for each outcome in the table, then each exit code matches REQ-AUTO.CLI.OUT-002.
- **AC-4** Given a run that emits warnings and progress under `--json`, then stdout still contains exactly one line.
- **AC-5** Given `cargo xtask gen-surfaces`, then `docs/cli/exit-codes.md` is generated and equals the table in code.

## Design

- **Registry command(s):** none.
- **Data structures:** `enum ExitCode` with `#[repr(u8)]` and a `from_envelope(&Envelope) -> ExitCode` function; the table is the single source for docs.
- **Algorithm:** a single `Output` sink that holds stdout until the final envelope is ready; everything else goes to stderr.
- **Errors:** an encoding failure becomes exit 1 with an envelope `internal_error` written to stderr.
- **Determinism:** D0 for the rendering function.
- **Performance budget:** negligible.
- **Crate:** `fid-cli`

## Test fixtures

Fake bus scripted per outcome; fixture manifest `fixtures/manifest/min-classes.json`.

## Build prompt

> In crate `fid-cli`, implement output rendering. With `--json`, write exactly one RFC 8785 envelope line to stdout and nothing else; logs, progress and warnings go to stderr. Without it, print outcome, journal entry hash (committed) or error code and message (failed). Exit codes: 0 committed/computed/dry_run, 1 internal, 2 usage, 3 policy refusal (including `raw_path_refused` and M1/S1/A0 refusals), 4 failed, 5 cancelled, 6 not_run, 7 no_reply, 8 request filed. Define the table once as an enum and generate `docs/cli/exit-codes.md` from it in the surface generation step. Acceptance: one-line stdout under `--json` even with warnings; exit codes per outcome; generated table matches. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`.

## Open questions

- None.

## Children

_Leaf: none._
