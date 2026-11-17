---
id: GEO.SEL.QUERY.CMD
title: Semantic selection commands and re-evaluation
level: 4
parent: GEO.SEL.QUERY
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.SEL.QUERY-001, REQ-GEO.SEL.QUERY-004]
covers: [GOM.CAD.01]
depends_on: [GEO.SEL.QUERY.GRAMMAR, GEO.SEL.QUERY.EVAL, GEO.SEL.QUERY.NEAR, GEO.SEL.QUERY.CAD, GEO.SEL.MODEL, GEO.CAD.GROUP, GEO.CAD.TOPO.CMD, GEO.TYPES.REG, CORE.REG, CORE.DEP, CORE.JRN]
side_effect: [W2, W2]
determinism: D0
cli: [fid geo query-selection, fid geo reevaluate-selection]
mcp: [geo_query_selection, geo_reevaluate_selection]
journal: [geo.query_selection@1, geo.reevaluate_selection@1]
updated: 2026-11-17
---

# GEO.SEL.QUERY.CMD: Semantic selection commands and re-evaluation

## Purpose

The registry commands that create a semantic selection from a JSON query and re-evaluate it when its target or a referenced element changes, journaling the query digest together with the resulting index-set digest. This L2 adapter wires the library evaluators to document state: element resolution, CAD group and layer membership from GEO.CAD.GROUP, and bodies through GEO.CAD.TOPO.CMD.

## Scope

**In:**
- `geo.query_selection@1` (W2): `{ target, query }` → a selection element holding the canonical query, its digest and the evaluated `IndexSet`
- `geo.reevaluate_selection@1` (W2): `{ selection, target? }` → re-evaluates on the current target (or re-targets to a new element of the same kind), storing the new index set
- CORE.DEP edges from the selection to its target and to every element, selection, group, layer and body the query references; when CORE.DEP recomputes a stale semantic selection, it runs `geo.reevaluate_selection@1`
- Providers: `ElementResolver`, `CadMembership` over GEO.CAD.GROUP, `BodySource` from GEO.CAD.TOPO.CMD; distance and CAD evaluators enabled when the P1b leaves are built (until then `unsupported_predicate`)
- Query input as a JSON object parameter (never a file path; CLI reads it from an argument or stdin)

**Out** (and which node owns it instead):
- Grammar and evaluation (owned by `GEO.SEL.QUERY.GRAMMAR`, `GEO.SEL.QUERY.EVAL`, `GEO.SEL.QUERY.NEAR`, `GEO.SEL.QUERY.CAD`)
- Index-set selection commands (owned by `GEO.SEL.CMD`)

## Requirements

