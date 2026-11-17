---
id: MIG.EXPLAIN.ATTR
title: Attribution rule, explanation records and adoption
level: 3
parent: MIG.EXPLAIN
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.EXPLAIN-001, REQ-MIG.EXPLAIN-002, REQ-MIG.EXPLAIN-004, REQ-MIG.EXPLAIN-005, REQ-MIG.EXPLAIN-007]
covers: []
depends_on: [MIG.EXPLAIN.CAT, MIG.EXPLAIN.PROBE, MIG.PARALLEL.REPORT, CORE.BUS.PRIN, CORE.JRN.CANON]
side_effect: [W2, W2]
determinism: D0
cli: [fid correlate explain-differences, fid correlate adopt-explanation]
mcp: [correlate_explain_differences, correlate_adopt_explanation]
journal: [correlate.explain_differences@1, correlate.adopt_explanation@1]
updated: 2026-11-17
---

# MIG.EXPLAIN.ATTR: Attribution rule, explanation records and adoption

## Purpose

Applies the crediting rule to probe results and writes one `DifferenceAttribution` record per flagged pair of a parallel-run report. A factor is credited only when a probe shows numerically that changing it (alone, or together with the lead factor) closes the difference below the run's threshold; otherwise the pair is `UNEXPLAINED` with the probes tried. Attributions made with agent-proposed candidates are labelled agent-assisted until a human adopts them with `correlate.adopt_explanation@1`.

## Scope

**In:**
- `correlate.explain_differences@1` (W2): for each flagged pair (or a selected subset) of a parallel-run report, run single-factor probes, then, if none closes, two-factor probes with a lead factor; credit and write records
- The crediting rule and lead-factor selection
- The `DifferenceAttribution` record (schema `mig-attribution@1`), attached to the parallel-run report by its digest
- Provenance labels: `HUMAN`, `AGENT_ASSISTED`, `ADOPTED`
- `correlate.adopt_explanation@1` (W2, `human_only: true` per D-031)

**Out:**
- Candidate model (owned by `MIG.EXPLAIN.CAT`) and probe execution (owned by `MIG.EXPLAIN.PROBE`)
- User-facing wording (owned by `MIG.EXPLAIN.TEXT`)
- Study-level aggregation (owned by `MIG.STUDY.RUN`)

## Requirements

