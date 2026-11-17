---
id: ALN.QA.REPORT
title: Residual report type, statistics and read command
level: 3
parent: ALN.QA
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.QA-001, REQ-ALN.QA-005, REQ-ALN.QA-007]
covers: []
depends_on: [ALN.MODEL.ELEM, KRN.NUM]
side_effect: R0
determinism: D0
cli: [fid align get-residuals]
mcp: [align_get_residuals]
journal: [align.get_residuals@1]
updated: 2026-11-16
---

# ALN.QA.REPORT: Residual report type, statistics and read command

## Purpose

The one `ResidualReport` every alignment solver writes: per-input residuals, used and rejected counts, RMS and maximum absolute deviation with units, computed with fixed-order reductions so statistics are bit-identical everywhere. It also offers `align.get_residuals@1`, the R0 read that CLI, MCP and reports use, with paging for large inputs.

## Scope

**In:**
- `ResidualReport` type, its builder API for solvers, serialisation and digest
- Statistics: RMS, weighted RMS (when weights exist), maximum absolute value, counts
- `align.get_residuals@1` with summary-only and paged modes

**Out** (and which node owns it instead):
- Conditioning and weak-DoF warnings (`ALN.QA.COND`); convergence history (`ALN.QA.HIST`)
- Solvers' residual definitions (each solver node)
- Charts and colour maps (`RPT`, `VIZ`)

## Requirements

- **REQ-ALN.QA.REPORT-001** The report shall hold, per input entry, the input reference, the entry kind (`point`, `feature`, `pair` or `locked_direction`), the signed scalar residual in metres, the residual vector when defined, the weight when given, and a `used` flag with a rejection reason when not used.
- **REQ-ALN.QA.REPORT-002** The report shall hold RMS over used entries, weighted RMS when weights exist, maximum absolute residual with its entry, number used and number rejected, all with units.
- **REQ-ALN.QA.REPORT-003** Sums shall be computed by a fixed pairwise reduction tree over entries in index order (block size 64), so identical inputs give bit-identical statistics on every platform.
- **REQ-ALN.QA.REPORT-004** If a solver adds a NaN or infinite residual, then the builder shall fail with `nan_residual` naming the entry.
- **REQ-ALN.QA.REPORT-005** `fid align get-residuals <alignment> --json` and the MCP tool `align_get_residuals` shall return identical content, as R0 reads recorded only in the Activity Log.
- **REQ-ALN.QA.REPORT-006** The read shall support `summary_only` and paging (`offset`, `limit` at most 100,000 entries per page), and the summary shall always include the report digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/qa/report-cases.json` (residual lists with hand-computed statistics), when reports are built, then RMS, weighted RMS, maximum, used and rejected counts equal the expected values exactly.
- **AC-2** Given a 1,000,000-entry report built on two platforms, then the statistics and report digest are bit-identical.
- **AC-3** (error path) Given a NaN residual, then the builder fails with `nan_residual`.
- **AC-4** Given an alignment from fixture `fixtures/aln/model/chain3`, when the CLI and MCP reads run, then their payloads are identical after canonicalisation and the Record Journal is unchanged.
- **AC-5** Given the 1,000,000-entry report, when read with `limit 100000` and `offset 900000`, then exactly the last 100,000 entries are returned; `limit 100001` fails with `page_too_large`.

## Design

- **Registry command:** `align.get_residuals@1` (R0). Params: `alignment`, `summary_only: bool`, `offset`, `limit`. Result: `ResidualSummary` plus optional `entries`.
- **Data structures:** `ResidualReport { id, alignment_id, entries: columnar arrays (refs, kinds, scalars, vectors, weights, used, reasons), stats: { rms, weighted_rms?, max_abs, max_entry, n_used, n_rejected, unit: m }, digest }`. `ReportBuilder::push(...)`, `ReportBuilder::finish() -> Result<ResidualReport, ReportError>`.
- **Algorithm:** pairwise tree sum of squares in blocks of 64 in index order; sqrt via fid-num; max by total-order comparison with lowest index on ties. Digest = SHA-256 over canonical JSON of stats plus SHA-256 of the columnar arrays' little-endian bytes.
- **Errors:** `nan_residual`, `not_found`, `page_too_large`.
- **Determinism:** D0 under KNP-1.
- **Performance budget:** build statistics for 1,000,000 entries in under 20 ms [unverified until measured].
- **Crate:** `fid-align-solve` (type and statistics) and `fid-align` (command); provisional names.

## Test fixtures

`fixtures/aln/qa/report-cases.json`: hand-authored residual lists with exact expected statistics (chosen as binary-representable values); a seeded 1,000,000-entry generator (seed 13); and `fixtures/aln/model/chain3`. Project-authored, CC-BY-4.0.

## Build prompt

> Implement the alignment residual report: type, builder and statistics in kernel crate `fid-align-solve`; the command `align.get_residuals@1` in `fid-align` (class R0; CLI `fid align get-residuals <alignment> [--summary-only] [--offset N --limit M] --json`; MCP `align_get_residuals`; Activity Log only).
> Requirements: per entry: input reference, kind (point, feature, pair, locked_direction), signed scalar residual in metres, optional vector, optional weight, used flag and rejection reason. Statistics over used entries: RMS, weighted RMS when weights exist, max absolute value with its entry, used and rejected counts, unit m. Sums by a fixed pairwise reduction tree in index order (block size 64); sqrt through fid-num; ties on max resolved by lowest index. NaN or infinite residual gives `nan_residual`. Paging limit at most 100,000 (`page_too_large`); the summary always carries the report digest (SHA-256 of canonical stats plus the columnar arrays' bytes).
> Acceptance: fixture `fixtures/aln/qa/report-cases.json` for exact statistics; seeded 1,000,000-entry report bit-identical across platforms; NaN rejection; CLI and MCP payloads identical and Record Journal unchanged (using `fixtures/aln/model/chain3`); paging.
> Constraints: KNP-1 (no FMA, fid-num sqrt, fixed-order reductions, total-order max, no std HashMap/HashSet, NaN is an error); no panic, no unwrap/expect, no unsafe; determinism D0.

## Open questions

- Whether UNC.XFM consumes the residual report directly stays with UNC's expansion (parent question).

## Children

_Leaf: none._
