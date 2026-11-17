---
id: ALN.DATUM.SYS.CMD
title: Datum-system alignment command and recording
level: 4
parent: ALN.DATUM.SYS
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.DATUM.SYS-001, REQ-ALN.DATUM.SYS-002, REQ-ALN.DATUM.SYS-004, REQ-ALN.DATUM.SYS-006, REQ-ALN.DATUM.SYS-007]
covers: [GOM.ALN.07, PCD.ALN.05, PW.ALN.06]
depends_on: [ALN.DATUM.SYS.GATE, ALN.DATUM.SYS.BUILD, GDT.DRF.DATUM, ALN.MODEL.ELEM, ALN.QA.REPORT]
side_effect: W2
determinism: D3
cli: [fid align run-datum-system]
mcp: [align_run_datum_system]
journal: [align.run_datum_system@1]
updated: 2026-11-17
---

# ALN.DATUM.SYS.CMD: Datum-system alignment command and recording

## Purpose

The registry command `align.run_datum_system@1` (GOM, PC-DMIS and PolyWorks datum-based alignments): resolves datum labels to their datum-feature definitions and measured data, runs `ALN.DATUM.SYS.GATE` then `ALN.DATUM.SYS.BUILD`, and wraps the result as an `Alignment` element recording the full interpretation context, so GDT can use it as the starting frame for mobility-aware evaluation.

## Scope

**In:** parameters; datum-label resolution through GDT's datum definitions; GATE and BUILD calls; recording of dialect, edition, profile, row IDs, chain digests, DMT digest, DoF table and free DoF; solver registration for recompute; dry-run.
**Out:** the gate and build logic (siblings); datum definitions (`GDT.DRF.DATUM`); mobility (`GDT.DRF.MOB`); ISO enablement (`ALN.DATUM.ISO`).

## Requirements

- **REQ-ALN.DATUM.SYS.CMD-001** When `align.run_datum_system` commits, the system shall create one alignment whose parameters record dialect, edition, defaults profile, each datum's label, row ID, variant and chain digest, the combined chain digest, the DMT digest, the per-datum constrained DoF, the free DoF, the policy and the effective determinism class, and journal one event.
- **REQ-ALN.DATUM.SYS.CMD-002** If a datum label has no definition or its feature has no measured data, then the command shall fail with `datum_undefined` or `datum_not_measured`; GATE and BUILD errors (including `dialect_unspecified`, `UNRESOLVED`, `UNSUPPORTED`, `underconstrained`) shall pass through; in every error case nothing is created or journaled.
- **REQ-ALN.DATUM.SYS.CMD-003** The command shall accept one to three datums; more shall fail with `too_many_datums`.
- **REQ-ALN.DATUM.SYS.CMD-004** With `--dry-run`, the command shall run as R1 and return the would-be transform, chain record and free DoF, journaling nothing.
- **REQ-ALN.DATUM.SYS.CMD-005** The command shall register the solver with `ALN.MODEL.ELEM` so that a change to a datum definition, its measured data, the DMT table digest or the parent recomputes the alignment.
- **REQ-ALN.DATUM.SYS.CMD-006** The registry entry shall declare class D3, and the alignment shall carry the effective class from BUILD.

## Acceptance criteria

- **AC-1** Given project fixture `fixtures/aln/datum/sys/block_abc.fidproj` (datums A, B, C defined, ASME test DMT), when `fid align run-datum-system --datums A,B,C --dialect asme --edition Y14.5-2018 --defaults-profile asme_test --unconstrained reject --json` runs, then one alignment is created with the known transform within 1e-9 m and 1e-9 rad, and its parameters contain three row IDs and the combined digest.
- **AC-2** (error path) Given no `--defaults-profile`, then the command fails with `dialect_unspecified` and journals nothing.
- **AC-3** (error path) Given `--dialect iso` while ISO rows are unsupported, then the command fails with `UNSUPPORTED` and the key, and journals nothing.
- **AC-4** Given `--dry-run`, then no element or event is created.
- **AC-5** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command:** `align.run_datum_system@1` (W2; dry-run R1; declared D3). Params: `datums: [DatumLabel; 1..=3]` in precedence order, `dialect`, `edition`, `defaults_profile`, `unconstrained: keep_parent | reject` (default `reject`), `parent?`. Result: `{ alignment, free_dof, combined_chain_digest, effective_class }`.
- **Algorithm:** resolve labels (GDT.DRF.DATUM definitions in the document); GATE; BUILD with the parent's composed pose; wrap through `ALN.MODEL.ELEM`.
- **Errors:** `datum_undefined`, `datum_not_measured`, `too_many_datums`, GATE and BUILD errors passed through.
- **Determinism:** declared D3; effective class recorded.
- **Performance budget:** command overhead under 5 ms [unverified until measured].
- **Crate:** `fid-align` (command layer; provisional name).

## Test fixtures

`block_abc.fidproj` built from `ALN.DATUM.SYS.FIXT`; test DMT table from GATE's fixtures (CC0).

## Build prompt

> In crate `fid-align` (command layer), register `align.run_datum_system@1` (W2, dry-run R1, declared D3; CLI `fid align run-datum-system`; MCP `align_run_datum_system`; journal `align.run_datum_system@1`). Params: datums (1-3 labels in precedence; else `too_many_datums`), dialect, edition, defaults_profile, `unconstrained: keep_parent|reject` (default reject), parent. Resolve labels to GDT datum definitions and measured data (`datum_undefined`, `datum_not_measured`); call `resolve_datum_chain` (ALN.DATUM.SYS.GATE) then `build_datum_alignment` (ALN.DATUM.SYS.BUILD) with the parent's composed pose; wrap with the alignment element API (ALN.MODEL.ELEM) recording dialect, edition, profile, per-datum row IDs, variants and chain digests, combined digest, DMT digest, DoF table, free DoF, policy and effective class; register for recompute with the DMT digest as an input. Any error: nothing created or journaled. Tests: AC-1..AC-5. Constraints: CONSTITUTION §3, §5 (no standards text), §8 (no `unwrap`/`expect`/`panic`/`unsafe`).

## Open questions

- None.

## Children

_Leaf: none._
