---
id: UNC.RDL
title: Reference data library and kernel validation
level: 2
parent: UNC
kind: subsystem
status: open
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

## Interfaces

- **Offers:** `RdlDataset` format; `rdl.list_dataset@1` (R0), `rdl.generate_dataset@1` (R1), `rdl.run_selftest@1` (R1, record W2 journal `rdl.run_selftest`), `rdl.submit_tracim@1` (X1); frozen release-set digest for the kernel manifest.
- **Consumes:** kernel fits and manifest from KRN; signatures from TRUST for distributed subsets.

## Decomposition hints

FORMAT, EXT (tier X import), GEN (generators), FREEZE (release sets), AST (self-test runner and point-of-use), TRACIM (client), BLIND (custody and challenge handling), LINT.

## Open questions

- **Owner assigned (level-2 review, accepted 2026-11-16):** UNC.RDL owns the REP-1 replay equivalence-tolerance table that CORE.RPL and TRUST.REPLAY consume. Include it when this node is expanded.
- Licence terms for redistributing NIST ATS and PTB datasets are unverified; tier X redistribution waits on confirmation.
- Whether automated CI submission of paid TraCIM orders is acceptable is to be asked of PTB [unverified].

## Children

_None yet: status `open`._
