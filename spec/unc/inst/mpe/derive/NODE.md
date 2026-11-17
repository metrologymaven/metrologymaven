---
id: UNC.INST.MPE.DERIVE
title: Mapping-table format, review state and derivation command
level: 4
parent: UNC.INST.MPE
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.INST.MPE-002, REQ-UNC.INST.MPE-003, REQ-UNC.INST.MPE-004, REQ-UNC.INST.MPE-005]
covers: [SA.UNC.01]
depends_on: [UNC.INST.SCHEMA, UNC.MODEL.GRAPH.SCHEMA, UNC.MODEL.DIST]
side_effect: W2
determinism: D0
cli: [fid unc derive-instrument-model]
mcp: [unc_derive_instrument_model]
journal: [unc.derive_instrument_model@1]
updated: 2026-11-17
---

# UNC.INST.MPE.DERIVE: Mapping-table format, review state and derivation command

## Purpose

The machinery every ISO 10360 mapping table uses: the `mpe-map/v1` table format (term symbol, MPE formula shape, contributor it maps to, distribution assumption, task applicability limits, clause citation), its review state, a lint that keeps standards text out, and `unc.derive_instrument_model@1`, which turns a maker's datasheet values into an `instrument-model/v1` of kind `spec_derived` capped at UEC-1, plus a derivation record that profiles cite. Table content lives in the per-part children.

## Scope

**In:**
- `mpe-map/v1` schema: `map_id` (`mpe-map/<part>@<semver>`), standard reference (designator, edition), `terms [{symbol, clause, formula: constant | constant_plus_length {a, b, unit_a, length_divisor} | min_of(...) , contributor_id, distribution: rectangular | normal_k{k}, applicability {task_kinds, max_length}}]`, `review {authors, reviewers, status, edition_held}`
- Lint: symbols match `[A-Za-z0-9_,.]{1,24}`, free text ≤ 280 characters, no field other than `note` holds prose
- `unc.derive_instrument_model@1` (W2): `map_id`, datasheet values, device class, source citation; result model plus `DerivationRecord`
- UEC cap: derived models carry `uec_max: 1`

**Out:**
- Table content per part (owned by `UNC.INST.MPE.CMM`, `.ARM`, `.TRK`, `.OPT`)
- Model storage and import (owned by `UNC.INST.SCHEMA`)
- Evidence-class assignment on results (owned by `UNC.UEC.CLASS`, which reads `uec_max`)

## Requirements

- **REQ-UNC.INST.MPE.DERIVE-001** Every derived model shall list, for each MPE term used, the term symbol, contributor ID, distribution assumption, MPE value with unit and the clause cited, copied from the mapping table and the datasheet input.
- **REQ-UNC.INST.MPE.DERIVE-002** Every derived model shall be kind `spec_derived` with `uec_max` 1, and its derivation record shall carry the mapping table ID and digest.
- **REQ-UNC.INST.MPE.DERIVE-003** If a datasheet value required by a term of the mapping table is missing, then derivation shall fail with `term_missing` naming the symbol and commit nothing.
- **REQ-UNC.INST.MPE.DERIVE-004** If the mapping table's review status is not `reviewed`, then derivation shall fail with `map_not_reviewed`, except under `--dry-run`, which shall mark the scratch result `draft_map`.
- **REQ-UNC.INST.MPE.DERIVE-005** If a mapping table field other than `note` contains text that does not match the symbol grammar, or `note` exceeds 280 characters, then the lint shall fail with `text_not_allowed` or `text_too_long`.
- **REQ-UNC.INST.MPE.DERIVE-006** For a term with a rectangular assumption over ±MPE, the derived standard uncertainty shall be MPE/√3, and for `normal_k{k}` it shall be MPE/k, evaluated at the task length where the formula is length dependent.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/inst/mpe/map_example.json` (an invented two-term table marked `reviewed`, symbols `EX_L` and `EX_P`) and `datasheet_example.json` (EX_L = 1.5 µm + L/333 with L in mm, EX_P = 1.9 µm), when `fid unc derive-instrument-model --map mpe-map/example@1.0.0 --values <handle> --json` runs, then the model has two contributors, is `spec_derived` with `uec_max` 1, and at L = 0.3 m the EX_L standard uncertainty equals the value in `fixtures/unc/inst/mpe/expected.json` (MPE at 300 mm divided by √3) within 1 ulp.
- **AC-2** (error path) Given a datasheet without `EX_P`, then derivation fails with `term_missing` naming `EX_P` and journals nothing.
- **AC-3** (review) Given the same table with status `draft`, then derivation fails with `map_not_reviewed`; with `--dry-run` it returns a scratch result flagged `draft_map`.
- **AC-4** (lint) Given `map_prose.json` (a 300-character note and a symbol containing spaces), then lint fails with `text_too_long` and `text_not_allowed`.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `unc.derive_instrument_model@1` (W2; `map: MapId`, `values: ImportHandle | inline`, `device_class`, `source: UntrustedText`; returns `{ model: InstrumentModel, record: DerivationRecord }`); `--dry-run` returns R1 scratch.
- **Data structures:** `MpeMap`, `MpeTerm`, `MpeFormula`, `Applicability`, `DerivationRecord { map_id, map_digest, values_digest, model_digest }`.
- **Algorithm:** lint table; check review; resolve each term's value; build one input node per term (distribution from `UNC.MODEL.DIST`) with the term's contributor ID and applicability as range axes of the model; canonicalize and digest through `UNC.INST.SCHEMA`.
- **Errors:** `term_missing`, `map_not_reviewed`, `text_not_allowed`, `text_too_long`, `map_not_found`, `unit_mismatch`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-unc-inst` (module `mpe`).

## Test fixtures

`fixtures/unc/inst/mpe/`: an invented `example` table and datasheet (CC-BY-4.0); no real ISO term symbols or maker values, so the fixtures carry no standards content.

## Build prompt

> In crate `fid-unc-inst`, module `mpe`, define `mpe-map/v1` (`map_id` `mpe-map/<part>@<semver>`, standard designator and edition, terms `{symbol, clause, formula (constant | constant_plus_length | min_of), contributor_id, distribution (rectangular | normal_k), applicability}`, review `{authors, reviewers, status, edition_held}`) with a lint: symbols match `[A-Za-z0-9_,.]{1,24}`, only `note` may hold prose and only ≤ 280 chars (`text_not_allowed`, `text_too_long`).
> Implement `unc.derive_instrument_model@1` (W2; CLI `fid unc derive-instrument-model`; MCP `unc_derive_instrument_model`; `--json`, `--dry-run`): require table status `reviewed` (`map_not_reviewed`, except dry-run which flags `draft_map`), fail `term_missing{symbol}` for absent datasheet values, produce an `instrument-model/v1` of kind `spec_derived`, `uec_max` 1, one input per term (u = MPE/√3 for rectangular, MPE/k for normal_k, at the task length), applicability as range axes, plus a `DerivationRecord {map_id, map_digest, values_digest, model_digest}`. Never store standards text. Tests AC-1..AC-5 with `fixtures/unc/inst/mpe/`. No panic or `unwrap`; BTreeMap only.

## Open questions

- ISO 10360-102 may introduce a symbol grammar that becomes the canonical identifier scheme [unverified]; the symbol pattern above may need widening.
- The number of reviewers required for a mapping table (one instrument-metrology expert, or two as D-009 sets for DMT rows) is not decided; this leaf records reviewers and checks only status `reviewed`.

## Children

_Leaf: none._
