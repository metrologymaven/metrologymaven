---
id: QUAL.EVID.IMPACT
title: Change-impact report against the previous release
level: 3
parent: QUAL.EVID
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-QUAL.EVID-004]
covers: []
depends_on: []
uses_artifacts: [KRN.MAN.MANI, UNC.RDL.AST, UNC.ORS.RUN, TRUST.REPLAY.CORPUS, ECO.REL.PROC.CHANNEL]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.EVID.IMPACT: Change-impact report against the previous release

## Purpose

Tells a regulated customer what changed in the numbers between two releases on the same channel: every algorithm whose implementation digest changed, its RDL self-test and ORS result deltas, and every REP-1 exceedance found by replaying the previous release's corpus on the new kernel. It is the member that upgrade OQ and a customer's change control read first.

## Scope

**In:**
- Diff of the previous and current kernel manifests by algorithm ID: added, removed, implementation digest changed
- RDL deltas per changed algorithm: case counts passed or failed in each release's full RDL run record
- ORS deltas per suite: pass statistics in each release
- REP-1 exceedances: outcomes `DIVERGENT` (and `EQUIVALENT` counts) from replaying the previous release's replay corpus on the current kernel
- Report `change-impact.json` and a Markdown rendering

**Out** (and which node owns it instead):
- Running RDL, ORS or the corpus replay (owned by `UNC.RDL.AST`, `UNC.ORS.RUN`, `TRUST.REPLAY.CORPUS`)
- Choosing the previous release (rule from `ECO.REL.PROC.CHANNEL`: latest earlier release on the same channel)

## Requirements

- **REQ-QUAL.EVID.IMPACT-001** The report shall list every algorithm ID whose implementation digest differs between the previous and current kernel manifests, plus every added and removed algorithm ID.
- **REQ-QUAL.EVID.IMPACT-002** For each listed algorithm, the report shall give the RDL passed and failed case counts in both releases and their differences.
- **REQ-QUAL.EVID.IMPACT-003** The report shall give, for every ORS suite, the pass statistics in both releases and their differences.
- **REQ-QUAL.EVID.IMPACT-004** The report shall list every corpus result whose cross-release replay outcome is `DIVERGENT`, with algorithm ID and characteristic, and the count of `EQUIVALENT` results per algorithm.
- **REQ-QUAL.EVID.IMPACT-005** If the previous-release inputs are absent (first release on a channel), then the report shall state `no_previous_release` and list all algorithms as added.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/evid/impact/minor/` (two manifests where `fit.circle.lsq` changed digest, RDL and ORS records for both, cross-replay outcomes with one DIVERGENT), when `cargo xtask qual evidence impact --previous prev/ --current cur/ --out out/` runs, then `out/change-impact.json` equals the golden file and lists the algorithm and the divergent result.
- **AC-2** Given `unchanged/` (equal digests, no divergence), then the report lists no algorithms and zero exceedances.
- **AC-3** Given `first/` (no previous inputs), then the report states `no_previous_release`.
- **AC-4** (determinism) Given AC-1 twice, then outputs are byte-identical.

## Design

- **Registry command(s):** none; repository tool `cargo xtask qual evidence impact --previous <dir> --current <dir> --out <dir>` (W1).
- **Data structures:** `Impact { schema: "qual-change-impact/1", previous, current, algorithms: Vec<{ id, change: Added|Removed|ImplChanged, rdl: Delta, }>, ors: Vec<{suite, previous, current}>, rep1: { divergent: Vec<{algorithm, characteristic, package}>, equivalent_counts } }`.
- **Algorithm:** BTreeMap joins by algorithm ID and suite name; replay outcomes read from the corpus `--check` output run against the previous corpus.
- **Errors:** `IMPACT-PARSE(file)`.
- **Determinism:** D0.
- **Performance budget:** under 5 s.
- **Crate:** `fid-xtask` (module `qual::evid::impact`).

## Test fixtures

`fixtures/qual/evid/impact/{minor,unchanged,first}/`; synthetic manifests and records; CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::evid::impact`: `qual evidence impact --previous <dir> --current <dir> --out <dir>`. Each dir holds `kernel/manifest.json`, the full RDL `RunRecord`, the ORS `SuiteRunRecord`, and (current only) the outcome file from replaying the previous release's replay corpus on the current kernel. Report (schema `qual-change-impact/1`, canonical JSON plus Markdown): algorithms added, removed or with changed implementation digest; per listed algorithm RDL pass/fail counts both releases and deltas; per ORS suite pass statistics and deltas; DIVERGENT results with algorithm and characteristic and EQUIVALENT counts per algorithm; `no_previous_release` when the previous dir is empty. No clock, no network, BTreeMap only, no panics. Tests: AC-1..AC-4 of spec node QUAL.EVID.IMPACT.

## Open questions

- Replaying the previous release's corpus on the new kernel is a CI job that `TRUST.REPLAY.CORPUS` does not yet describe (it checks the current corpus); cross-domain.

## Children

_Leaf: none._
