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

D-045 paces GD&T work to expert review. Until every interpretation rule of the **first-review subset** is signed off, no DMT row outside the subset may be added or changed, and no GDT leaf outside the subset may be claimed or marked built. The subset covers:
- datum reference frames from planar and cylindrical datum features;
- position, RFS and at MMC with bonus, plus the size rows those need;
- flatness;
- profile of a surface.

This leaf is the CI gate that enforces the rule mechanically, so agent build speed cannot outrun review by accident. Its inputs are explicit lists, never patterns. Whether the subset is complete is decided only from registers that CI itself rebuilds and verifies, never from a register file a change supplies. Shared machinery (schema, resolver, workflow, record, zone engine, lint) is not paced.

## Scope

**In:**
- The pacing manifest `spec/gdt/pace.toml` (format `gdt-pace/v1`), with explicit lists:
  - the DMT keys of the subset;
  - the GDT leaf IDs classified `subset` or `shared`;
  - the GDT leaves already built when D-045 was accepted (`existing`).
- Rebuilding the head status register in CI from the head journal and roster. Verifying the base register against the project-published digests (`GDT.RES.STATUS`).
- Gate checks PC1 to PC5 below, comparing a base revision and a head revision of the spec tree and of the DMT table.
- A CI target in the spec-check workflow. It is added in a separate CI pull request (AGENTS.md working rule 6).

**Out:**
- Rule statuses and register verification (owned by `GDT.RES.STATUS`; this gate calls its `export-rule-status` and `verify_register`).
- The row inventory and its coverage check (owned by `GDT.RES.ROWASME.INV`).
- Changing the subset. That needs a new decision record amending D-045.

## Requirements

