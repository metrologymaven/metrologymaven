---
id: FLOW.STUDY.CLAIM
title: Published study summary and public-claim evidence rule
level: 3
parent: FLOW.STUDY
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-FLOW.STUDY-006]
covers: []
depends_on: [FLOW.STUDY.RECORD]
uses_artifacts: [FLOW.STUDY.PROTO]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.STUDY.CLAIM: Published study summary and public-claim evidence rule

## Purpose

Defines what evidence a public performance claim needs and produces it: a `study-summary@1` document generated from aggregates, which a website or announcement claim must cite by digest. Under D-021 (accepted, option A for the first releases) incumbent baseline times stay internal and never appear in a summary.

## Scope

**In:**
- `study-summary@1` format: protocol version, release, per job partner count, median and maximum task time, success rate, mean SUS, and conformance flags against the protocol
- `cargo xtask study-summary` generator from `study-aggregate@1`
- The evidence rule as data: a claim needs a summary digest whose protocol flags all pass

**Out:**
- Claim wording and the site claim lint (owned by `OUTR.BRAND.LINT`, `OUTR.SITE.CLAIM`)
- Benchmark publication (owned by `OUTR.BENCH`; incumbent comparisons per D-021)

## Requirements

- **REQ-FLOW.STUDY.CLAIM-001** The summary generator shall set `protocol_conformant` true for a job only if it has at least 3 partners with at least 2 sessions each in the release.
- **REQ-FLOW.STUDY.CLAIM-002** While D-021 stays at option A, the summary shall contain no incumbent time, ratio or product name, and the generator shall fail with `CLAIM-INCUMBENT` if asked to include them.
- **REQ-FLOW.STUDY.CLAIM-003** The evidence rule file shall state that a public performance claim is admissible only if it cites a `study-summary@1` digest in which every job it names is `protocol_conformant`.
- **REQ-FLOW.STUDY.CLAIM-004** The summary shall be canonical JSON with a SHA-256 digest and shall contain no partner token.

## Acceptance criteria

- **AC-1** Given aggregate fixture `fixtures/flow/claim/agg-ok.json` (3 partners x 2 sessions), when `cargo xtask study-summary` runs, then the job is `protocol_conformant` and the digest equals `agg-ok.summary.digest`.
- **AC-2** Given `agg-two.json` (2 partners), then the job is not conformant.
- **AC-3** (error path) Given `--include-incumbent`, then it exits 1 with `CLAIM-INCUMBENT`.

## Design

- **Registry command(s):** none; repository tool `cargo xtask study-summary --aggregate <file>`.
- **Data structures:** `StudySummary`, `EvidenceRule { requires: "study-summary@1", all_named_jobs_conformant: true }`.
- **Algorithm:** map aggregates to summary rows; set flags; canonicalise and digest.
- **Errors:** `CLAIM-INCUMBENT`, `CLAIM-SCHEMA`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-accept` (xtask wrapper).

## Test fixtures

Synthetic aggregates under `fixtures/flow/claim/`.

## Build prompt

> In crate `fid-accept`, add `cargo xtask study-summary --aggregate <file>` producing `study-summary@1` (protocol version, release, per job: partners, median and max task time, success rate, mean SUS, protocol_conformant = partners >= 3 and sessions >= 2 each), canonical JSON plus SHA-256, no partner tokens, no incumbent data (`CLAIM-INCUMBENT` if requested). Ship `acceptance/study/evidence-rule.json` stating claims must cite a summary digest where all named jobs are conformant. Tests AC-1..AC-3 of spec node FLOW.STUDY.CLAIM.

## Open questions

- When D-021 moves to option B (third-party comparisons), the summary format needs a separate, counsel-reviewed comparison section; that is a later change.

## Children

_Leaf: none._
