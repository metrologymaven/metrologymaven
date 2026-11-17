---
id: MIG.EXPLAIN.CAT
title: Closed factor catalogue and probe candidates
level: 3
parent: MIG.EXPLAIN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-MIG.EXPLAIN-001, REQ-MIG.EXPLAIN-007]
covers: []
depends_on: [CORE.JRN.CANON, CORE.BUS.PRIN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# MIG.EXPLAIN.CAT: Closed factor catalogue and probe candidates

## Purpose

Defines the only causes the explainer may ever name, and the typed form in which a candidate setting for each cause is proposed. The catalogue is a closed Rust enum plus a versioned data file (`mig-factors@1`) describing each factor's parameter schema; a `ProbeCandidate` carries the factor, the candidate setting, where the setting came from and who proposed it. Everything downstream (variants, probes, attribution, text) is typed against this catalogue, so a free-form cause cannot be represented.

## Scope

**In:**
- `Factor` enum: `FitMethod`, `Filter`, `Alignment`, `DatumInterpretation`, `PointSelection`, `Rounding`, with a fixed catalogue order used everywhere for tie-breaking
- Per-factor setting schema (JSON Schema in `mig-factors@1`): e.g. association policy ID for `FitMethod`, filter policy ID and parameters for `Filter`, incumbent known-transform reference for `Alignment`, alternative resolver row for `DatumInterpretation`, point index set for `PointSelection`, decimals and rounding rule for `Rounding`
- `ProbeCandidate { factor, setting, source: ExportStated | UserSelected | AgentProposed, proposer, session? }` with validation and canonical digest
- Candidate-set validation (one setting per factor per candidate, schema check, size limit)

**Out:**
- Turning a candidate into a re-evaluation (owned by `MIG.EXPLAIN.VARIANT`)
- Running probes (owned by `MIG.EXPLAIN.PROBE`) and crediting (owned by `MIG.EXPLAIN.ATTR`)
- Principal identification (owned by `CORE.BUS.PRIN`)

## Requirements

- **REQ-MIG.EXPLAIN.CAT-001** The factor catalogue shall contain exactly `FIT_METHOD`, `FILTER`, `ALIGNMENT`, `DATUM_INTERPRETATION`, `POINT_SELECTION` and `ROUNDING`, in that order, and adding a factor shall require a new major version of `mig-factors`.
- **REQ-MIG.EXPLAIN.CAT-002** If a candidate names a factor outside the catalogue or a setting that fails that factor's schema, then validation shall fail with `candidate_invalid` naming the candidate index and factor.
- **REQ-MIG.EXPLAIN.CAT-003** When a candidate is created by an agent principal, its source shall be `AgentProposed` with the agent principal ID and session ID, whatever source the caller claims.
- **REQ-MIG.EXPLAIN.CAT-004** A candidate set shall hold at most 64 candidates [unverified limit], and larger sets shall fail with `candidate_set_too_large`.
- **REQ-MIG.EXPLAIN.CAT-005** The candidate-set digest shall be the SHA-256 of its RFC 8785 canonical JSON with candidates sorted by factor catalogue order and then by setting digest.

## Acceptance criteria

- **AC-1** Given `fixtures/mig/explain/cat/valid-set.json` (one candidate per factor), when validated, then validation passes and the digest equals `valid-set.digest`.
- **AC-2** (error path) Given `bad-factor.json` (factor `PROBE_RADIUS`), then validation fails with `candidate_invalid` at index 0.
- **AC-3** Given `valid-set.json` submitted by principal `agent-1` with every source set to `UserSelected`, when validated, then every candidate's source is `AgentProposed` with `agent-1`.
- **AC-4** Given `valid-set.json` with candidates reordered, then the digest is unchanged.
- **AC-5** (error path) Given a 65-candidate set, then validation fails with `candidate_set_too_large`.

## Design

- **Registry command(s):** none; library used by `correlate.explain_differences@1` (`MIG.EXPLAIN.ATTR`).
- **Data structures:** `enum Factor` (repr u8, catalogue order); `ProbeCandidate`; `CandidateSet { candidates: Vec<ProbeCandidate>, digest }`; data file `mig-factors@1.json` with per-factor setting JSON Schema and a short identifier (no prose).
- **Algorithm:** schema validation per factor; principal override for agents; sort; canonical JSON (`CORE.JRN.CANON`); SHA-256.
- **Errors:** `candidate_invalid`, `candidate_set_too_large`.
- **Determinism:** D0.
- **Performance budget:** 64-candidate set validated in under 5 ms [unverified until measured].
- **Crate:** `fid-mig` (module `explain::cat`).

## Test fixtures

`fixtures/mig/explain/cat/`: hand-written candidate sets, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `explain::cat` implementing spec node MIG.EXPLAIN.CAT. Define `enum Factor { FitMethod, Filter, Alignment, DatumInterpretation, PointSelection, Rounding }` in that fixed order, the `mig-factors@1` data file with a JSON Schema per factor setting, `ProbeCandidate { factor, setting, source: ExportStated|UserSelected|AgentProposed, proposer, session }` and `CandidateSet`. Validation: unknown factor or schema failure → `candidate_invalid{index,factor}`; more than 64 candidates → `candidate_set_too_large`; when the calling principal is an agent, force `source = AgentProposed` with its principal and session IDs. Digest = SHA-256 of RFC 8785 canonical JSON sorted by catalogue order then setting digest. Tests for AC-1..AC-5. No free-form cause strings anywhere in the types. No `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- None.

## Children

_Leaf: none._
