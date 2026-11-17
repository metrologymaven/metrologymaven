---
id: MIG.EXPLAIN
title: Difference attribution ("Why is it different?")
level: 2
parent: MIG
kind: subsystem
status: open
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-MIG-003]
covers: []
depends_on: [MIG.PARALLEL, CORE.RPL.RETGT, ALN.QA.CMP, GDT.RES.RESOLVE, GDT.FORM.FCORE]
cli: [fid correlate explain-differences, fid correlate show-explanation]
mcp: [correlate_explain_differences, correlate_show_explanation]
journal: [correlate.explain_differences@1]
updated: 2026-11-16
---

# MIG.EXPLAIN: Difference attribution

## Purpose

For every flagged pair in a parallel-run report, finds which named factor accounts for the difference: fit method, filter, alignment, datum interpretation, point selection or rounding. It does this by deterministic **factor probes**: re-evaluating our side on a scratch branch with one factor changed to the setting the incumbent most plausibly used (as stated in the export, or as a user-selected candidate), and checking whether the difference falls below the threshold. A factor is credited only when a probe shows it numerically. Anything that no probe closes is reported `UNEXPLAINED`. No LLM output is ever an attribution.

## Scope

**In:**
- The closed factor catalogue: `FIT_METHOD`, `FILTER`, `ALIGNMENT`, `DATUM_INTERPRETATION`, `POINT_SELECTION`, `ROUNDING`, each with its probe definition
- Factor probes as re-targeted re-evaluations on a scratch branch (never the working document)
- Single-factor and ordered two-factor probes, with a bounded probe budget per pair
- Attribution result per pair: credited factors with residual difference after each probe, or `UNEXPLAINED`
- Explanation records attached to the parallel-run report, with "Why?" text generated from templates in our own words
- `correlate.explain_differences@1` (W2: writes explanation records), `correlate.show_explanation@1` (R0)

**Out** (and which node owns it instead):
- Pairing and flagging (owned by `MIG.PARALLEL`)
- Re-targeted replay machinery (owned by `CORE.RPL.RETGT`)
- Alignment comparison (owned by `ALN.QA.CMP`)
- Interpretation of GD&T callouts (owned by `GDT.RES`)
- Aggregating attributions across a study (owned by `MIG.STUDY`)

## Requirements

- **REQ-MIG.EXPLAIN-001** For every flagged pair, the explainer shall output either one or more credited factors from the closed factor catalogue or `UNEXPLAINED`, and shall never output free-form causes.
- **REQ-MIG.EXPLAIN-002** A factor shall be credited only when a probe that changes that factor alone, or that factor with one previously credited factor, reduces the absolute difference below the run's flag threshold; the residual difference after each probe shall be reported.
- **REQ-MIG.EXPLAIN-003** Factor probes shall run on a scratch branch through the replay engine and shall not create, change or delete any ResultRecord in the working document.
- **REQ-MIG.EXPLAIN-004** If the probe budget for a pair (default 12 probes [unverified default]) is exhausted without closing the difference, then the pair shall be reported `UNEXPLAINED` with the list of probes tried.
- **REQ-MIG.EXPLAIN-005** Attribution shall be deterministic: the same parallel-run report, probe candidates and kernel version shall produce byte-identical explanation records.
- **REQ-MIG.EXPLAIN-006** Explanation text shown to users shall be generated from versioned templates in our own words, citing standards by designator and clause only, and shall quote kernel values rather than computing any.
- **REQ-MIG.EXPLAIN-007** When an agent proposes probe candidates, the candidates shall be provenance-tagged and the resulting attribution shall be labelled agent-assisted until a human adopts it.

## Interfaces

- **Offers:** `correlate.explain_differences@1` (W2), `correlate.show_explanation@1` (R0); `DifferenceAttribution` type and factor catalogue consumed by `MIG.STUDY` and UX "Why?" panels.
- **Consumes:** `MIG.PARALLEL` reports; `CORE.RPL.RETGT` re-targeted replay; `ALN.QA.CMP`; `GDT.RES.RESOLVE` for datum interpretation variants; `GDT.FORM.FCORE` association policies for fit-method variants.

## Decomposition hints

CAT (factor catalogue and probe definitions), PROBE (scratch-branch probe runner and budget), ATTR (attribution rule and records), TEXT (template text and "Why?" view).

## Open questions

- Filter probes need filter settings from the incumbent export; many exports do not state them [unverified]. Then FILTER can only be probed with user-selected candidates.

## Children

_None yet: status `open`._
