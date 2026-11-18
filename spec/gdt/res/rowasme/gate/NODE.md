---
id: GDT.RES.ROWASME.GATE
title: Dialect row-set release gate
level: 4
parent: GDT.RES.ROWASME
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.RES.ROWASME-001, REQ-GDT.RES.ROWASME-002, REQ-GDT.RES.ROWASME-003, REQ-GDT.RES.ROWASME-004]
covers: []
depends_on: [GDT.RES.ROWASME.INV, GDT.RES.LINT, GDT.RES.ROWFLOW.STATE, GDT.RES.ROWFLOW.ROSTER]
uses_artifacts: [UNC.ORS.RUN, GDT.RES.ROWFLOW.ADOPT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.RES.ROWASME.GATE: Dialect row-set release gate

## Purpose

The set-level check that a dialect's row set may be called released for a phase: every inventory key present at the required status, every row signed off by two roster reviewers with edition-held attestations, every supported row's ORS cases green at the release commit, and no agent principal anywhere in a row's authorship or adoption. Per-row adoption (`GDT.RES.ROWFLOW.ADOPT`) checks one row at a time; this gate checks the whole table against the inventory and the ORS run, and runs in CI on release branches. It is dialect-agnostic, so `GDT.RES.ROWISO` reuses it.

## Scope

**In:** `gate(table, inventory, ors_run_stats, adoption_records, roster, config) -> GateReport`; checks G1–G5 below; a CI target for release branches; config (`allowed_author_basis`, release commit).
**Out:** per-row workflow (owned by `GDT.RES.ROWFLOW`); lint rules (owned by `GDT.RES.LINT`); ORS execution (owned by `UNC.ORS.RUN`, whose per-suite statistics JSON is read as an artifact).

## Requirements

- **REQ-GDT.RES.ROWASME.GATE-001** (G1) The gate shall fail when `GDT.RES.ROWASME.INV` coverage reports any finding for the release's inventory.
- **REQ-GDT.RES.ROWASME.GATE-002** (G2) The gate shall fail when `GDT.RES.LINT` in release mode reports any finding on the table.
- **REQ-GDT.RES.ROWASME.GATE-003** (G3) The gate shall fail for every `supported` row with a linked ORS case that is absent from, or not `pass` in, the ORS run statistics produced at the release commit named in the config.
- **REQ-GDT.RES.ROWASME.GATE-004** (G4) The gate shall fail for every row whose adoption record names an agent principal as author (without a human re-proposal), reviewer or adopter.
- **REQ-GDT.RES.ROWASME.GATE-005** (G5) The gate shall fail for every row whose human author's roster basis is not in the config's `allowed_author_basis` (for the P1a ASME set: `paid_author` or `partner`, per D-009) or whose reviews lack an edition-held attestation for a cited edition (D-016).
- **REQ-GDT.RES.ROWASME.GATE-006** The report shall list findings sorted by (rule, key), state pass or fail, and record the table digest, inventory digest, ORS statistics digest and roster digest it checked.

## Acceptance criteria

- **AC-1** Given fixture set `fixtures/gdt/gate/ok/` (synthetic table, inventory, ORS statistics with all linked cases passing, adoption records, roster), when the gate runs, then it passes with zero findings and records the four digests.
- **AC-2** (error paths) Given each fixture in `fixtures/gdt/gate/bad/` (missing key, lint L5 failure, ORS case failing, ORS run from another commit, agent reviewer in an adoption record, community-basis author with `allowed_author_basis: [paid_author]`, missing attestation), then the matching rule G1–G5 fires naming the key.
- **AC-3** Running the gate twice on AC-1 gives a byte-identical report.

## Design

- **Registry command(s):** none; a library and a CI target (`cargo test -p fid-gdt --test dmt_release_gate` on release branches).
- **Data structures:** `GateConfig { release_commit, allowed_author_basis, inventory_path }`; `GateReport { pass, findings: Vec<GateFinding{rule, key, detail}>, digests }`; adoption records as journaled by `gdt.adopt_dmt_row@1`.
- **Algorithm:** run INV coverage and LINT release mode; join rows with ORS statistics by case ID; scan adoption records and roster; sort findings.
- **Errors:** input load errors propagate; findings are data.
- **Determinism:** D0.
- **Performance budget:** 5 000 rows, 20 000 ORS outcomes < 500 ms [unverified].
- **Crate:** `fid-gdt` (module `dmt_gate`).

## Test fixtures

Synthetic tables, inventories, ORS statistics, adoption records and rosters (invented clause numbers, lorem-ipsum notes; CC0, in-repo).

## Build prompt

> In crate `fid-gdt`, add module `dmt_gate` with `gate(table, inventory, ors_stats, adoptions, roster, cfg) -> GateReport`. Rules: G1 any inventory coverage finding (`dmt_inventory::coverage`); G2 any release-mode lint finding (`dmt_lint`); G3 a `supported` row's linked ORS case missing from or not passing in the ORS per-suite statistics JSON produced at `cfg.release_commit`; G4 an agent principal as author (without human re-proposal), reviewer or adopter in the row's adoption record; G5 author roster basis not in `cfg.allowed_author_basis`, or a review lacking an edition-held attestation for a cited edition. Report sorted by (rule, key) with pass/fail and the digests of table, inventory, ORS statistics and roster; byte-identical for identical inputs. Add a CI test target run on release branches. Tests per the acceptance criteria with synthetic fixtures only. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`, never store or emit standards text.

## Open questions

- **Revise for D-036/D-037 (accepted 2026-11-17):** DMT row sign-off is gate H4 (once per row); agents propose rows with evidence.
- The ORS per-suite statistics JSON shape is published by `UNC.ORS.RUN` (REQ-UNC.ORS.RUN-005); its schema version must be named in that node's Interfaces before this leaf is implemented.

## Children

_Leaf: none._
