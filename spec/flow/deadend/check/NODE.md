---
id: FLOW.DEADEND.CHECK
title: Fix-catalogue consistency check against the registry manifest
level: 3
parent: FLOW.DEADEND
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-FLOW.DEADEND-001, REQ-FLOW.DEADEND-002]
covers: [JOB.USE.05]
depends_on: [FLOW.DEADEND.CATALOG, CORE.REG.MAN, CORE.BUS.PIPE]
uses_artifacts: []
side_effect: R1
determinism: D0
cli: [fid accept check-fixes]
mcp: [accept_check_fixes]
journal: [accept.check_fixes@1]
updated: 2026-11-17
---

# FLOW.DEADEND.CHECK: Fix-catalogue consistency check against the registry manifest

## Purpose

The CI gate that makes "no dead ends" enforceable: every reason code a command can emit must have a catalogue entry, and every fix in the catalogue must name a command that exists with parameters it accepts. A new reason code without a fix fails the build.

## Scope

**In:**
- `accept.check_fixes@1` (R1) over the in-process registry manifest and the shipped catalogue
- Reason-code discovery from the manifest's result schemas
- Findings report and exit codes; use in CI

**Out:**
- The catalogue file and loader (owned by `FLOW.DEADEND.CATALOG`)
- Runtime projection into results (owned by `FLOW.DEADEND.RESULT`)
- The manifest and its schema keywords (owned by `CORE.REG.MAN` and `CORE.REG.DEF`)

## Requirements

- **REQ-FLOW.DEADEND.CHECK-001** The check shall collect declared reason codes as the enum values of every result-schema property annotated `x-reason-code` in the manifest, qualified by the declaring command's domain.
- **REQ-FLOW.DEADEND.CHECK-002** When `fid accept check-fixes` runs, the system shall report `FIX-MISSING <code>` for each declared reason code with no catalogue entry and `FIX-UNKNOWN-COMMAND <code> <command>` for each fix whose command ID and major are absent from the manifest.
- **REQ-FLOW.DEADEND.CHECK-003** The check shall report `FIX-BAD-ARG <code> <command> <arg>` for each fix argument that is not a parameter of the named command's parameter schema.
- **REQ-FLOW.DEADEND.CHECK-004** The check shall report a catalogue entry whose reason code is declared by no command as a warning `FIX-ORPHAN`, not a failure.
- **REQ-FLOW.DEADEND.CHECK-005** The command shall return outcome `failed` with exit code 1 when any non-warning finding exists, list findings sorted by code then reason code, and include the manifest digest and catalogue digest in its result.

## Acceptance criteria

- **AC-1** Given fixture manifest `fixtures/flow/fixes/manifest-ok.json` (3 commands declaring 4 reason codes) and `catalogue-ok.json`, when `fid accept check-fixes --json` runs against them in the test harness, then the outcome is `computed`, findings are empty and exit code is 0.
- **AC-2** (error path) Given `manifest-newcode.json` (adds `scan.no_overlap` with no entry), when the check runs, then it exits 1 with `FIX-MISSING scan.no_overlap`.
- **AC-3** (error path) Given `catalogue-badcmd.json` (fix `decide.bind_rules@1`), then it exits 1 with `FIX-UNKNOWN-COMMAND`.
- **AC-4** Given `catalogue-orphan.json`, then it exits 0 and reports one `FIX-ORPHAN` warning.
- **AC-5** (determinism) Running AC-2 twice gives byte-identical `--json` results.

## Design

- **Registry command(s):** `accept.check_fixes@1`, R1, no parameters in production (the live manifest and the build-embedded catalogue); test builds may inject fixtures. Result `{manifest_digest, catalogue_digest, findings:[{severity, code, reason_code, command?, arg?}]}`.
- **Algorithm:** walk each command's result schema (bounded depth 64), collect `x-reason-code` enums; set differences against catalogue; resolve each fix command in the manifest; compare `args` keys with the parameter schema's `properties`.
- **Errors:** `CHECK-MANIFEST` (manifest not loadable), `CHECK-CATALOGUE` (loader error passed through).
- **Determinism:** D0.
- **Performance budget:** under 1 s for 2 000 commands [unverified].
- **Crate:** `fid-accept` (L3, FLOW), using `fid-fixes`.

## Test fixtures

Hand-written manifests and catalogues under `fixtures/flow/fixes/` (project-authored).

## Build prompt

> In crate `fid-accept`, add registry command `accept.check_fixes@1` (side-effect R1; CLI `fid accept check-fixes`; MCP `accept_check_fixes`). Load the in-process registry manifest and the `fid-fixes` catalogue. Collect reason codes from enum values of result-schema properties annotated `x-reason-code` (qualify as `<domain>.<value>`). Findings: `FIX-MISSING`, `FIX-UNKNOWN-COMMAND`, `FIX-BAD-ARG` (errors) and `FIX-ORPHAN` (warning); sort by code then reason code; return manifest and catalogue digests; exit 1 on any error finding. Tests for AC-1..AC-5 of spec node FLOW.DEADEND.CHECK using injected fixtures. Follow CONSTITUTION §8: no panic, no `unsafe`, deny `unwrap`/`expect`; bounded schema walking.

## Open questions

- The `x-reason-code` annotation is not yet a project keyword in `CORE.REG.DEF` (REQ-CORE.REG.DEF-002 lists the allowed keywords). Until CORE adds it, the check finds no declared codes and passes vacuously; this is a cross-domain change for CORE.

## Children

_Leaf: none._