- **REQ-GDT.RES.PACE-001** (PC1) While the subset is incomplete, the gate shall fail for every DMT row key that is not in the manifest's subset key list and that the head table adds, or whose content digest (`row_content_digest`) changes from base to head. The one exception is a row with status `unsupported` and an empty chain (a support-matrix placeholder).
- **REQ-GDT.RES.PACE-002** (PC2) While the subset is incomplete, the gate shall fail for every GDT leaf not listed as `subset`, `shared` or `existing` whose `claimed_by` becomes non-empty, or which gains `built_in`, between base and head.
- **REQ-GDT.RES.PACE-003** (PC3) A GDT leaf that the manifest does not list shall be treated as outside the subset (fail closed).
- **REQ-GDT.RES.PACE-004** (PC4) The subset shall count as complete exactly when the **base** status register both verifies against the project-published digests and lists every key in the manifest's subset key list with its latest revision `signed_off`. An unverified base register shall count as incomplete. The gate shall rebuild the head register from the head journal and roster in CI and report its statuses, but shall never read completeness from a register file in the change.
- **REQ-GDT.RES.PACE-005** (PC5) If the pacing manifest changes between base and head, then the gate shall fail with `manifest_without_decision` unless the same change adds a new file under `decisions/` whose frontmatter lists `D-045` in `amends:`.
- **REQ-GDT.RES.PACE-006** The report shall:
  - list findings sorted by (check, ID);
  - state pass or fail, and whether the subset is complete;
  - record the digests it read: the manifest, both tables, the verified base register and the rebuilt head register.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/pace/subset_row/` (head adds the subset key `dmt:asme/flatness/none@1`; the subset is incomplete), when `python tools/gdt_pace.py --base base/ --head head/ --published published.json --json` runs, then it exits 0 with no findings.
- **AC-2** (error path) Given `beyond_row/` (head adds a cylindricity row; the subset is incomplete), then it exits 1 with one PC1 finding naming the key.
- **AC-3** Given `placeholder/` (head adds a cylindricity row with status `unsupported` and an empty chain), then it exits 0.
- **AC-4** (error path) Given `beyond_leaf/` (head sets `built_in` on `GDT.PROF.SURF.COMP`), then PC2 fires naming that leaf. Given `unclassified_leaf/`, then PC2 fires through PC3.
- **AC-5** Given `complete/` (a base register that verifies and shows every subset key `signed_off`), then the changes of AC-2 and AC-4 pass and the report says `subset_complete`.
- **AC-6** (forged completion) Given `forged_head/` (the change commits a head register file marking every subset key `signed_off`, while the base register shows them `provisional`), then the subset is incomplete and the changes of AC-2 still fail. Given `foreign_base/` (a base register built with a roster whose digest is not published), then the subset is incomplete.
- **AC-7** (error path) Given `manifest_change/` (pace.toml edited, no new decision file), then the gate fails with `manifest_without_decision`. With a new decision file whose `amends:` lists `D-045`, PC5 passes. With a changed existing decision file only, it still fails.
- **AC-8** (determinism) Running AC-2 twice gives byte-identical JSON.

## Design

- **Command:** `python tools/gdt_pace.py --base <dir> --head <dir> --published <file> [--json]`.
  - Each directory holds `spec/` (or a git tree export), the DMT table JSON, the ROWFLOW journal export and the roster.
  - The base directory also holds the base register.
  - The tool rebuilds the head register by running `fid gdt export-rule-status` on the head inputs. It verifies the base register with `fid gdt export-rule-status` and a byte comparison, against `--published` (`gdt-published/v1`).
  - Project tooling, standard library only; nearest class R1. Exit codes: 0 pass, 1 findings, 2 unreadable input.
- **Manifest (`gdt-pace/v1`, TOML).** Explicit lists only; a `*` in any entry is `manifest_invalid`.
  - `[subset] dmt_keys`. The initial list follows; the maintainers confirm the characteristic names against the inventory (`GDT.RES.ROWASME.INV`) in the PR that adds the file:
    - `dmt:asme/datum_planar/none@1`
    - `dmt:asme/datum_cylindrical/none@1`
    - `dmt:asme/position/none@1`
    - `dmt:asme/position/mmc@1`
    - `dmt:asme/size_envelope/none@1`
    - `dmt:asme/flatness/none@1`
    - `dmt:asme/profile_surface/none@1`
    - `dmt:asme/defaults_profile/none@1`
  - `[subset] nodes`. Planar and cylindrical datums at RMB only: no datum targets, no common datums, no MMB or LMB, no composite profile.
    - `GDT.DRF.DATUM`, `GDT.DRF.BUILD`
    - `GDT.DRF.SIM.FIT`, `GDT.DRF.SIM.ROCK`, `GDT.DRF.SIM.SELECT`
    - `GDT.DRF.CHECK.FIELDS`, `GDT.DRF.CHECK.MEMB`, `GDT.DRF.MOB.REPR`
    - `GDT.FORM.FLAT`
    - `GDT.LOC.POS.RFS`, `GDT.LOC.POS.MMC`, `GDT.LOC.POS.CMD`
    - `GDT.ZONE.MATL.BONUS`, `GDT.DIM.SIZEASME.ENVLP`
    - `GDT.PROF.SURF.CMD`, `GDT.PROF.SURF.PROB`, `GDT.PROF.ZDEF.UNIF`
  - `[shared] nodes`: every leaf under `GDT.CHAR` and `GDT.RES`, plus `GDT.ZONE.ZTYPE` and the leaves under `GDT.ZONE.POSE`, each listed by ID.
  - Explicitly outside the subset: `GDT.DRF.COMMON.CDAT`, `GDT.DRF.COMMON.TARGET`, `GDT.DRF.COMMON.COVER`, `GDT.DRF.MOB.BOUND`, `GDT.DRF.MOB.GROUP` and `GDT.PROF.SURF.COMP`.
  - `[existing] nodes`: every GDT leaf with `built_in` at the merge of D-045, listed by ID.
- **Algorithm:**
  1. Parse both spec trees' frontmatter with the same parser rules as `tools/scope.py`.
  2. Diff `claimed_by` and `built_in` per GDT leaf.
  3. Diff the two tables by key and recomputed content digest.
  4. Verify the base register and compute PC4 from it alone.
  5. Rebuild the head register for the report.
  6. Check PC5 against `decisions/` frontmatter.
  7. Sort the findings.
- **Errors:** `manifest_invalid`, `table_invalid`, `register_unverifiable` and `input_unreadable` (exit 2). Findings are data.
- **Determinism:** D0; no clock, no network, sorted output.
- **Crate:** none. Python tool `tools/gdt_pace.py`, with tests under `tools/tests/`. It calls the `fid` CLI built in CI for the register steps.

## Test fixtures

Small synthetic base/head pairs under `fixtures/gdt/pace/`, hand-labelled from the checks above under CC0. Each pair holds:
- a few NODE.md frontmatter stubs;
- tiny DMT tables with invented clause numbers;
- ROWFLOW journal exports;
- rosters, base registers and a `gdt-published/v1` file.

## Build prompt

> Write `tools/gdt_pace.py` (Python 3.12, standard library only) with tests under `tools/tests/`. It enforces the D-045 review pacing rule.
>
> **Inputs:**
> - `--base <dir>` and `--head <dir>`, each holding a `spec/` tree, a DMT table JSON, a ROWFLOW journal export and the roster; the base also holds its status register.
> - `--published <file>` (`gdt-published/v1`).
> - The manifest `spec/gdt/pace.toml` (`gdt-pace/v1`) with explicit lists only, no globs: `[subset] dmt_keys`, `[subset] nodes`, `[shared] nodes` and `[existing] nodes`.
>
> **Register steps:**
> - Rebuild the head register by running `fid gdt export-rule-status` on the head inputs.
> - Verify the base register by rebuilding it the same way, comparing bytes, and checking that its register and roster digests are listed in `--published`.
> - Never read a register file from the head.
>
> **Checks.** PC4 decides completeness: the subset is complete only when the verified base register lists every subset key with its latest revision `signed_off`. While it is incomplete:
> - PC1: a DMT key not in the subset list that is added, or whose recomputed content digest changes, fails. Status `unsupported` with an empty chain is exempt.
> - PC2: a GDT leaf not listed as `subset`, `shared` or `existing` that gains a non-empty `claimed_by` or a `built_in` fails.
> - PC3: unlisted GDT leaves count as outside the subset.
>
> When the subset is complete, report `subset_complete` and skip PC1 and PC2. PC5: a manifest change fails with `manifest_without_decision` unless the change adds a new decision file whose frontmatter `amends:` lists `D-045`.
>
> **Output:** report findings sorted by (check, ID), pass or fail, the subset state, and the digests of the manifest, both tables and both registers. Support `--json`. Exit 0, 1 or 2.
>
> **Tests:** per AC-1..AC-8, with hand-labelled fixtures under `fixtures/gdt/pace/`. Never read or emit standards text.

## Open questions

- The gate needs the DMT table, journal and roster at both revisions. If the DMT data lives in a separate repository, its CI runs the gate there, and the spec-tree CI runs PC2, PC3 and PC5 only.
- Whether pacing should also refuse `gdt.propose_dmt_row@1` at run time for keys outside the subset is left open. The CI gate is enough while all rows enter through pull requests.
- The characteristic names in the initial key list are placeholders until `GDT.RES.ROWASME.INV` fixes them. They must be replaced by name, never by a pattern.

## Children

_Leaf: none._
