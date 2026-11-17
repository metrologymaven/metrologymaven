---
id: MIG.PROJ.CHAR
title: Characteristic, tolerance and datum mapping
level: 3
parent: MIG.PROJ
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.PROJ-004]
covers: []
depends_on: [GDT.CHAR.MODEL, GDT.CHAR.SPEC, GDT.CHAR.IDENT, MIG.PROJ.XID, MIG.PROJ.MAPREP, MIG.PROJ.PROFILE, IO.DMIS.OUT, IO.QIF.IDS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# MIG.PROJ.CHAR: Characteristic, tolerance and datum mapping

## Purpose

Maps incumbent characteristics (dimensions and geometric tolerances with their limits, feature references and datum references) onto the QIF-based characteristic model so our kernel can evaluate them on imported points. The incumbent identifier becomes the characteristic's imported identity, so parallel-run can pair results. The GD&T dialect is set only when the export states it; otherwise the characteristic has no dialect and stays `NOT_DECIDABLE` until a person chooses one, because guessing between ASME and ISO interpretations would change results silently.

## Scope

**In:**
- Mapping from QIF characteristic definitions and items (identity preserved), DMIS output tolerance statements (via `IO.DMIS.OUT`), and documented CSV feature/characteristic exports (via profile column maps)
- Characteristic type mapping table (incumbent characteristic kind → our characteristic kind) as profile data, with unknown kinds `UNSUPPORTED`
- Tolerance limits with authored decimals and units kept; datum references resolved to imported features by external ID
- Dialect rule: stated dialect (and edition) set through `GDT.CHAR.SPEC`; otherwise unset
- Mapping-report items with reasons `dialect_unset` (`PARTIAL`), `datum_unresolved` (`PARTIAL`), `unsupported_construct` (`UNSUPPORTED`)

**Out:**
- Characteristic model and dialect refusal behaviour (owned by `GDT.CHAR.MODEL` and `GDT.CHAR.SPEC`)
- Evaluating the characteristics (owned by `GDT`)
- Results-only import of incumbent values (owned by `IO.RES.TRANS`)

## Requirements

- **REQ-MIG.PROJ.CHAR-001** For each characteristic record the profile maps, the system shall create a characteristic with its type, limits (authored value and unit), feature references and datum references, with identity from the incumbent identifier through `GDT.CHAR.IDENT` and an `ExternalId`.
- **REQ-MIG.PROJ.CHAR-002** If the export states a dialect and edition for the characteristic or the whole file, then the characteristic shall receive that governing specification; otherwise the characteristic shall have no dialect and its item shall be `PARTIAL` with reason `dialect_unset`.
- **REQ-MIG.PROJ.CHAR-003** A characteristic without a dialect shall evaluate to `NOT_DECIDABLE` until a human principal sets its governing specification, and an agent principal's attempt to set it shall create a proposal on the agent branch only.
- **REQ-MIG.PROJ.CHAR-004** If a datum reference names a feature that the import did not create, then the characteristic shall be created without that datum and its item shall be `PARTIAL` with reason `datum_unresolved`.
- **REQ-MIG.PROJ.CHAR-005** If the incumbent characteristic kind has no entry in the profile's type mapping table, then no characteristic shall be created and the item shall be `UNSUPPORTED` with reason `unsupported_construct`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/proj/char/qif-asme/` (QIF file stating ASME Y14.5-2018 with 8 characteristics), when mapped, then 8 characteristics exist with QIF identities preserved and the governing specification set, all items `MAPPED`.
- **AC-2** Given `dmis-out-nodialect/` (DMIS output with 5 tolerance results and no dialect statement), then 5 characteristics exist with no dialect, items `PARTIAL dialect_unset`, and evaluating any of them gives `NOT_DECIDABLE`.
- **AC-3** Given AC-2, when an agent principal sets the dialect, then the change exists only on its agent branch and the working characteristic still has no dialect.
- **AC-4** Given `csv-missing-datum/` (a position tolerance referencing datum `C` not in the file), then the item is `PARTIAL datum_unresolved` and the characteristic has two datum references.
- **AC-5** Given `csv-unknown-kind/` (kind `profile_of_line_freeform` absent from the mapping table), then the item is `UNSUPPORTED unsupported_construct`.

## Design

- **Registry command(s):** none; library called by `migrate.import_project_export@1` (`MIG.PROJ.PROJECT`). Setting a dialect afterwards uses the existing GDT command owned by `GDT.CHAR.SPEC`.
- **Data structures:** `CharMapping { kind_table: BTreeMap<IncumbentKind, CharKind>, limit_fields, datum_fields }` (profile data); output: characteristic entities per `GDT.CHAR.MODEL` with `ExternalId`.
- **Algorithm:** per record: kind lookup → limits with authored decimal → feature reference by external ID lookup (XID index of this import) → datum references → dialect from record or file header → create through the GDT model builder → MAPREP item.
- **Errors:** recorded as report items; structural errors (`profile_schema`) raised.
- **Determinism:** D0.
- **Performance budget:** 5,000 characteristics in under 300 ms [unverified until measured].
- **Crate:** `fid-mig` (module `proj::char`).

## Test fixtures

`fixtures/mig/proj/char/`: QIF, DMIS output and CSV files authored for the project (not exported from an incumbent), CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `proj::char` implementing spec node MIG.PROJ.CHAR: map profile-selected characteristic records from QIF (identity preserved), DMIS output tolerance rows and CSV exports to characteristics on the GDT characteristic model with authored limits and units, feature references and datum references resolved by ExternalId; identity through the GDT characteristic identity library plus `proj::xid`. Set the governing specification only when the export states dialect and edition; otherwise leave it unset and add a `PARTIAL dialect_unset` item (evaluation stays `NOT_DECIDABLE`; agent attempts to set it go to the agent branch). Unresolved datum → create without it, `PARTIAL datum_unresolved`; unknown kind → no entity, `UNSUPPORTED unsupported_construct`. Never guess a dialect. Tests for AC-1..AC-5. No `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- None.

## Children

_Leaf: none._
