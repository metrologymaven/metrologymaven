---
id: GDT.RES.PACE
title: Review pacing gate for the first-review subset
level: 3
parent: GDT.RES
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.RES-012]
covers: []
depends_on: []
uses_artifacts: [GDT.RES.STATUS, GDT.RES.SCHEMA]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# GDT.RES.PACE: Review pacing gate for the first-review subset

## Purpose

D-045 paces GD&T work to expert review: until every interpretation rule of the **first-review subset** is signed off, no DMT row outside the subset may be added or changed, and no GDT leaf that serves only callouts outside it may be claimed or marked built. The subset is datum reference frames from planar and cylindrical datum features; position RFS and at MMC with bonus (with the size rows those need); flatness; and profile of a surface. This leaf is the CI gate that enforces the rule mechanically, so agent build speed cannot outrun review by accident. Shared machinery (schema, resolver, workflow, record, zone engine, lint) is not paced.

## Scope

**In:**
- The pacing manifest `spec/gdt/pace.toml` (format `gdt-pace/v1`): DMT key patterns of the subset; GDT node IDs classified `subset` or `shared`; the list of GDT leaves already built when D-045 was accepted (`existing`)
- Gate checks PC1 to PC5 below, over a base and a head revision of the spec tree, the DMT table and the status register
- A CI target in the spec-check workflow (added in a separate CI pull request, AGENTS.md working rule 6)

**Out:**
- Rule statuses (owned by `GDT.RES.STATUS`, read as the register artifact)
- The row inventory and its coverage check (owned by `GDT.RES.ROWASME.INV`)
- Changing the subset (a new decision record amending D-045)

## Requirements

