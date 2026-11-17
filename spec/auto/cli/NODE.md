---
id: AUTO.CLI
title: The fid command-line interface
level: 2
parent: AUTO
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO-001, REQ-AUTO-010]
covers: []
depends_on: [CORE, AUTO.SURF]
cli: [fid *]
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.CLI: The `fid` command-line interface

## Purpose

The `fid` binary's generated command tree: `fid <domain> <verb-object> [args]` for every registered command, with uniform `--json` output, dry run, documented exit codes, progress on a terminal and cancellation on interrupt. It is the human's scripting surface and the reference surface for CI pipelines. Subcommands that host other surfaces (`fid mcp`, `fid ctrl`) are owned by those children; this node owns only the shared binary conventions and the generated per-command tree.

## Scope

**In:**
- Generated command tree from `AUTO.SURF` projections
- Argument parsing for typed parameters, including authored values with units
- `--json` output using the shared envelope; human-readable default output
- `--dry-run` where the command declares it meaningful
- Exit-code table
- Terminal progress display and interrupt-to-cancel
- Shell completion generated from the projection
- Session targeting: run against a project file headless, or against a running app via the control channel

**Out** (and which node owns it instead):
- `fid mcp` subcommand behaviour (`AUTO.MCP`)
- `fid ctrl` subcommand behaviour (`AUTO.CTRL`)
- `fid script` and `fid program` command semantics (`AUTO.PY`, `AUTO.PROG`, `AUTO.REC`, `AUTO.RUN`, `AUTO.SCRIPTED`)
- `fid-verify` (`TRUST`)
- Packaging the binary (`ECO`)

## Requirements

- **REQ-AUTO.CLI-001** The CLI shall expose every registered command as `fid <domain> <verb-object>`, derived from its command ID by the CONSTITUTION §3 rule, with no hand-written command names or aliases.
- **REQ-AUTO.CLI-002** When `--json` is given, the CLI shall write exactly one shared-envelope JSON document to stdout and nothing else to stdout.
- **REQ-AUTO.CLI-003** When `--dry-run` is given to a command that declares dry-run support, the CLI shall report what would change and shall append nothing to the journal.
- **REQ-AUTO.CLI-004** If `--dry-run` is given to a command that does not declare dry-run support, then the CLI shall refuse it with a usage error and execute nothing.
- **REQ-AUTO.CLI-005** The CLI shall exit with distinct, documented codes for at least: success, usage error, refused by side-effect policy, command failed, cancelled, `NOT_RUN` timeout and `NO_REPLY` timeout.
- **REQ-AUTO.CLI-006** When the user interrupts a running command once, the CLI shall request cancellation of its job and report the job's final state; it shall not exit with a success code unless the command committed.
- **REQ-AUTO.CLI-007** The CLI shall parse an authored value with its unit (for example `12.700 mm`) and pass value and unit unchanged to the bus, leaving unit conversion to the single journaled rounding in CORE.
- **REQ-AUTO.CLI-008** If an M1, S1 or A0 command is invoked from the CLI, then the CLI shall not execute it directly and shall direct the user to the approval station or the out-of-band path that command requires.
- **REQ-AUTO.CLI-009** `fid --help` and every `fid <domain> <verb-object> --help` shall be generated from the manifest and shall match the generated docs entry for the same manifest digest.

## Interfaces

- **Offers:** the `fid` binary command tree; exit-code table; completion scripts.
- **Consumes:** `AUTO.SURF` projections, envelope and job semantics; CORE bus; `AUTO.CTRL` client when targeting a running app.

## Decomposition hints

TREE (generated command tree and help), ARGS (typed and unit-bearing parameters), OUT (json and human output, exit codes), JOB (progress and interrupt), TARGET (headless project vs running app), COMP (completions).

## Open questions

- Should an M1 invocation from the CLI create a run-ticket request for the Approval Station (CMM) or simply refuse? This node requires "not executed directly" and leaves the request path to CMM.

## Children

_None yet: status `open`._
