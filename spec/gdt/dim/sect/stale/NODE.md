---
id: GDT.DIM.SECT.STALE
title: Staleness of section results after a re-cut
level: 4
parent: GDT.DIM.SECT
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM.SECT-002, REQ-GDT.DIM.SECT-003]
covers: []
depends_on: [CORE.DEP, GDT.DIM.SECT.CMDS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DIM.SECT.STALE: Staleness of section results after a re-cut

## Purpose

A section check is only valid for the section it was computed on. This leaf registers each committed section result as a dependent of its section element in the project dependency graph, so a re-cut marks it `outdated` through the normal CORE.DEP propagation, and offers the check that a release step calls to refuse outdated section results.

## Scope

**In:** declaring the dependency edge (result → section element) when a section command commits; comparing the stored `section_digest` with the section's current digest; `section_result_current(result) -> Current | Stale{old_digest, new_digest}` for release gates.
**Out:** change propagation and recompute (owned by `CORE.DEP`); the release step itself (owned by `RPT` and `TRUST`); the section commands (owned by `GDT.DIM.SECT.CMDS`).

## Requirements

- **REQ-GDT.DIM.SECT.STALE-001** When a section command commits a result, the system shall declare a dependency edge from the result to each section element it used.
- **REQ-GDT.DIM.SECT.STALE-002** When the section element's digest differs from the result's recorded `section_digest`, `section_result_current` shall return `Stale` with both digests.
- **REQ-GDT.DIM.SECT.STALE-003** If a release gate asks about a stale section result, then the check shall return `Stale` and the gate shall refuse release until the characteristic is re-evaluated.

## Acceptance criteria

- **AC-1** Given a committed section distance result from fixture `fixtures/gdt/sect/rect_with_fillet.json`, when the section is re-cut with a shifted plane, then CORE.DEP marks the result `outdated` and `section_result_current` returns `Stale` with both digests.
- **AC-2** Given re-evaluation after the re-cut, then the check returns `Current`.
- **AC-3** Given an unchanged section (re-cut with identical parameters and identical digest), then the result stays `Current`.

## Design

- **Registry command(s):** none; hooks called by the section commands' commit path and by release gates.
- **Data structures:** `SectionCurrency { Current, Stale{old_digest, new_digest} }`.
- **Algorithm:** edge declaration through the CORE.DEP edge API; digest equality check.
- **Errors:** `section_missing{id}` when the section element was deleted (treated as stale by gates).
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-gdt` (module `dim::section_stale`).

## Test fixtures

Reuses the section fixtures of `GDT.DIM.SECT.SPAN` and a CORE.DEP in-memory graph (CC0).

## Build prompt

> In crate `fid-gdt`, module `dim::section_stale`: when a section check commits, declare a CORE.DEP dependency edge from the result element to every section element used; implement `section_result_current(result, project) -> Current | Stale{old_digest, new_digest}` by comparing the result's recorded `section_digest` with the section element's current digest (deleted section gives `section_missing`, which gates treat as stale). Tests per acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`.

## Open questions

- None.

## Children

_Leaf: none._