- **REQ-GDT.RES.PACE-001** (PC1) While the subset is incomplete, the gate shall fail for every DMT row key that the head table adds, or whose content digest changes from base to head, when the key matches no subset pattern, except a row with status `unsupported` and an empty chain (support-matrix placeholder).
- **REQ-GDT.RES.PACE-002** (PC2) While the subset is incomplete, the gate shall fail for every GDT leaf classified as neither `subset`, `shared` nor `existing` whose `claimed_by` becomes non-empty or which gains `built_in` between base and head.
- **REQ-GDT.RES.PACE-003** (PC3) A GDT leaf that the manifest does not classify shall be treated as serving only callouts outside the subset (fail closed).
- **REQ-GDT.RES.PACE-004** (PC4) The subset shall count as complete exactly when every rule in the head status register whose key matches a subset pattern has its latest revision `signed_off`, and every subset pattern matches at least one such rule; the report shall then state `subset_complete` and PC1 and PC2 shall not fire.
- **REQ-GDT.RES.PACE-005** (PC5) If the pacing manifest changes between base and head without a new or changed file under `decisions/` in the same change, then the gate shall fail with `manifest_without_decision`.
- **REQ-GDT.RES.PACE-006** The report shall list findings sorted by (check, ID), state pass or fail and whether the subset is complete, and record the digests of the manifest, both tables and the register it read.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/pace/subset_row/` (head adds a flatness row; subset incomplete), when `python tools/gdt_pace.py --base base/ --head head/ --json` runs, then it exits 0 with no findings.
- **AC-2** (error path) Given `beyond_row/` (head adds a cylindricity row; subset incomplete), then it exits 1 with one PC1 finding naming the key.
- **AC-3** Given `placeholder/` (head adds a cylindricity row with status `unsupported` and an empty chain), then it exits 0.
- **AC-4** (error path) Given `beyond_leaf/` (head sets `built_in` on a leaf classified neither subset nor shared), then PC2 fires naming the leaf; given `unclassified_leaf/`, then PC2 fires through PC3.
- **AC-5** Given `complete/` (every subset rule's latest revision `signed_off` in the head register), then the same changes as AC-2 and AC-4 pass and the report says `subset_complete`.
- **AC-6** (error path) Given `manifest_change/` (pace.toml edited, no decision file changed), then `manifest_without_decision`; with a changed decision file, it passes PC5.
- **AC-7** (determinism) Running AC-2 twice gives byte-identical JSON.

## Design

- **Command:** `python tools/gdt_pace.py --base <dir> --head <dir> [--json]`; each directory holds `spec/` (or a git tree export), the DMT table JSON and the status register JSON. Project tooling, standard library only (REQ-ECO.COMM-001 style); nearest class R1. Exit 0 pass, 1 findings, 2 unreadable input.
- **Manifest (`gdt-pace/v1`, TOML):** `[subset] dmt_keys = [...]` (glob patterns over the DMT key grammar, for example `dmt:asme/flatness/*`), `nodes = [...]`; `[shared] nodes = [...]`; `[existing] nodes = [...]`. Indicative initial classification: `subset` covers `GDT.DRF.*`, `GDT.FORM.FLAT`, `GDT.LOC.POS.RFS`, `GDT.LOC.POS.MMC`, `GDT.LOC.POS.CMD`, `GDT.ZONE.MATL.BONUS`, `GDT.DIM.SIZEASME.ENVLP`, `GDT.PROF.SURF.*`, `GDT.PROF.ZDEF.UNIF`; `shared` covers `GDT.CHAR.*`, `GDT.RES.*`, `GDT.ZONE.ZTYPE`, `GDT.ZONE.POSE.*`; `existing` lists every GDT leaf with `built_in` at the merge of D-045. The maintainers confirm the classification in the PR that adds the file.
- **Algorithm:** parse both spec trees' frontmatter (same parser rules as `tools/scope.py`); diff `claimed_by` and `built_in` per GDT leaf; diff the two tables by key and content digest (digest rules from `GDT.RES.SCHEMA`); compute PC4 from the head register; collect and sort findings.
- **Errors:** `manifest_invalid`, `table_invalid`, `register_invalid` (exit 2); findings are data.
- **Determinism:** D0; no clock, no network, sorted output.
- **Crate:** none; Python tool `tools/gdt_pace.py` with tests under `tools/tests/`.

## Test fixtures

Small synthetic base/head pairs under `fixtures/gdt/pace/` (a few NODE.md frontmatter stubs, tiny DMT tables with invented clause numbers, hand-written registers), hand-labelled from the checks above; CC0.

## Build prompt

> Write `tools/gdt_pace.py` (Python 3.12, standard library only) with tests under `tools/tests/`. It enforces the D-045 review pacing rule. Inputs: `--base <dir>` and `--head <dir>`, each holding a `spec/` tree, a DMT table JSON and an interpretation status register JSON (`gdt-rule-status/v1`), plus the manifest `spec/gdt/pace.toml` (`gdt-pace/v1`: `[subset] dmt_keys` glob patterns and `nodes`, `[shared] nodes`, `[existing] nodes`). Checks while the subset is incomplete: PC1 a DMT key added, or with a changed content digest, matching no subset pattern fails (except status `unsupported` with an empty chain); PC2 a GDT leaf not classified `subset`, `shared` or `existing` that gains a non-empty `claimed_by` or a `built_in` fails; PC3 unclassified GDT leaves count as outside the subset. PC4: the subset is complete when every head-register rule matching a subset pattern has its latest revision `signed_off` and every pattern matches at least one rule; then report `subset_complete` and skip PC1 and PC2. PC5: a manifest change with no new or changed file under `decisions/` fails with `manifest_without_decision`. Report findings sorted by (check, ID), pass/fail, subset state and the digests of manifest, tables and register; `--json`; exit 0, 1 or 2. Tests per AC-1..AC-7 with hand-labelled fixtures under `fixtures/gdt/pace/`. Never read or emit standards text.

## Open questions

- The gate needs the DMT table and register at both revisions; if the DMT data lives in a separate repository, its CI runs the gate there and the spec-tree CI runs PC2, PC3 and PC5 only.
- Whether pacing should also refuse `gdt.propose_dmt_row@1` at run time for keys outside the subset is left open; the CI gate is enough while all rows enter through pull requests.

## Children

_Leaf: none._
