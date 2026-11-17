---
id: CORE.DEP.RECOMP
title: Deterministic recompute with failure isolation and single-entry commit
level: 3
parent: CORE.DEP
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.DEP-004, REQ-CORE.DEP-005, REQ-CORE.DEP-006, REQ-CORE.DEP-009, REQ-CORE.DEP-010]
covers: [GOM.PRJ.05]
depends_on: [CORE.DEP.EDGE, CORE.DEP.PROP, CORE.JOB, CORE.BUS, CORE.PRJ.TREE]
cli: [fid project recompute-dependents]
mcp: [project_recompute_dependents]
journal: [project.recompute_dependents@1]
updated: 2026-11-16
---

# CORE.DEP.RECOMP: Deterministic recompute

## Purpose

The parametric engine's core loop. RECOMP takes a set of outdated elements, orders them topologically with a deterministic tie-break, re-runs each element's own registered command against its current inputs, isolates failures, and commits the whole recompute as one journal entry that lists every recomputed element and its new output digest. It runs as a cancellable job; cancellation rolls everything back.

## Scope

**In:**
- Topological ordering with tie-break by stable entity ID
- Re-running each element's recorded command and arguments with current inputs through the bus handler path (no per-element journal entries)
- Failure isolation: failed element marked `failed`, its dependents stay `outdated`, independent elements continue
- Single recompute entry: list of recomputed elements, new output digests, failures
- Cancellation via the job system with full rollback
- Determinism checks (D0 and D1 elements reproduce original digests)
- Optional order-independent parallelism within one topological level
- `project.recompute_dependents@1` (W2)

**Out** (and which node owns it instead):
- Marking and mode (owned by `CORE.DEP.PROP`)
- The element computations (owned by `KRN`, `ALN`, `GDT`, `SCAN`)
- Job scheduling and progress (owned by `CORE.JOB`)

## Requirements

- **REQ-CORE.DEP.RECOMP-001** When a recompute runs, the dependency graph shall process elements in a topological order whose ties are broken by ascending stable entity ID.
- **REQ-CORE.DEP.RECOMP-002** When a recompute completes, the system shall append exactly one journal entry listing every recomputed element with its new output digest and every failed element with its error code.
- **REQ-CORE.DEP.RECOMP-003** If an element fails to recompute, then the dependency graph shall mark it `failed`, leave its transitive dependents `outdated`, and continue with elements that do not depend on it.
- **REQ-CORE.DEP.RECOMP-004** If a recompute is cancelled, then the dependency graph shall discard all partial results, leave every element in its pre-recompute state, and journal nothing in the Record Journal.
- **REQ-CORE.DEP.RECOMP-005** Recomputing the same graph with the same inputs and kernel shall produce output digests identical to the original computation for D0 and D1 elements.
- **REQ-CORE.DEP.RECOMP-006** If elements of one topological level are computed in parallel, then the committed entry shall be byte-identical to the entry of a sequential run.
- **REQ-CORE.DEP.RECOMP-007** The recompute shall run as a job that reports progress as the fraction of elements processed and checks for cancellation between elements.

## Interfaces

- **Offers:** `DepGraph::recompute(scope) -> JobId`; the recompute entry schema (consumed by `CORE.RPL` and `VIZ`).
- **Commands:** `project.recompute_dependents@1` (W2; scope: element list, `all_outdated`, or a subtree).
- **Consumes:** `CORE.DEP.EDGE`, `CORE.DEP.PROP`, `CORE.JOB`, `CORE.BUS` (handler invocation), `CORE.PRJ.TREE`.

## Decomposition hints

ORDER (topological order and tie-break), RUN (per-element handler invocation and failure isolation), COMMIT (single entry and rollback), PAR (order-independent parallel levels), CMD (command and scope options).

## Open questions

- Recompute calls element handlers without per-element journal entries; the bus must offer a "handler-only" path for system principals. Confirm the interface with `CORE.BUS.PIPE`.

## Children

_None yet: status `open`._
