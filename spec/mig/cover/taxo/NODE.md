---
id: MIG.COVER.TAXO
title: Per-format construct taxonomies
level: 3
parent: MIG.COVER
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-MIG.COVER-001, REQ-MIG.COVER-006]
covers: []
depends_on: [IO.REG.DETECT]
uses_artifacts: [IO.DMIS.CONF]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-mig, fid-xtask]
built_pr: 482
updated: 2026-11-22
---

# MIG.COVER.TAXO: Per-format construct taxonomies

## Purpose

A coverage table must list every construct a format has, including constructs no corpus file contains, otherwise unsupported features would silently vanish from the table. This leaf owns the per-format construct taxonomy files (`mig-taxonomy@1`): a closed list of construct IDs per format (for example "point-set record", "known-transform alignment", "merged-range header cell", or a DMIS statement keyword), written in our own words, plus the check that keeps them complete and consistent.

## Scope

**In:**
- The `mig-taxonomy@1` schema: format ID, taxonomy version, constructs (ID, kind, short description in our own words, optional `dmis_keyword`)
- Initial taxonomies for each launch format: QIF project export, DMIS output, documented CSV point and feature exports, customer form workbooks; the DMIS program taxonomy is generated from the `IO.DMIS.CONF` statement list
- `cargo xtask mig taxonomy-check`: schema, uniqueness, DMIS completeness and report-closure checks

**Out:**
- Which constructs the producers map (owned by `MIG.PROJ.MAPREP`, `MIG.TEMPL.CHECK`, `MIG.PROGEQ`)
- Status computation and tables (owned by `MIG.COVER.GEN`)
- The DMIS statement list itself (owned by `IO.DMIS.CONF`)

## Requirements

- **REQ-MIG.COVER.TAXO-001** Each taxonomy shall list constructs with IDs matching `^[a-z][a-z0-9_.]{1,63}$`, unique within the format, each with a kind and a description of at most 200 characters.
- **REQ-MIG.COVER.TAXO-002** If the DMIS program taxonomy omits a statement keyword listed in the `IO.DMIS.CONF` conformance table, then the taxonomy check shall fail with `TAXO-INCOMPLETE` and name the keyword.
- **REQ-MIG.COVER.TAXO-003** If a producer report in the check input names a construct ID that the format's taxonomy does not contain, then the taxonomy check shall fail with `TAXO-UNKNOWN` and name the report and construct.
- **REQ-MIG.COVER.TAXO-004** Each taxonomy shall carry a version, and removing or renaming a construct ID shall require a major version increase, which the check shall enforce against the previous committed version.
- **REQ-MIG.COVER.TAXO-005** Taxonomy descriptions shall be authored in our own words and shall not contain text copied from incumbent documentation; each taxonomy file shall carry a reviewer attestation field that the check requires to be present.

## Acceptance criteria

- **AC-1** Given `fixtures/mig/cover/taxo/ok/` (taxonomies for `qif-project`, `dmis-output`, `csv-points`, `form-xlsx` and a DMIS conformance table extract), when `cargo xtask mig taxonomy-check --dir ok/ --dmis-conf ok/dmis-conformance.json` runs, then it exits 0.
- **AC-2** (error path) Given `dmis-missing/` (the DMIS taxonomy lacks `SNSET`), then it exits 1 with `TAXO-INCOMPLETE SNSET`.
- **AC-3** (error path) Given `ok/` plus `--reports unknown-report/` (a mapping report naming `point_set.weighted`), then it exits 1 with `TAXO-UNKNOWN`.
- **AC-4** (error path) Given `renamed/` (a construct ID renamed with only a minor version bump against `--previous ok/`), then it exits 1 with `TAXO-BREAKING`.
- **AC-5** (error path) Given `no-attest/` (attestation field empty), then it exits 1 with `TAXO-ATTEST`.

## Design

- **Registry command(s):** none; repository tool `cargo xtask mig taxonomy-check --dir <dir> [--dmis-conf <json>] [--reports <dir>] [--previous <dir>]`.
- **Data structures:** `Taxonomy { schema: "mig-taxonomy@1", format_id, version: SemVer, reviewer_attestation: { reviewer, date, statement }, constructs: Vec<Construct> }`, `Construct { id, kind: Record | Field | Statement | Layout, description, dmis_keyword? }`; stored as `taxonomies/<format_id>.json`.
- **Algorithm:** load and validate each file; DMIS completeness = set difference of conformance keywords minus taxonomy keywords (BTreeSet); report closure = every construct ID in every report is in the taxonomy; breaking-change = previous IDs minus current IDs non-empty without major bump.
- **Errors:** `TAXO-SCHEMA`, `TAXO-DUP`, `TAXO-INCOMPLETE`, `TAXO-UNKNOWN`, `TAXO-BREAKING`, `TAXO-ATTEST`.
- **Determinism:** D0.
- **Performance budget:** all taxonomies checked in under 1 s [unverified until measured].
- **Crate:** `fid-mig` (module `cover::taxo`), exposed through `fid-xtask`.

## Test fixtures

`fixtures/mig/cover/taxo/`: hand-written taxonomies and report extracts, CC0-1.0; DMIS conformance extract generated from the `IO.DMIS.CONF` fixtures.

## Build prompt

> In crate `fid-mig`, add module `cover::taxo` with the `mig-taxonomy@1` types and a checker exposed as `cargo xtask mig taxonomy-check --dir <dir> [--dmis-conf <json>] [--reports <dir>] [--previous <dir>]`. Implement spec node MIG.COVER.TAXO: validate construct IDs (`^[a-z][a-z0-9_.]{1,63}$`, unique, description ≤ 200 chars), require a non-empty reviewer attestation (`TAXO-ATTEST`), fail `TAXO-INCOMPLETE <kw>` when a DMIS keyword from the conformance JSON is missing from the DMIS taxonomy, fail `TAXO-UNKNOWN` when a producer report names an unknown construct, fail `TAXO-BREAKING` when IDs were removed or renamed without a major version bump versus `--previous`. Author the initial taxonomies for `qif-project`, `dmis-output`, `csv-points` and `form-xlsx` in original wording (never copy incumbent documentation). Tests for AC-1..AC-5. Bounded parsing, no `unwrap`/`expect`/`panic`/`unsafe`, BTreeMap/BTreeSet only.

## Open questions

- The set of documented incumbent CSV exports that get their own taxonomy depends on `MIG.SRC` admissions [unverified]; start with the generic `csv-points` column-map format.

## Children

_Leaf: none._
