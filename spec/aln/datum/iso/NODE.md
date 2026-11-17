---
id: ALN.DATUM.ISO
title: ISO 5459 datum-system alignment enablement
level: 3
parent: ALN.DATUM
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.DATUM-003, REQ-ALN.DATUM-006, REQ-ALN.DATUM-007]
covers: [GOM.ALN.07, PW.ALN.06]
depends_on: [ALN.DATUM.SYS.GATE, ALN.DATUM.SYS.BUILD, ALN.DATUM.SYS.CMD, GDT.DRF.COMMON]
uses_artifacts: [GDT.RES.ROWISO, ALN.DATUM.SYS.FIXT]
side_effect: W2
determinism: D3
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ALN.DATUM.ISO: ISO 5459 datum-system alignment enablement

## Purpose

Enables the ISO 5459:2011 dialect on the existing command `align.run_datum_system@1`. The engine is dialect-agnostic: GATE asks GDT's resolver, and the resolver answers from DMT rows. So this leaf adds no command. It adds the ISO dialect and edition values to the gate's accepted vocabulary, routes ISO common datums and datum targets to `GDT.DRF.COMMON` in BUILD's simulation step, publishes ISO datum-alignment fixtures, and adds the tests that prove the unsupported-to-supported switch and the ASME/ISO difference.

## Scope

**In:**
- the ISO dialect and edition identifiers in the gate's context types;
- routing of ISO common datums (for example A-B) and datum targets to GDT.DRF.COMMON's simulation;
- ISO cases in the `DatumAlignFixture` v1 format;
- tests for the `UNSUPPORTED` gate, recorded chains, noise-free recovery and the ASME/ISO chain difference.

**Out:**
- ISO DMT rows and which ISO default associations differ from ASME's (`GDT.RES.ROWISO`, authored under D-009 with texts per D-016);
- the engine, gate, build and command (`ALN.DATUM.SYS.*`);
- common-datum and datum-target simulation (`GDT.DRF.COMMON`);
- ISO GPS tolerance evaluation (`GDT`).

## Requirements

- **REQ-ALN.DATUM.ISO-001** While GDT's DMT reports ISO 5459 rows as `unsupported`, a datum-system run with dialect ISO shall fail with `UNSUPPORTED` and the row key, and journal nothing.
- **REQ-ALN.DATUM.ISO-002** When ISO rows are supported, a datum-system run with dialect ISO, edition 2011 and an ISO defaults profile shall use the simulators and order named by those rows and record them as in REQ-ALN.DATUM.SYS-004.
- **REQ-ALN.DATUM.ISO-003** Given the published ISO datum-alignment fixtures with a known transform, the recovered frame shall match within 1e-9 m and 1e-9 rad for noise-free data [unverified].
- **REQ-ALN.DATUM.ISO-004** Given a fixture where ASME and ISO default simulators differ, runs under the two dialects shall record different operator-chain digests.
- **REQ-ALN.DATUM.ISO-005** ISO runs shall meet the same KNP-1 and determinism declarations as `ALN.DATUM.SYS`.
- **REQ-ALN.DATUM.ISO-006** When a datum is an ISO common datum or is established by datum targets, BUILD shall obtain its simulated datum from GDT.DRF.COMMON, and the alignment shall record the common-datum composition or target set.
- **REQ-ALN.DATUM.ISO-007** The ISO fixtures shall include at least `block_abc_iso`, `shaft_common_ab` (common datum A-B from two coaxial cylinders) and `panel_targets` (datum targets), in the `ALN.DATUM.SYS.FIXT` format.

## Acceptance criteria

- **AC-1** Given a DMT table whose ISO rows are `unsupported`, when `fid align run-datum-system --datums A,B,C --dialect iso --edition 5459:2011 --defaults-profile iso_test --json` runs on `block_abc_iso`, then it fails with `UNSUPPORTED` and the key, and journals nothing.
- **AC-2** Given a test DMT table with supported ISO test rows, when the same command runs, then the alignment records the ISO row IDs and chain digests and matches the known transform within 1e-9 m and 1e-9 rad.
- **AC-3** Given `shaft_common_ab`, then the common datum A-B is simulated by GDT.DRF.COMMON, the free DoF are the translation along and rotation about A-B (policy `keep_parent`), and the composition is recorded.
- **AC-4** Given a fixture and test tables in which the ASME and ISO rows for the primary plane name different simulators, then the two runs' combined chain digests differ.
- **AC-5** (replay) Exact replay of AC-2 gives `IDENTICAL`.

## Design

- **Registry command(s):** none new; the ISO path of `align.run_datum_system@1` (W2, declared D3), owned by `ALN.DATUM.SYS.CMD`.
- **Data structures:** `Dialect::Iso`, `Edition::Iso5459_2011` in the gate context; `DatumRef::Common { members: [Label] }` and `DatumRef::Targets { set }` routed to GDT.DRF.COMMON.
- **Algorithm:** no interpretation in code. GATE passes the ISO context to the resolver unchanged; BUILD dispatches common datums and targets to GDT.DRF.COMMON; everything else is the ASME path.
- **Errors:** as GATE, BUILD and CMD; plus `common_datum_unsupported` passed through from GDT.DRF.COMMON.
- **Determinism:** declared D3 like the engine; effective class recorded per run.
- **Performance budget:** as ALN.DATUM.SYS.BUILD.
- **Crate:** `fid-align-solve` (gate and build additions; fixtures under `tests/datum_fixtures/`); provisional name.

## Test fixtures

ISO cases in `DatumAlignFixture` v1 (`fixtures/aln/datum/iso/`, generated, CC0). Test-only DMT tables are marked `test_only`; real ISO rows come from GDT.RES.ROWISO.

## Build prompt

> In crate `fid-align-solve`, extend the datum-system gate and build (spec nodes ALN.DATUM.SYS.GATE and ALN.DATUM.SYS.BUILD) for ISO 5459:2011 without encoding any interpretation: add `Dialect::Iso` and `Edition::Iso5459_2011` to the gate's context and pass them unchanged to the GDT resolver; in the build, route `DatumRef::Common` and `DatumRef::Targets` to GDT's common-datum simulation (GDT.DRF.COMMON) and record the composition or target set on the alignment. Add ISO fixtures `block_abc_iso`, `shaft_common_ab` and `panel_targets` in the `DatumAlignFixture` v1 format and generator. Tests (through `fid align run-datum-system`): AC-1 (`UNSUPPORTED` while ISO rows are unsupported; nothing journaled), AC-2 (supported test rows recover the transform within 1e-9 m and rad), AC-3 (common datum A-B), AC-4 (ASME and ISO runs record different chain digests when rows differ), AC-5 (replay `IDENTICAL`). Constraints: never store or paste ISO text (CONSTITUTION §5); CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`); KNP-1.

## Open questions

- Phase P1b follows CONSTITUTION §7 (ISO GPS dialect in P1b) and GDT.RES (ISO rows from P1b). This resolves the parent's phase question without moving ALN.DATUM.
- Which ISO 5459 default associations differ from ASME's for alignment needs a credentialed expert (D-009 process) [unverified]; that content lives in GDT.RES.ROWISO rows, so it does not block this leaf.
- GDT.DRF.COMMON is open and P1b; AC-3 runs once it lands.
- Layering of ALN on GDT: see ALN.DATUM.SYS's open questions.

## Children

_Leaf: none._
