---
id: TRUST.VERIFY.REPORT
title: Verification report schema, verdict and exit codes
level: 3
parent: TRUST.VERIFY
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.VERIFY-003, REQ-TRUST.VERIFY-004, REQ-TRUST.VERIFY-005]
covers: []
depends_on: [TRUST.SIGN.TROOT, TRUST.CHAIN.LINKS, TRUST.REPLAY.RUN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# TRUST.VERIFY.REPORT: Verification report schema, verdict and exit codes

## Purpose

Composes the signature, chain and replay sections into one verification report, computes the overall verdict, maps it to documented exit codes, and renders a short human summary. The report schema is a published, versioned contract that third parties parse, so it is defined once here and reused by the CLI, the MCP server and the browser build.

## Scope

**In:**
- Report JSON schema (versioned, JSON Schema 2020-12)
- Sections: package inventory, signatures, chain, replay (or `not_requested`), known limitations
- Overall verdict `PASS | FAIL | ERROR` and the exit-code mapping
- Fail-fast versus full-report modes
- Plain-text human summary (stable wording, no colour codes when not a TTY)

**Out:**
- The checks themselves (`TRUST.SIGN.TROOT`, `TRUST.CHAIN.LINKS`, `TRUST.REPLAY.RUN`)
- Argument parsing and process exit (`TRUST.VERIFY.CLI`)

## Requirements

- **REQ-TRUST.VERIFY.REPORT-001** The report shall contain separate `signatures`, `chain` and `replay` sections, with `replay.status` equal to `not_requested` or `not_applicable` when replay did not run, and `report_version` as a semantic version.
- **REQ-TRUST.VERIFY.REPORT-002** The verdict shall be `PASS` only when at least one signature is `VALID_TRUSTED` and none is `INVALID`, `REVOKED` or `EXPIRED`, the chain is `VALID`, no record is `UNLINKED` or `MISLINKED`, and replay is `IDENTICAL`, `EQUIVALENT`, `not_requested` or `not_applicable`.
- **REQ-TRUST.VERIFY.REPORT-003** The exit code shall be 0 for `PASS`; for `FAIL` it shall be the bitwise OR of 1 (signatures), 2 (chain or linkage), 4 (replay `DIVERGENT`) and 8 (replay requested but `NOT_REPLAYABLE`); 64 for malformed input, 65 for usage errors and 70 for internal errors.
- **REQ-TRUST.VERIFY.REPORT-004** The JSON report shall validate against the published schema for its `report_version`, enforced by a CI test over every report produced in the test suite.
- **REQ-TRUST.VERIFY.REPORT-005** In fail-fast mode, the composer shall stop after the first failing section and mark later sections `skipped`; in full mode (default), it shall run every section.
- **REQ-TRUST.VERIFY.REPORT-006** The report shall list known limitations that apply, including "signing time is signer-asserted" and "unsigned package" when relevant.

## Acceptance criteria

- **AC-1** Given section results in `fixtures/verify/report/all-pass.json`, when composed, then verdict `PASS`, exit code 0, and the JSON validates against `schema/verify-report-1.json`.
- **AC-2** Given section fixtures for each failure combination in `fixtures/verify/report/combos/`, then each exit code equals the expected bitmask (e.g. signature and replay divergent gives 5).
- **AC-3** Given `fixtures/verify/report/unsigned.json` (no signatures, chain valid), then verdict `FAIL`, exit code 1 and the limitation "unsigned package" is listed.
- **AC-4** Given fail-fast mode and a signature failure, then chain and replay are `skipped`.
- **AC-5** Given the human summary for AC-1 in a non-TTY, then it equals `fixtures/verify/report/all-pass.txt` byte for byte.

## Design

- **Registry command(s):** none (used by `verify.package@1` in `TRUST.VERIFY.CLI`).
- **Data structures:** `VerificationReport { report_version, tool: { name, version }, package: Inventory, signatures: SignatureReport, chain: ChainReport, replay: ReplaySection, verdict, exit_code, limitations: Vec<String> }`.
- **Algorithm:** pure composition over section results; verdict and exit code computed by fixed rules above.
- **Errors:** none of its own.
- **Determinism:** D0 (report contains no timestamps; the CLI may add `checked_at` outside the hashed body).
- **Performance budget:** trivial.
- **Crate:** `fid-verify` (library module `report`).

## Test fixtures

- `fixtures/verify/report/*`: synthetic section results and expected outputs.

## Build prompt

> In the `fid-verify` crate's library, add module `report`. Compose section results (signatures from `fid-sign-core::troot`, chain from `fid-chain::links`, replay from `fid-vreplay::run`) into a `VerificationReport` with `report_version` 1.0.0, publish `schema/verify-report-1.json`, compute verdict (`PASS` only if at least one `VALID_TRUSTED` signature and none invalid, revoked or expired; chain `VALID`; no `UNLINKED`/`MISLINKED`; replay `IDENTICAL`, `EQUIVALENT`, `not_requested` or `not_applicable`) and exit code (0 pass; OR of 1 signatures, 2 chain/linkage, 4 replay divergent, 8 replay not replayable; 64 malformed input, 65 usage, 70 internal). Support fail-fast (later sections `skipped`) and full mode; list applicable limitations. Render a stable plain-text summary. Tests: the five acceptance criteria, plus schema validation of every report produced in tests. Constraints: CONSTITUTION C8 (free verifier), §8, D0.

## Open questions

- Whether an unsigned package with a valid chain should be `PASS` with a warning for Lean users; proposed no (exit 1), revisit with `TRUST.PROFILE`.

## Children

_Leaf: none._
