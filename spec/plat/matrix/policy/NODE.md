---
id: PLAT.MATRIX.POLICY
title: Tier definitions, Tier 1 set and end-of-life policy
level: 3
parent: PLAT.MATRIX
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.MATRIX-005, REQ-PLAT.MATRIX-006]
covers: []
depends_on: [PLAT.MATRIX.FILE]
uses_artifacts: [PLAT.MATRIX.CI]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.MATRIX.POLICY: Tier policy and matrix content

## Purpose

Owns what the support matrix *says*: the written tier definitions, the actual rows (which OS versions and architectures get which tier from which phase), promotion and demotion rules, and end-of-life tracking for listed OS versions. FILE owns the format; this node owns the content and the rules for changing it.

## Scope

**In:**
- Tier definitions document (obligations per tier)
- The rows of `platform/support-matrix.toml`, including the P1a Tier 1 set and the ARM64 rows
- Promotion and demotion rules, including demotion at vendor end of support
- A vendor end-of-support date list per listed OS version and a CI warning ahead of each date

**Out** (and which node owns it instead):
- File schema, validator and lookup (`PLAT.MATRIX.FILE`)
- Job generation and release gate (`PLAT.MATRIX.CI`)
- The generated page (`PLAT.MATRIX.PAGE`)
- Browser tiers (`PLAT.WEB.BROWSERS`)

## Requirements

- **REQ-PLAT.MATRIX.POLICY-001** The project shall publish tier definitions stating, for each of Tier 1, Tier 2 and Tier 3, the CI obligation, whether a failure blocks a release and the expected support response.
- **REQ-PLAT.MATRIX.POLICY-002** From phase P1a, the matrix file shall list Windows 10 and Windows 11 on x86-64, the Ubuntu LTS releases inside the Tier 1 window and the macOS releases inside the Tier 1 window on ARM64 as Tier 1 rows, with the window set by D-010.
- **REQ-PLAT.MATRIX.POLICY-003** The matrix file shall contain rows for Linux ARM64 and Windows ARM64 with the tier set by D-010, and Tier 3 until D-010 is decided.
- **REQ-PLAT.MATRIX.POLICY-004** The project shall keep a machine-readable list of vendor end-of-support dates for every OS version listed at Tier 1 or Tier 2.
- **REQ-PLAT.MATRIX.POLICY-005** When a Tier 1 OS version's vendor end-of-support date has passed, the next release shall demote it out of Tier 1 and record the date and reason in the row notes [unverified: release-cycle length not yet set].
- **REQ-PLAT.MATRIX.POLICY-006** When a listed OS version is within 90 days of its end-of-support date, CI shall emit a warning naming the row [unverified: lead time to be tuned].
- **REQ-PLAT.MATRIX.POLICY-007** A combination shall be promoted to Tier 1 only after its Tier 1 jobs have passed on at least two consecutive release candidates [unverified: count to be tuned].

## Interfaces

- **Offers:** `docs/platform/tiers.md`; the matrix rows; `platform/os-eol.toml` (os, version, vendor end-of-support date, source URL, `extended_until` for paid extended updates); `xtask plat-matrix eol-check` and `xtask plat-matrix promote-check`.
- **Consumes:** `PLAT.MATRIX.FILE` schema and lookup. Artifact: job result history of `PLAT.MATRIX.CI` (gate verdict files per release candidate).

## Decomposition hints

See Children.

## Open questions

- D-010 is accepted (option B): the Tier 1 window is the latest two Ubuntu LTS releases and the latest two macOS major releases; Linux ARM64 is Tier 2 and Windows ARM64 Tier 3; Windows 10 stays Tier 1 while Microsoft's extended security updates are available to enterprises [unverified], then is demoted by the EOL rule. Applied in `PLAT.MATRIX.POLICY.ROWS` and `PLAT.MATRIX.POLICY.EOL`.
- Vendor end-of-support dates must be cited from vendor pages and marked [unverified] until checked.

## Split rationale

Split by **artefact**: a definitions document (DEFS), the matrix rows with their promotion check (ROWS), and the end-of-support list with its warning and demotion checks (EOL). Rows change with each OS release; the EOL list changes with vendor announcements; definitions rarely change.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.MATRIX.POLICY.DEFS | Tier definitions document and its lint | 001 | none |
| PLAT.MATRIX.POLICY.ROWS | Matrix rows per D-010 and the promotion check | 002 003 007 | none |
| PLAT.MATRIX.POLICY.EOL | End-of-support list, warning and demotion check | 004 005 006 | none |
