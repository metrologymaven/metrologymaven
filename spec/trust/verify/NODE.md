---
id: TRUST.VERIFY
title: fid-verify binary and verification report
level: 2
parent: TRUST
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST-002, REQ-TRUST-003]
covers: []
depends_on: [TRUST.PKG.READER]
uses_artifacts: [TRUST.VERIFY.REPORT]
cli: [fid verify package, fid-verify, fid-verify mcp]
mcp: [verify_package, verify_list_files]
journal: []
updated: 2026-11-17
---

# TRUST.VERIFY: fid-verify binary and verification report

## Purpose

The standalone, free and open `fid-verify` program that anyone can run on a Result Package. It composes the signature, chain and replay checks into one run, emits a stable machine-readable verification report and a human summary, and exposes the same checks as a local stdio MCP server and as `fid verify` in the main CLI. It exists separately because it is a distinct binary with its own distribution and licence guarantees (C8).

## Scope

**In:**
- The `fid-verify` binary: arguments, exit codes, `--json` report
- `fid verify *` subcommands in the main CLI, generated from the same registry entries
- Local stdio MCP server for `fid-verify` (read and compute tools only)
- Verification report schema: per check, per result, overall verdict
- Ordering and composition of checks; fail-fast versus full-report mode
- Offline operation and no licence check
- Minimal dependency footprint and reproducible builds for the verifier
- A browser build of the verifier (WASM) for drag-and-drop checking without install

**Out** (and which node owns it instead):
- Signature cryptography (`TRUST.SIGN`)
- Chain and checkpoint logic (`TRUST.CHAIN`)
- Replay logic (`TRUST.REPLAY`)
- Packaging, installers and release signing of the binary (`ECO`)
- The generic MCP and CLI generators (`AUTO`)

## Requirements

- **REQ-TRUST.VERIFY-001** `fid-verify` shall be distributed under the project's open-source licence for every outcome of D-001 and shall contain no licence check, account check or activation step.
- **REQ-TRUST.VERIFY-002** `fid-verify` shall complete all checks with networking disabled, and a CI test shall run it in a sandbox with no network interface.
- **REQ-TRUST.VERIFY-003** When given a package, `fid-verify` shall run signature, chain and (when points are present) replay checks and report each as a separate section.
- **REQ-TRUST.VERIFY-004** The verifier shall exit 0 only when all signatures are valid and trusted, the chain is `VALID`, no result is `UNLINKED`, and the replay outcome is `IDENTICAL` or `EQUIVALENT` or replay was not requested; it shall use distinct documented non-zero codes otherwise.
- **REQ-TRUST.VERIFY-005** With `--json`, the verifier shall emit a report that validates against a published, versioned JSON Schema.
- **REQ-TRUST.VERIFY-006** The `fid-verify` MCP server shall expose only R0 and R1 tools and shall read packages only through handles to roots the user passed on the command line.
- **REQ-TRUST.VERIFY-007** If the input is malformed, truncated or over the format limits, then the verifier shall exit with a typed error code and never panic, demonstrated by a fuzz target in CI.
- **REQ-TRUST.VERIFY-008** The verifier's release binaries shall be reproducible: two independent builds from the same tag shall produce byte-identical outputs on at least Linux x86-64 [unverified].
- **REQ-TRUST.VERIFY-009** `fid-verify` shall not depend on GUI, GPU, device or project-store crates, and the layering check shall fail the build if it does.

## Interfaces

- **Offers:** `fid-verify` binary; `verify.package` (R1); stdio MCP tools `verify_package`, `verify_check_signature`, `verify_check_chain`, `verify_replay`, `verify_inspect_package`; the verification report schema.
- **Consumes:** `TRUST.PKG` reader, `TRUST.SIGN` signature checks, `TRUST.CHAIN` chain checks, `TRUST.REPLAY` replay; registry manifest generation from `AUTO`.

## Decomposition hints

CLI (args, exit codes, human summary), REPORT (schema + JSON), MCP (stdio server), WEB (browser build), REPRO (reproducible build + layering guard).

## Split rationale

The split separates **the contract** (REPORT: schema, verdict, exit codes) from **each delivery surface** (CLI binary and registry command, stdio MCP server, browser build) and from **the evidence that the binary itself can be trusted** (REPRO). Every surface renders the same report, so parity tests are simple equality checks against the CLI.

Alternatives considered:
- **One leaf per surface with its own verdict logic.** Rejected: exit codes and verdict rules would drift.
- **Browser build inside CLI.** Rejected: it has its own shell, streaming and memory limits, and starts later (P1b).

## Open questions

- CONSTITUTION §7 lists the verifier under P0 while TRUST is tagged P1a; opened as D-025 (recommendation: move the chain-only path to P0).
- `fid-verify` is a separate binary with a smaller dependency set (CONSTITUTION §2 names it); `TRUST.VERIFY.REPRO` enforces the crate allowlist.
- `TRUST.VERIFY.WEB` uses VIZ.WEB conventions without a `depends_on`, because VIZ.WEB depends on TRUST and the reverse edge would form a cycle.
- Dependencies narrowed (2026-11-17, D-034): each child names the check leaves it composes; the report schema (`TRUST.VERIFY.REPORT`) is read by MCP, WEB and REPRO as an artifact. `TRUST.VERIFY.MCP` follows AUTO.MCP conventions as artifacts (AUTO is L3).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| TRUST.VERIFY.REPORT | Verification report schema, verdict and exit codes | 003 004 005 | none |
| TRUST.VERIFY.CLI | fid-verify binary and verify-package command | 001 002 003 007 | none |
| TRUST.VERIFY.MCP | fid-verify local stdio MCP server | 006 | none |
| TRUST.VERIFY.WEB | In-browser verifier | 002 | none |
| TRUST.VERIFY.REPRO | Verifier reproducible build and layering guard | 001 008 009 | none |
