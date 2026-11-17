---
id: ALN.PRE
title: Prealignment and picked point-pair alignment
level: 2
parent: ALN
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN-002]
covers: [GOM.ALN.01, GOM.ALN.11, PW.ALN.01, SA.XFM.06]
depends_on: [ALN.MODEL, ALN.QA, ALN.XFM, GEO, KRN]
cli: [fid align run-prealign*, fid align run-pick-pairs*]
mcp: [align_run_prealign*, align_run_pick_pairs*]
journal: [align.run_prealign*, align.run_pick_pairs*]
updated: 2026-11-16
---

# ALN.PRE: Prealignment and picked point-pair alignment

## Purpose

Gets measured data roughly onto the reference with no prior pose: an automatic, pick-free global prealignment, and a fallback that solves from a few user- or program-supplied point pairs. Its output is a coarse alignment that `ALN.BFIT` or other methods refine. It is separate because it searches the whole pose space, while every other method assumes a starting pose.

## Scope

**In:**
- Automatic prealignment of a point cloud or mesh to CAD or a reference mesh, with no picks
- Optional hints: up-direction, restricted rotation axes, symmetry handling, region of interest
- Ambiguity reporting when the part is symmetric or several poses score similarly
- Picked point-pair initial alignment (3 or more pairs, from index sets or coordinates)
- Quick alignment (SA-style few-point rough fit) as a named preset of picked pairs

**Out** (and which node owns it instead):
- Interactive picking and snapping UI (`VIZ`); this node receives already-resolved index sets or coordinates (CONSTITUTION C2)
- Refinement after prealignment (`ALN.BFIT`)
- The closed-form point-pair solver itself (`ALN.XFM`); this node calls it
- Multi-scan registration of several clouds to each other (`SCAN`, pending its open question)
- Residual statistics (`ALN.QA`)

## Requirements

- **REQ-ALN.PRE-001** When prealignment runs on measured data and a reference, the system shall return a transform without any user-picked points.
- **REQ-ALN.PRE-002** The prealignment shall bring each synthetic test part in the published prealignment corpus within 2 mm and 2 degrees of the true pose for at least 95 % of a seeded set of random starting poses [unverified].
- **REQ-ALN.PRE-003** When two or more candidate poses score within a documented tolerance of each other, the system shall report all candidates and flag the result as ambiguous rather than choose silently.
- **REQ-ALN.PRE-004** Prealignment shall be deterministic for a given seed and input (determinism class D0 or D2, declared in the registry).
- **REQ-ALN.PRE-005** When the user supplies at least 3 non-collinear point pairs, the system shall compute a rigid initial alignment from them and report the per-pair residual.
- **REQ-ALN.PRE-006** If fewer than 3 point pairs are supplied or the pairs are collinear within a documented tolerance, then the system shall reject the command with a typed error and journal nothing.
- **REQ-ALN.PRE-007** Prealignment of 1 million measured points against a 1 million-triangle reference shall finish in under 10 s on reference hardware [unverified].

## Interfaces

- **Offers:** `align.run_prealign@1` (W2), `align.run_pick_pairs@1` (W2), each producing an `Alignment` element through `ALN.MODEL`.
- **Consumes:** GEO spatial indexes and distance queries; KRN optimisation and seeded RNG; `ALN.XFM` point-pair solver; `ALN.QA` report type.

## Decomposition hints

GLOBAL (pick-free search), HINTS (constraints and ambiguity), PAIRS (picked pairs and quick alignment), CORPUS (prealignment test corpus and metrics).

## Split rationale

The split separates the **search** (GLOBAL), the **decision about its candidates** (AMBIG), the **correspondence-given fallback** (PAIRS) and the **yardstick** (CORPUS). The search is the only open-ended research piece; isolating it lets the other three be finished as leaves now and lets the corpus pick the search method.

Alternatives considered:
- **Hints inside GLOBAL.** Rejected: hint filtering and ambiguity are deterministic post-processing with their own fixtures, and keeping them out of the search keeps the search replaceable.
- **PAIRS in ALN.XFM.** Rejected: XFM owns the solver; PAIRS owns the pick-resolution and quick-preset workflow that calls it.

Phase: GLOBAL, AMBIG and CORPUS are tagged P1b (scan-to-CAD, CONSTITUTION §7); PAIRS stays P1a, so the node itself stays P1a.

## Open questions

- Prealignment against meshes and CAD is scan-to-CAD work, which the constitution places in P1b; should this child move to P1b? A phase move needs a steering decision, so it stays P1a for now.

- Resolution proposed for the phase question above: the scan-to-CAD children are tagged P1b and the node stays P1a through `ALN.PRE.PAIRS`. No phase of an existing node changed; reviewers should confirm.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ALN.PRE.GLOBAL | Pick-free global prealignment search | 001 002 004 007 | GOM.ALN.01 |
| ALN.PRE.AMBIG | Prealignment hints and ambiguity reporting (leaf) | 003 | none |
| ALN.PRE.PAIRS | Picked point-pair initial alignment and quick alignment (leaf) | 005 006 | GOM.ALN.11, PW.ALN.01, SA.XFM.06 |
| ALN.PRE.CORPUS | Prealignment test corpus, success metric and benchmark (leaf) | 002 007 | none |
