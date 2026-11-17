---
id: CORE.DEP
title: Parametric dependency graph and recompute
level: 2
parent: CORE
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE-007]
covers: [GOM.PRJ.05, GOM.PRJ.07]
depends_on: [CORE.PRJ, CORE.BUS, CORE.JOB]
cli: [fid project recompute*, fid project show-dependencies]
mcp: [project_recompute*, project_show_dependencies]
journal: [project.recompute*]
updated: 2026-11-16
---

# CORE.DEP: Parametric dependency graph

## Purpose

Makes the project parametric. Every element records which elements and parameters it was computed from. When an input changes (a mesh is replaced, an alignment is redone, a nominal is edited), DEP finds every dependent element, recomputes them in a deterministic order through their own registered commands, and journals the whole recompute as one entry. It is the signature capability of scan-inspection incumbents and is separate from PRJ because it is an algorithm over the tree, not the tree itself.

## Scope

**In:**
- Dependency edges between elements, recorded when an element is created by a command
- Cycle detection and rejection
- Change propagation: marking dependents `outdated`
- Recompute: deterministic topological order, per-element failure isolation, cancellation
- Recompute as one journal entry listing every recomputed element and its new digest
- Input replacement (replace mesh, replace CAD, replace a referenced element) with recompute
- Manual vs automatic recompute mode setting
- Query: dependents and dependencies of an element

**Out** (and which node owns it instead):
- Element types and the element tree (owned by `CORE.PRJ`)
- The computations themselves (owned by each element type's domain: `KRN`, `ALN`, `GDT`, `SCAN`)
- Job execution and progress (owned by `CORE.JOB`)
- Journal append (owned by `CORE.JRN`)

## Requirements

- **REQ-CORE.DEP-001** When a command creates or updates an element, the dependency graph shall record an edge from each input element and parameter to the new element.
- **REQ-CORE.DEP-002** If a command would create a dependency cycle, then the dependency graph shall reject it with a typed cycle error naming the elements in the cycle.
- **REQ-CORE.DEP-003** When an element changes, the dependency graph shall mark every transitive dependent `outdated` before any recompute starts.
- **REQ-CORE.DEP-004** When a recompute runs, the dependency graph shall recompute dependents in a topological order whose tie-break is deterministic (by stable entity ID), so two runs on the same graph produce the same order.
- **REQ-CORE.DEP-005** When a recompute completes, the system shall append exactly one journal entry that lists every recomputed element with its new output digest.
- **REQ-CORE.DEP-006** If one element fails to recompute, then the dependency graph shall mark it `failed`, mark its dependents `outdated`, continue with independent elements, and report the failure in the recompute entry.
- **REQ-CORE.DEP-007** When an input element is replaced (for example the measured mesh), the dependency graph shall keep the IDs of all dependents and recompute them against the new input.
- **REQ-CORE.DEP-008** While automatic recompute is off, the dependency graph shall only mark dependents `outdated` and shall not recompute until a recompute command is issued.
- **REQ-CORE.DEP-009** When a recompute is cancelled, the dependency graph shall roll back all of its partial results and journal nothing.
- **REQ-CORE.DEP-010** Recomputing the same graph with the same inputs and kernel shall produce output digests identical to the original computation for D0 and D1 elements.
- **REQ-CORE.DEP-011** Change propagation (marking) over a graph of 100,000 elements shall complete in under 100 ms on reference hardware [unverified until measured].

## Interfaces

- **Offers:** `DepGraph::edges(element)`, `DepGraph::dependents(element)`, `DepGraph::recompute(scope) -> Job`; the recompute journal entry schema.
- **Commands (illustrative):** `project.recompute_dependents@1` (W2), `project.replace_input@1` (W2), `project.set_recompute_mode@1` (W2), `project.show_dependencies@1` (R0).
- **Consumes:** `CORE.PRJ` (elements and state), `CORE.BUS` (executing each element's command), `CORE.JOB` (running recompute as a cancellable job).

## Decomposition hints

EDGE (edge recording and cycle detection), PROP (propagation and marking), RECOMP (ordering, failure isolation, single-entry commit), REPL (input replacement), MODE (auto/manual setting).

## Open questions

- Parent Open question: DEP in CORE or a shared document-model subsystem with GEO. This split follows the parent's recommendation (CORE.DEP; element types from their domains).
- Should recompute of thousands of independent elements run in parallel? Allowed only if the result is order-independent; KNP-1 fixed-order reductions inside each element still apply.

## Split rationale

The dividing principle is **the three phases of a parametric update, plus its main user action**: EDGE records what depends on what (and refuses cycles); PROP marks dependents outdated and decides whether to recompute (mode); RECOMP recomputes deterministically and commits once; REPL is the input-replacement action that drives the other three. EDGE, PROP and REPL are small and fully specified with test-kit fixtures, so they are leaves; RECOMP holds the hard parts (ordering, failure isolation, rollback, parallelism) and stays open.

Alternatives considered:
- **MODE as its own child** (as the hints suggest). Rejected: the mode only decides whether propagation submits a recompute, so it sits with PROP.
- **Queries as their own child.** Rejected: the dependency query reads the edge index and belongs with EDGE.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.DEP.EDGE | Dependency edges, cycle rejection and dependency queries (leaf) | 001 002 | none |
| CORE.DEP.PROP | Change propagation and recompute mode (leaf) | 003 008 011 | none |
| CORE.DEP.RECOMP | Deterministic recompute with failure isolation and single-entry commit | 004 005 006 009 010 | GOM.PRJ.05 |
| CORE.DEP.REPL | Input replacement (leaf) | 007 | GOM.PRJ.07 |
