---
id: PLAT.MATRIX.POLICY.EOL
title: End-of-support list, warning and demotion check
level: 4
parent: PLAT.MATRIX.POLICY
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-PLAT.MATRIX.POLICY-004, REQ-PLAT.MATRIX.POLICY-005, REQ-PLAT.MATRIX.POLICY-006]
covers: []
depends_on: [PLAT.MATRIX.FILE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-platform, fid-xtask]
updated: 2026-11-22
---

# PLAT.MATRIX.POLICY.EOL: End-of-support tracking

## Purpose

Keeps a dated, sourced list of vendor end-of-support dates for every OS version listed at Tier 1 or Tier 2, warns 90 days ahead, and blocks a release that still claims Tier 1 for a version past its date. For Windows 10, D-010 keeps it Tier 1 while enterprise extended security updates last, so its effective date is `extended_until`.

## Scope

**In:**
- `platform/os-eol.toml`: `{ os, version, eol, extended_until?, source }` per listed Tier 1 or Tier 2 version
- `xtask plat-matrix eol-check --today <date>`: coverage, warnings, demotion verdict
- The demotion note format `demoted: <date> eol` that `PLAT.MATRIX.POLICY.ROWS` recognises

**Out** (and which node owns it instead):
- Editing rows (a reviewed PR by maintainers; `PLAT.MATRIX.POLICY.ROWS` checks the window)

## Requirements

- **REQ-PLAT.MATRIX.POLICY.EOL-001** If an OS version listed at Tier 1 or Tier 2 has no entry in `os-eol.toml`, then the check shall fail naming it.
- **REQ-PLAT.MATRIX.POLICY.EOL-002** The effective end date of an entry shall be `extended_until` when present and `eol` otherwise.
- **REQ-PLAT.MATRIX.POLICY.EOL-003** When `today` is within 90 days before an entry's effective end date, the check shall emit a warning naming the row and date, and exit 0 [unverified: lead time].
- **REQ-PLAT.MATRIX.POLICY.EOL-004** When `today` is after an entry's effective end date and its row is still effective Tier 1 without a `demoted:` note, the check shall exit 1 with `PastEndOfSupport` naming the row, so the release cannot ship until the row is demoted.
- **REQ-PLAT.MATRIX.POLICY.EOL-005** Every entry shall carry a `source` URL, and the shipped dates shall be marked unverified in a comment until a maintainer checks them.
- **REQ-PLAT.MATRIX.POLICY.EOL-006** The check shall take `today` as an explicit argument (CI passes the release date) so its output is reproducible.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/matrix/eol-valid.toml`, `fixtures/plat/matrix/valid.toml` and `--today 2026-01-01`, when `eol-check` runs, then it exits 0 with no warnings.
- **AC-2** Given an entry ending 2026-03-01 and `--today 2026-01-15`, then a warning names that row and it exits 0.
- **AC-3** (error path) Given `--today 2026-03-02` with that row still Tier 1, then it exits 1 with `PastEndOfSupport`; with a `demoted: 2026-03-02 eol` note, it exits 0.
- **AC-4** Given a Windows 10 entry with `eol` passed but `extended_until` in the future, then no failure.
- **AC-5** (error path) Given a Tier 2 version without an entry, then `MissingEolEntry`.

## Design

- **Registry command(s):** none; data and `xtask plat-matrix eol-check --today <YYYY-MM-DD> [--matrix <file>] [--eol <file>]`.
- **Data structures:** `OsEol { schema_version, entries: Vec<{ os, version, eol: Date, extended_until: Option<Date>, source: Url }> }`.
- **Errors:** `MissingEolEntry`, `MissingSource`, `PastEndOfSupport`, parse errors.
- **Determinism:** D0 (no system clock).
- **Crate:** `fid-platform` (module `os_eol`) plus `xtask`.

## Test fixtures

Hand-written TOML under `fixtures/plat/matrix/` with invented dates. Licensed with the code.

## Build prompt

> In crate `fid-platform`, add module `os_eol` and `xtask plat-matrix eol-check --today <date>`: parse `platform/os-eol.toml` (os, version, eol, optional extended_until, source URL; 64 KiB cap); require an entry for every Tier 1 or Tier 2 version in the matrix; effective end = extended_until or eol; warn within 90 days before; fail `PastEndOfSupport` when past and the row is still effective Tier 1 without a `demoted: <date> eol` note. Ship entries for the P1a rows with vendor source URLs and a comment marking dates unverified; Windows 10 gets `extended_until` for enterprise extended updates per D-010. No system clock. Tests: AC-1..AC-5. Constraints: CONSTITUTION §8. Commit: `feat(fid-platform): OS end-of-support check`.

## Open questions

- Windows 10 extended-update end date for enterprises must be checked on the vendor page [unverified].

## Children

_Leaf: none._
