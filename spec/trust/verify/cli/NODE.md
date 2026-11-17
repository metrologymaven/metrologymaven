---
id: TRUST.VERIFY.CLI
title: fid-verify binary and verify-package command
level: 3
parent: TRUST.VERIFY
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.VERIFY-001, REQ-TRUST.VERIFY-002, REQ-TRUST.VERIFY-003, REQ-TRUST.VERIFY-007]
covers: []
depends_on: [TRUST.VERIFY.REPORT, TRUST.PKG.READER]
side_effect: R1
determinism: D0
cli: [fid verify package, fid-verify]
mcp: [verify_package]
journal: [verify.package@1]
updated: 2026-11-17
---

# TRUST.VERIFY.CLI: fid-verify binary and verify-package command

## Purpose

The `fid-verify` executable and the `verify.package@1` registry command it shares with `fid verify package`. It parses arguments, opens the package, runs the composed checks, prints the human summary or JSON report, and exits with the code from `TRUST.VERIFY.REPORT`. It has no licence check, no account and no network code path.

## Scope

**In:**
- `fid-verify <package> [--trust-root F] [--revoked F] [--replay] [--rep1 F] [--fail-fast] [--json] [--memory-limit N] [--time-budget D]`
- `fid-verify mcp` subcommand entry (server itself in `TRUST.VERIFY.MCP`)
- `verify.package@1` registry entry for the main `fid` CLI, generated from the same manifest
- Offline guarantee test and fuzz target on the whole pipeline
- `--version` printing tool version, report version and supported package format versions

**Out:**
- Report composition (`TRUST.VERIFY.REPORT`)
- MCP server (`TRUST.VERIFY.MCP`)
- Browser build (`TRUST.VERIFY.WEB`)
- Release packaging and installers (`ECO`)

## Requirements

- **REQ-TRUST.VERIFY.CLI-001** `fid-verify` shall contain no licence, account, activation or telemetry code path, checked by a CI test that the binary has no network-capable dependency in its dependency tree.
- **REQ-TRUST.VERIFY.CLI-002** When run on a golden package in a sandbox with no network interface, `fid-verify` shall produce the same report as with networking enabled.
- **REQ-TRUST.VERIFY.CLI-003** When `--replay` is given and the package has a points tier, the command shall run the replay section; without `--replay` it shall report replay `not_requested`.
- **REQ-TRUST.VERIFY.CLI-004** If the package is malformed, truncated or over limits, then `fid-verify` shall print a typed error and exit 64, and a fuzz target over the full command shall show no panic in CI.
- **REQ-TRUST.VERIFY.CLI-005** `verify.package@1` in the main CLI shall produce a report byte-identical to `fid-verify --json` for the same inputs.
- **REQ-TRUST.VERIFY.CLI-006** `fid-verify` shall accept file paths on its own command line (a human or script invoking the standalone tool) and shall open them read-only.

## Acceptance criteria

- **AC-1** Given `fixtures/pkg/golden/points.zip` and `fixtures/troot/both.json`, when `fid-verify points.zip --trust-root both.json --replay --json` runs, then exit code 0 and the report verdict is `PASS` with replay `IDENTICAL`.
- **AC-2** Given AC-1 in a CI container started with `--network none`, then the report is byte-identical to AC-1.
- **AC-3** (error path) Given every file in `fixtures/pkg/malformed/`, then exit 64 with the expected error code on stderr.
- **AC-4** Given AC-1 inputs to `fid verify package --json`, then the report equals AC-1's.
- **AC-5** (fuzz) Given the whole-pipeline fuzz target run 10 minutes in CI, then no crash or hang.
- **AC-6** Given `cargo tree` for `fid-verify`, then no crate from the network deny-list (HTTP, TLS, DNS clients) appears.

## Design

- **Registry command(s):** `verify.package@1`, R1; params `package`, `trust_root?`, `revoked?`, `replay: bool`, `rep1?`, `fail_fast: bool`, `memory_limit?`, `time_budget?`; result `VerificationReport`. Activity Log only when run inside `fid`; the standalone binary writes no journal.
- **Algorithm:** parse args → open package read-only via `fid-pkg::reader` → signatures → chain → replay (if requested) → report → print → exit.
- **Errors:** exit codes from `TRUST.VERIFY.REPORT`.
- **Determinism:** D0.
- **Performance budget:** a 100 MB results-and-features package fully verified (no replay) in under 3 s [unverified until measured].
- **Crate:** `fid-verify` (binary `fid-verify`; library shared with `fid` CLI registration).

## Test fixtures

- Golden and malformed packages (`TRUST.PKG.FIXTURE`), trust roots (`TRUST.SIGN.TROOT`), test kernel (`KRN.MAN`).

## Build prompt

> Build the `fid-verify` binary in crate `fid-verify` and register `verify.package@1` (R1; CLI `fid verify package`; MCP `verify_package`). Arguments: `<package> [--trust-root F] [--revoked F] [--replay] [--rep1 F] [--fail-fast] [--json] [--memory-limit N] [--time-budget D]`, plus `--version` and a `mcp` subcommand hook. Open inputs read-only; run signatures (`fid-sign-core::troot`), chain (`fid-chain::links`), replay when `--replay` (`fid-vreplay::run`), compose with `report`, print summary or JSON, exit with the report's code (64 malformed, 65 usage, 70 internal). No licence, account, activation, telemetry or network code; add a CI dependency deny-list check and a `--network none` container test. Add a whole-pipeline fuzz target. Tests: the six acceptance criteria. Constraints: CONSTITUTION C7, C8, §8 (never panic), D0; must not depend on GUI, GPU, device or project-store crates.

## Open questions

- A P0 chain-only verifier is proposed in the phase decision referenced from `TRUST.VERIFY`.

## Children

_Leaf: none._
