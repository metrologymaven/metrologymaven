---
id: ALN.BFIT.LOCAL
title: Best-fit scope resolution for local best-fit
level: 3
parent: ALN.BFIT
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.BFIT-001]
covers: [GOM.ALN.03]
depends_on: [GEO.SEL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ALN.BFIT.LOCAL: Best-fit scope resolution for local best-fit

## Purpose

Turns the `scope` parameter of best-fit into the concrete measured index set and reference patch set the solver uses, so that a local best-fit uses only the selected regions or elements. Resolution happens once, before the first iteration, and its digests are recorded so the alignment is re-runnable.

## Scope

**In:**
- `BestfitScope` parsing: `global`, or `local` with measured selectors and optional reference selectors (element IDs, index sets, GEO.SEL semantic queries, CAD face IDs)
- Resolution to a measured index set and a reference patch set
- Minimum-size check and digests

**Out** (and which node owns it instead):
- The iteration loop and correspondence filtering by patch (`ALN.BFIT.ICP`)
- Selection semantics and lasso resolution (`GEO.SEL`, `VIZ`)
- Feature-level selection for feature best-fit (`ALN.BFIT.FEAT`)

## Requirements

- **REQ-ALN.BFIT.LOCAL-001** When scope is `global`, the function shall return the full measured index set and no reference restriction.
- **REQ-ALN.BFIT.LOCAL-002** When scope is `local`, the function shall return the union of the measured selections as a sorted, de-duplicated index set and the union of the reference selections as a sorted set of face or triangle IDs.
- **REQ-ALN.BFIT.LOCAL-003** If the resolved measured set is empty, then the function shall fail with `empty_local_scope`.
- **REQ-ALN.BFIT.LOCAL-004** If the resolved measured set has fewer than 6 points [unverified], then the function shall fail with `insufficient_points` stating the count.
- **REQ-ALN.BFIT.LOCAL-005** The function shall return the SHA-256 digest of the canonical selector list and of each resolved set, for recording in the alignment parameters.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/bfit/plate-regions` (a 10,000-point plate cloud with three named regions R1, R2, R3 of 1,000, 2,000 and 500 points, two overlapping by 100), when scope is `local [R1, R2]`, then the result has 2,900 sorted unique indices.
- **AC-2** Given the same fixture and reference faces F3 and F7, then the reference patch set is exactly {F3, F7}.
- **AC-3** Given scope `global`, then the result has 10,000 indices and no reference restriction.
- **AC-4** (error path) Given a selector that matches nothing, then the call fails with `empty_local_scope`.
- **AC-5** (error path) Given an index set of 5 points, then the call fails with `insufficient_points` and count 5.
- **AC-6** (determinism) Given AC-1 twice with selectors in a different order, then the index set and its digest are identical; the selector-list digest follows the canonical (sorted) selector order.

## Design

- **Function:** `resolve_scope(doc_view, scope: &BestfitScope) -> Result<ResolvedScope, ScopeError>`; R1; called once by `ALN.BFIT.ICP`.
- **Data structures:** `BestfitScope = Global | Local { measured: Vec<Selector>, reference: Vec<Selector> }`; `ResolvedScope { measured: IndexSet, reference_patches: Option<SortedIds>, selector_digest, measured_digest, reference_digest }`.
- **Algorithm:** resolve each selector through GEO.SEL, merge by sorted union (fixed order), canonicalise the selector list by sorting its canonical JSON before digesting.
- **Errors:** `empty_local_scope`, `insufficient_points`, `selector_invalid`.
- **Determinism:** D0; integer set operations only.
- **Performance budget:** 1 M selected indices in under 50 ms [unverified until measured].
- **Crate:** `fid-align` (resolution needs the document view; provisional name).

## Test fixtures

`fixtures/aln/bfit/plate-regions`: seeded generator (seed 4), a 300 x 200 mm plate cloud of 10,000 points with named regions as index sets and a reference mesh with stable face IDs. Project-generated, CC-BY-4.0.

## Build prompt

> In crate `fid-align`, implement `resolve_scope(doc_view, scope) -> Result<ResolvedScope, ScopeError>` for best-fit. It is an R1 helper with no registry command of its own; `align.run_bestfit@1` calls it once before iterating.
> Requirements: `global` returns all measured indices and no reference restriction; `local` resolves each measured and reference selector (element IDs, index sets, GEO semantic selectors, CAD face IDs) and returns the sorted, de-duplicated union for each side; empty measured set gives `empty_local_scope`; fewer than 6 points gives `insufficient_points` with the count; return SHA-256 digests of the canonical (sorted) selector list and of each resolved set.
> Acceptance: fixture `fixtures/aln/bfit/plate-regions` (seed 4); tests for union with overlap (2,900 indices), reference patch set, global scope, empty and too-small scopes, order independence of sets and digests.
> Constraints: no std HashMap/HashSet (use sorted vectors or BTree collections); no panic, no unwrap/expect, no unsafe; determinism D0.

## Open questions

- None blocking.

## Children

_Leaf: none._
