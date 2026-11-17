---
id: IO.RES.AQDEF.KSET
title: Published AQDEF K-field subset table
level: 4
parent: IO.RES.AQDEF
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.RES.AQDEF-001]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.RES.AQDEF.KSET: Published AQDEF K-field subset table

## Purpose

The data table that bounds what the AQDEF reader and writer may touch: each K-field the platform uses, its role, value type and length, how it maps to a ResultRecord or transcription field, and the public document that describes it. Only fields with a public documentation source may enter the table, which keeps the AQDEF support uncertified but clean.

## Scope

**In:**
- `aqdef-kset@1.toml`: rows of K-number, level (part, characteristic, value), our own short description, value type, maximum length, mapping to platform fields, public source citation (document title, version, section)
- Profiles naming subsets for consumers (`default`, `piweb`)
- A CI check that every row has a source and every profile names only table rows
- Generated documentation page including the "not Q-DAS certified" statement

**Out:**
- Reading and writing DFQ files (`IO.RES.AQDEF.IN`, `IO.RES.AQDEF.OUT`)
- Certification (a commercial matter; out of scope)

## Requirements

- **REQ-IO.RES.AQDEF.KSET-001** Every row of the table shall cite a public documentation source for its K-field, and CI shall fail with `kfield_source_missing` for any row without one.
- **REQ-IO.RES.AQDEF.KSET-002** Descriptions in the table shall be written in the project's own words and shall not copy text from the cited documents.
- **REQ-IO.RES.AQDEF.KSET-003** Each profile shall list only K-fields present in the table, and CI shall fail with `kfield_profile_unknown` otherwise.
- **REQ-IO.RES.AQDEF.KSET-004** The generated documentation page shall state that the AQDEF support is not Q-DAS certified.
- **REQ-IO.RES.AQDEF.KSET-005** The table shall be versioned, and CI shall fail with `kset_version_not_bumped` when it changes without a version increase.

## Acceptance criteria

- **AC-1** Given the committed table, when the CI check runs, then it passes and the generated page contains the uncertified statement.
- **AC-2** (error path) Given a test copy with one row lacking `source`, then `kfield_source_missing`.
- **AC-3** (error path) Given profile `piweb` naming a K-number absent from the table, then `kfield_profile_unknown`.
- **AC-4** (error path) Given a row edit without a version bump, then `kset_version_not_bumped`.

## Design

- **Registry commands:** none (data plus CI check).
- **Data structures:** `KField { k: u16, level: Part|Char|Value, desc, ty: Text|Decimal|Int|Date|Enum, max_len, maps_to: Field, source: Citation }`; `Profile { name, fields: Vec<u16> }`.
- **Algorithm:** load and validate the TOML; render the doc page.
- **Errors:** `kfield_source_missing`, `kfield_profile_unknown`, `kset_version_not_bumped`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-io-res` (module `aqdef::kset`).

## Test fixtures

The committed table and test copies under `fixtures/io/res/aqdef/kset/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-res`, add module `aqdef::kset`: load `aqdef-kset@1.toml` (K-number, level, own-words description, value type, max length, platform field mapping, public source citation; profiles `default` and `piweb`), validate it in CI (`kfield_source_missing`, `kfield_profile_unknown`, `kset_version_not_bumped`), and render a documentation page that states the support is not Q-DAS certified. Never copy text from the cited documents. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-4.

## Open questions

- Which K-fields are publicly documented, and under what terms the AQDEF format description may be used, is [unverified]; a maintainer must fill the citations from the public documents before OUT and IN ship.

## Children

_Leaf: none._