- **REQ-GEO.SEL.QUERY.CMD-001** `geo.query_selection@1` shall be class W2, reject any query that GEO.SEL.QUERY.GRAMMAR rejects with `bad_query` or `query_limit` and journal nothing, and otherwise produce one journal entry containing the query digest, the target digest and the index-set digest.
- **REQ-GEO.SEL.QUERY.CMD-002** `geo.reevaluate_selection@1` shall be class W2 and produce one journal entry containing the query digest, the new target digest and the new index-set digest, leaving earlier index sets addressable in history.
- **REQ-GEO.SEL.QUERY.CMD-003** When a target or referenced element of a semantic selection changes, CORE.DEP shall mark the selection stale, and recomputation shall run `geo.reevaluate_selection@1` rather than reuse the old index set.
- **REQ-GEO.SEL.QUERY.CMD-004** If evaluation fails (`unresolved_reference`, `domain_mismatch`, `unsupported_predicate`, `frame_mismatch`), then the command shall fail with that error and journal nothing; a re-evaluation failure shall leave the selection stale with the error recorded.
- **REQ-GEO.SEL.QUERY.CMD-005** Re-targeted replay of a program containing `geo.query_selection@1` on a new target shall re-evaluate the stored query on the new target and journal the new digests (CONSTITUTION §4 re-targeted mode).

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/geo/sel/query/proj_cloud` (the `cloud_attrs` cloud), when `fid geo query-selection --target <cloud> --query '{"v":1,"domain":"points","where":{"attr_range":{"channel":"intensity","min":0.2}}}' --json` runs, then a selection is created whose index set equals the EVAL brute-force result and the journal grows by one entry with three digests.
- **AC-2** Given AC-1, when the cloud is replaced by a second scan through its import command, then the selection is stale; `fid geo reevaluate-selection <sel> --json` writes one entry with the new digests.
- **AC-3** (error path) Given a query with an unknown key, then `bad_query` with a JSON pointer and the journal is unchanged.
- **AC-4** (error path) Given an `in_group` query in a build without the CAD provider, then `unsupported_predicate`; with it (P1b), on `fixtures/geo/sel/query/cad_bracket`, the GEO.SEL.QUERY.CAD AC-3 result.
- **AC-5** (replay) Given the AC-1 journal, exact replay gives `IDENTICAL`; re-targeted replay on the second scan gives the AC-2 index set.
- **AC-6** Given the generated manifest, then both tools are named tools (W2; agents write to their branch) and the `query` parameter schema is the GRAMMAR v1 schema.

## Design

- **Registry command(s):** `geo.query_selection@1` W2 `{ target: ElementRef, query: SelectionQueryJson }` → `{ selection, query_digest, index_digest, count }`; `geo.reevaluate_selection@1` W2 `{ selection: SelectionRef, target?: ElementRef }` → same shape.
- **Data structures:** semantic selection element `{ query_canonical, query_digest, target, target_digest, index_set }` registered through GEO.TYPES.REG; providers implemented over CORE and GEO.CAD.GROUP state.
- **Algorithm:** parse and canonicalise, resolve target, build `EvalContext` with providers, evaluate, register the selection and its dependency edges, journal.
- **Errors:** `bad_query`, `query_limit`, `bad_quantity`, `unresolved_reference`, `domain_mismatch`, `unsupported_predicate`, `frame_mismatch`, `stale_selection`, `not_found`.
- **Determinism:** D0.
- **Performance budget:** adapter overhead under 5 ms plus evaluation time [unverified].
- **Crate:** `fid-geo-cmd` (L2 command crate).

## Test fixtures

- `fixtures/geo/sel/query/proj_cloud` (two scans of one synthetic part, seeds 41 and 42), `cad_bracket` (from GEO.SEL.QUERY.CAD). CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cmd` (L2), register `geo.query_selection@1` (W2, `{ target, query }`) and `geo.reevaluate_selection@1` (W2, `{ selection, target? }`) through CORE.REG; CLI and MCP are generated, with the `query` parameter typed by the `fid-geo-sel` v1 JSON schema (no file paths). Parse and canonicalise with `query::parse`/`query::canon`, build an `EvalContext` with an `ElementResolver` over CORE elements, a `CadMembership` over GEO.CAD.GROUP groups and layers, the GEO.CAD.TOPO `BodySource`, and the `query::near`/`query::cad` evaluators (behind build features until the P1b leaves exist; otherwise `unsupported_predicate`). Register the selection (canonical query, digests, index set) via GEO.TYPES.REG with CORE.DEP edges to the target and every referenced element, selection, group, layer and body; CORE.DEP recompute of a stale semantic selection runs `reevaluate_selection`. Each commit journals one entry with query, target and index-set digests; failures journal nothing. Acceptance: AC-1..AC-6. Constraints: CONSTITUTION C6 (queries are data), §3 (W2, agents write to their branch), §4 (re-targeted replay), §8 (no panic, no `unsafe`). Files: `crates/fid-geo-cmd/src/sel_query.rs`, `tests/sel_query_cmd.rs`, `fixtures/geo/sel/query/proj_cloud/`.

## Open questions

- This P0 leaf depends on the P1b leaves GEO.SEL.QUERY.NEAR, GEO.SEL.QUERY.CAD, GEO.CAD.GROUP and GEO.CAD.TOPO.CMD for its CAD and distance wiring; in P0 it ships with those providers disabled. If reviewers prefer, the wiring could move to a separate P1b command leaf.
- GEO.CAD.GROUP is implemented in `fid-geo-cmd` and depends on CORE, but its node ID does not end in `.CMD`; under the amended CONSTITUTION §2.1 its layer placement may need a review (not changed here).

## Children

_Leaf: none._