- **REQ-MIG.EXPLAIN.ATTR-001** For each processed pair, the record shall have outcome `CREDITED` with a non-empty list of catalogue factors, or `UNEXPLAINED`, and shall contain no free-text cause field.
- **REQ-MIG.EXPLAIN.ATTR-002** A factor shall be credited in the single-factor stage when its probe's absolute residual is below the run's flag threshold (or, for decision-change flags, when the probe also makes our decision equal the incumbent's).
- **REQ-MIG.EXPLAIN.ATTR-003** If no single-factor probe closes the difference, then the lead factor shall be the factor whose single probe gave the smallest absolute residual (ties broken by catalogue order), two-factor probes shall pair the lead with each other applicable candidate, and the lead and partner shall both be credited only when a pair probe closes the difference.
- **REQ-MIG.EXPLAIN.ATTR-004** If the probe set ends without any closing probe, including by budget exhaustion, then the outcome shall be `UNEXPLAINED` and the record shall list every probe tried with its residual.
- **REQ-MIG.EXPLAIN.ATTR-005** Each record shall state the parallel-run report digest, the pair ID, the candidate-set digest, the budget, the kernel version and every probe result, and the same inputs shall produce a byte-identical record.
- **REQ-MIG.EXPLAIN.ATTR-006** When any candidate used by a record has source `AgentProposed`, or the command principal is an agent, the record shall be labelled `AGENT_ASSISTED` with the agent principal and session.
- **REQ-MIG.EXPLAIN.ATTR-007** When a human principal runs `correlate.adopt_explanation@1` with a record ID and its expected digest, the system shall label the record `ADOPTED` with adopter and time; if the principal is not human the command shall fail with `adopter_not_human`, and if the digest differs it shall fail with `digest_mismatch`, journaling nothing in either case.
- **REQ-MIG.EXPLAIN.ATTR-008** If the referenced parallel-run report does not exist or has no flagged pairs in the selection, then `correlate.explain_differences@1` shall fail with `nothing_to_explain` and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/explain/attr/fit-and-round/` (report with three flagged pairs: one closed by `FIT_METHOD`, one closed only by `ALIGNMENT`+`ROUNDING`, one not closable), when `fid correlate explain-differences --report <id> --candidates cands.json --json` runs as a human, then the records are `CREDITED[FIT_METHOD]`, `CREDITED[ALIGNMENT, ROUNDING]` (lead `ALIGNMENT`), and `UNEXPLAINED` with all probes listed, all labelled `HUMAN`.
- **AC-2** Given the same inputs and budget 2, then the third pair is `UNEXPLAINED` with exactly 2 probes listed.
- **AC-3** Given AC-1 run by principal `agent-1` on its agent branch, then every record is `AGENT_ASSISTED` with `agent-1`.
- **AC-4** Given AC-3, when human `qa-1` runs `fid correlate adopt-explanation <record> --expected-digest <d>`, then the record is `ADOPTED` by `qa-1`; when `agent-1` runs it, it fails with `adopter_not_human` and journals nothing.
- **AC-5** (error path) Given a report with no flagged pairs, then the command fails with `nothing_to_explain` and journals nothing.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `correlate.explain_differences@1` (W2; params `report: ParallelRunReportId`, `pairs?: Vec<PairId>`, `candidates: CandidateSet`, `budget?: u8` default 12; result `Vec<DifferenceAttribution>` and digests). `correlate.adopt_explanation@1` (W2, `human_only: true`; params `record: AttributionId`, `expected_digest: Sha256`, `note?`; result `{ label, adopted_digest }`).
- **Data structures:** `DifferenceAttribution { id (UUIDv5 from report digest and pair ID), report_digest, pair_id, outcome: Credited(Vec<Factor>) | Unexplained, lead?: Factor, probes: Vec<ProbeResult>, candidate_set_digest, budget, kernel_version, provenance: Human | AgentAssisted{agent, session} | Adopted{by, at, digest} }`.
- **Algorithm:** for each selected flagged pair in pair-ID order: run single stage (PROBE) → credit closing factors → if none, pick lead (min |residual|, tie by catalogue order), run pair stage with remaining budget → credit lead+partner for closing pairs → else `UNEXPLAINED`. Write records in one bus transaction.
- **Errors:** `nothing_to_explain`, `adopter_not_human`, `digest_mismatch`, `not_found`, plus `candidate_invalid` from CAT.
- **Determinism:** D0 (given PROBE's D0 results).
- **Performance budget:** 100 flagged pairs with 12 probes each in under 60 s on reference hardware [unverified until measured].
- **Crate:** `fid-mig` (module `explain::attr`).

## Test fixtures

`fixtures/mig/explain/attr/`: seeded synthetic projects with incumbent values constructed from known alternative settings so the expected credits are known by construction; CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `explain::attr` and register two commands: `correlate.explain_differences@1` (W2; CLI `fid correlate explain-differences`, MCP `correlate_explain_differences`) and `correlate.adopt_explanation@1` (W2, `human_only: true`; CLI `fid correlate adopt-explanation`, MCP `correlate_adopt_explanation`, projected as a request-only tool). Implement spec node MIG.EXPLAIN.ATTR: for each flagged pair of a parallel-run report, run single-factor probes via `explain::probe`; credit every factor whose probe closes the difference below the run threshold (and restores the incumbent decision for decision-change flags); if none closes, choose the lead factor (smallest absolute residual, tie by catalogue order), run lead+candidate pair probes within the remaining budget, credit both when a pair closes; otherwise `UNEXPLAINED` with all probes listed. Records carry report digest, pair ID, candidate-set digest, budget, kernel version, probes and provenance (`HUMAN`, `AGENT_ASSISTED`, `ADOPTED`); agent principals or agent-proposed candidates force `AGENT_ASSISTED`. Adoption requires a human principal (`adopter_not_human`) and matching digest (`digest_mismatch`) and journals nothing on failure. Fail `nothing_to_explain` when no flagged pair is selected. Records are canonical JSON (RFC 8785) and byte-identical for identical inputs. Tests for AC-1..AC-6. Never accept free-text causes; never let an LLM output decide a credit. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- The lead-factor rule interprets REQ-MIG.EXPLAIN-002's "previously credited factor"; see the parent's Open questions.

## Children

_Leaf: none._
