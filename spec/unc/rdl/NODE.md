---
id: UNC.RDL
title: Reference data library and kernel validation
level: 2
parent: UNC
kind: subsystem
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-UNC-005]
covers: []
depends_on: [KRN]
cli: [fid rdl *]
mcp: [rdl_*]
journal: [rdl.*]
updated: 2026-11-16
---

# UNC.RDL: Reference data library and kernel validation

## Purpose

The **Reference Data Library**: a versioned, open, content-addressed collection of reference datasets (inputs plus reference outputs) used to validate the kernel's fits and evaluations. It holds external datasets (tier X), generated datasets with high-precision references (tier G), and the record format and runner shared with the open suites (tier O, content owned by `UNC.ORS`). It owns synthetic generators, the frozen release set whose digest enters kernel identity, the algorithm self-test runner, blind-challenge handling and the PTB TraCIM client. It is separate because it is a data and validation product used by every kernel release.

## Scope

**In:**
- RdlDataset record format (aim, inputs, reference outputs, test uncertainty, generator, cross-checks, provenance, licence, visibility)
- Tier X external datasets (NIST, PTB) subject to licence confirmation
- Tier G generators: null-space construction for least squares, constructed equioscillation sets for minimum zone, partial features, extreme aspect ratios, noise and outlier stress
- Tier B blind-set handling (held outside the main repository)
- Frozen release sets and `rdl_suite_digest`
- Algorithm self-test runner: comparison against reference with test uncertainty and MPE
- Point-of-use subset run with a journaled record
- PTB TraCIM client (public test and order submission)
- Lint that forbids RDL identifiers or constants in kernel code

**Out** (and which node owns it instead):
- GD&T, DRF, size, alignment and uncertainty-coverage suite content (owned by `UNC.ORS`)
- Independent oracle implementations (separate repository, referenced by provenance)
- Signing run records (owned by `TRUST`)
- The fits being tested (owned by `KRN`)

## Requirements

- **REQ-UNC.RDL-001** Every RDL dataset shall be identified by tier, aim and a SHA-256 content digest, and shall record its licence and provenance.
- **REQ-UNC.RDL-002** If a dataset's licence permits no redistribution, then the system shall store only its reference and digest, never its content, in public releases.
- **REQ-UNC.RDL-003** Each tier G generator shall be deterministic for a given seed and shall produce reference outputs computed to at least 50 significant digits for coordinates rounded to 16 significant digits.
- **REQ-UNC.RDL-004** Each tier G reference shall be cross-checked by at least one independent oracle, and the cross-check result shall be recorded in the dataset.
- **REQ-UNC.RDL-005** Each kernel release shall be validated against one frozen RDL release set, and its `rdl_suite_digest` shall be recorded in the kernel manifest.
- **REQ-UNC.RDL-006** The self-test shall pass a case only when the absolute deviation plus its test uncertainty is no greater than the internal MPE, set by default to one tenth of the TraCIM default MPEs [unverified].
- **REQ-UNC.RDL-007** The TraCIM client shall submit fit results for public test runs and record the returned verdict with the kernel manifest digest.
- **REQ-UNC.RDL-008** When the point-of-use subset is run, the system shall write a journaled record with kernel version, subset digest and per-case outcome.
- **REQ-UNC.RDL-009** If kernel source references an RDL dataset ID, file name or reference constant, then the lint shall fail the build.
- **REQ-UNC.RDL-010** The RDL shall publish a versioned replay equivalence-tolerance table (REP-1) with bounds per quantity kind and rules per determinism class, together with a classifier that `CORE.RPL` and `TRUST.REPLAY` use, and any change to a bound shall require a new table version.

## Interfaces

- **Offers:** `RdlDataset` format; the REP-1 table and classifier (`fid-rep`); `rdl.list_dataset@1` (R0), `rdl.generate_dataset@1` (R1), `rdl.run_selftest@1` (R1, record W2 journal `rdl.run_selftest`), `rdl.submit_tracim@1` (X1); frozen release-set digest for the kernel manifest.
- **Consumes:** kernel fits and manifest from KRN; signatures from TRUST for distributed subsets.

## Decomposition hints

FORMAT, EXT (tier X import), GEN (generators), FREEZE (release sets), AST (self-test runner and point-of-use), TRACIM (client), BLIND (custody and challenge handling), LINT.

## Open questions

- **Owner assigned (level-2 review, accepted 2026-11-16):** UNC.RDL owns the REP-1 replay equivalence-tolerance table that CORE.RPL and TRUST.REPLAY consume. Incorporated at expansion as REQ-UNC.RDL-010 and child `UNC.RDL.REP`.
- Licence terms for redistributing NIST ATS and PTB datasets are unverified; tier X redistribution waits on confirmation.
- Whether automated CI submission of paid TraCIM orders is acceptable is to be asked of PTB [unverified].

## Split rationale

Split by **pipeline stage of reference data**: the record format every tier shares (FORMAT); the three ways data enters (EXT for external NMI data, GEN for generated data with oracle cross-checks, BLIND for sealed sets and challenges); freezing a release whose digest enters kernel identity (FREEZE); running cases and recording outcomes (AST); the external certification client (TRACIM); the data side of the anti-overfitting lint (LINT); and the replay equivalence policy (REP). REQ-UNC.RDL-010 was added at expansion to carry the level-2 review's assignment of the REP-1 table to this node; it is owned by REP.

Alternatives considered:
- **REP-1 in KRN.MAN or CORE.RPL.** Rejected by the level-2 review, which assigned it here; it is also data-like (a versioned table) and its tolerances derive from RDL's MPE conventions.
- **LINT owned only by KRN.NUM.** Kept split: KRN.NUM owns the lint engine (REQ-KRN.NUM-009); RDL owns the denylist and a canary that proves the lint bites.
- **Point-of-use run as its own child.** Rejected: it is the same runner on a subset; only the selection differs.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.RDL.FORMAT | RDL dataset record format and listing (leaf) | 001 002 | none |
| UNC.RDL.EXT | Tier X external dataset import | 001 002 | none |
| UNC.RDL.GEN | Tier G synthetic generators and oracle cross-checks | 003 004 | none |
| UNC.RDL.FREEZE | Frozen RDL release sets and suite digest (leaf) | 005 | none |
| UNC.RDL.AST | Algorithm self-test runner and point-of-use run (leaf) | 006 008 | none |
| UNC.RDL.TRACIM | PTB TraCIM client | 007 | none |
| UNC.RDL.BLIND | Tier B blind sets and challenge handling | 002 | none |
| UNC.RDL.LINT | Kernel reference-data denylist (leaf) | 009 | none |
| UNC.RDL.REP | Replay equivalence tolerance table (REP-1) and classifier (leaf) | 010 | none |
