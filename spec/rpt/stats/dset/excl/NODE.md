---
id: RPT.STATS.DSET.EXCL
title: Automatic and user exclusions
level: 4
parent: RPT.STATS.DSET
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.DSET-004, REQ-RPT.STATS.DSET-005]
covers: []
depends_on: [RPT.STATS.DSET.DEF, RPT.CATALOG.CHECK, RPT.BALLOON.ENTRY, GDT.CHAR.RECORD]
side_effect: [W2, W2]
determinism: D0
cli: [fid stats exclude-dataset, fid stats reinclude-dataset]
mcp: [stats_exclude_dataset, stats_reinclude_dataset]
journal: [stats.exclude_dataset@1, stats.reinclude_dataset@1]
updated: 2026-11-17
---

# RPT.STATS.DSET.EXCL: Automatic and user exclusions

## Purpose

Owns the closed list of exclusion reasons and the rule that decides, for each candidate record, whether it is left out of a data set automatically. It also owns the two commands by which a user takes a record out of a data set, or puts it back, with a reason. The record itself never changes; the exclusion lives on the data-set definition so that one record can be in one analysis and out of another.

## Scope

**In:**
- `ExclusionReason` codes and their fixed precedence
- The pure predicate `auto_exclusion(candidate) -> Option<ExclusionReason>`
- `stats.exclude_dataset@1` and `stats.reinclude_dataset@1` (W2)

**Out:**
- The definition type and its storage (`RPT.STATS.DSET.DEF`)
- Applying the predicate during materialisation (`RPT.STATS.DSET.MAT`)
- Statistic-level exclusions such as incomplete subgroups (`RPT.STATS.SPC.SUB`)
- Marking catalog entries `STALE` (`RPT.CATALOG.CHECK`) and voiding readings (`RPT.BALLOON.ENTRY`)

## Requirements

- **REQ-RPT.STATS.DSET.EXCL-001** The system shall define the exclusion reasons `NOT_DECIDABLE`, `VOIDED`, `STALE`, `UNAVAILABLE`, `NO_VALUE`, `UNIT_MISMATCH`, `MISSING_ORDER_KEY` and `USER`, and no other.
- **REQ-RPT.STATS.DSET.EXCL-002** When a candidate record has decision outcome `NOT_DECIDABLE`, is a voided reading, comes from a catalog entry with status `STALE` or `UNAVAILABLE`, has no numeric value, has a unit different from the characteristic's unit, or lacks the value of the chosen ordering key, the predicate shall return the matching reason.
- **REQ-RPT.STATS.DSET.EXCL-003** If a candidate matches more than one automatic reason, then the predicate shall return only the first in the order `UNAVAILABLE`, `STALE`, `VOIDED`, `NO_VALUE`, `UNIT_MISMATCH`, `NOT_DECIDABLE`, `MISSING_ORDER_KEY`.
- **REQ-RPT.STATS.DSET.EXCL-004** When `stats.exclude_dataset@1` runs with a data-set ID, a record ID and a non-empty reason text, the system shall add a `USER` exclusion with that text and the principal to the definition as a journaled W2 command, and shall not change the record.
- **REQ-RPT.STATS.DSET.EXCL-005** If the reason text is empty, only whitespace or longer than 500 characters, then `stats.exclude_dataset@1` shall fail with `stats.exclusion_reason_required` and nothing shall be journaled.
- **REQ-RPT.STATS.DSET.EXCL-006** When `stats.reinclude_dataset@1` runs for a record with a `USER` exclusion, the system shall remove that exclusion as a journaled W2 command; if the record has no `USER` exclusion, the command shall fail with `stats.not_excluded`.
- **REQ-RPT.STATS.DSET.EXCL-007** A user shall not be able to reinclude a record that is excluded for an automatic reason; such a request shall fail with `stats.auto_excluded` naming the reason.

## Acceptance criteria

- **AC-1** Given a table fixture `fixtures/rpt/stats/exclusion_cases.json` (one candidate per automatic reason and three candidates matching two reasons each), when the predicate runs on each, then every result equals the expected reason in the file, including the precedence cases.
- **AC-2** Given fixture `fixtures/rpt/stats/three_part_run` and a saved definition, when `fid stats exclude-dataset --dataset <id> --record <rid> --reason "fixture moved" --json` runs, then the definition lists the `USER` exclusion with that text, one event is journaled, and the record's journal entry hash is unchanged.
- **AC-3** Given AC-2's state, when `fid stats reinclude-dataset --dataset <id> --record <rid>` runs, then the exclusion is gone and the definition digest equals the digest before AC-2.
- **AC-4** (error path) Given an empty reason, when the exclude command runs, then it fails with `stats.exclusion_reason_required` and journals nothing.
- **AC-5** (error path) Given a record excluded as `VOIDED`, when the reinclude command runs, then it fails with `stats.auto_excluded` naming `VOIDED`.
- **AC-6** (replay) Given the journal from AC-2 and AC-3, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:**
  - `stats.exclude_dataset@1(dataset: DataSetId, record: RecordId, reason: UntrustedText) -> DataSetRef`, W2.
  - `stats.reinclude_dataset@1(dataset: DataSetId, record: RecordId) -> DataSetRef`, W2.
- **Data structures:** `enum ExclusionReason { Unavailable, Stale, Voided, NoValue, UnitMismatch, NotDecidable, MissingOrderKey, User }` serialised in SCREAMING_SNAKE_CASE; `Candidate { record: RecordId, decision: Option<Outcome>, voided: bool, catalog_status: Option<CatalogStatus>, value: Option<f64>, unit: Unit, char_unit: Unit, order_key: Option<OrderValue> }`.
- **Algorithm:** check reasons in the fixed precedence order of REQ-003 and return the first that matches. Unit equality is exact unit identity (no conversion; see Open questions). Voided status comes from `RPT.BALLOON.ENTRY`; catalog status from `RPT.CATALOG.CHECK`; decision from the ResultRecord (`GDT.CHAR.RECORD`).
- **Errors:** `stats.exclusion_reason_required`, `stats.not_excluded`, `stats.auto_excluded{reason}`, `stats.dataset_not_found`, `stats.record_not_in_dataset`.
- **Determinism:** D0; the predicate is pure and has no arithmetic.
- **Performance budget:** predicate under 1 µs per candidate [unverified until measured].
- **Crate:** `fid-stats-cmd`.

## Test fixtures

- `fixtures/rpt/stats/exclusion_cases.json`: hand-written table of candidates and expected reasons, CC-BY-4.0.
- `fixtures/rpt/stats/three_part_run`: shared synthetic project (see `RPT.STATS.DSET.DEF`).

## Build prompt

> In crate `fid-stats-cmd` (layer L2, RPT), implement data-set exclusions.
>
> **Reasons:** `ExclusionReason = UNAVAILABLE | STALE | VOIDED | NO_VALUE | UNIT_MISMATCH | NOT_DECIDABLE | MISSING_ORDER_KEY | USER` (closed set). Write a pure function `auto_exclusion(c: &Candidate) -> Option<ExclusionReason>` that checks the automatic reasons in exactly that order and returns the first match: catalog status `UNAVAILABLE`, catalog status `STALE`, voided reading, no numeric value, unit not identical to the characteristic's unit, decision `NOT_DECIDABLE`, missing ordering-key value.
>
> **Commands:** `stats.exclude_dataset@1(dataset, record, reason)` (W2) adds `UserExclusion { record, reason, by: principal }` to the stored `DataSetDef` (from module DSET.DEF); reason is untrusted text, non-empty after trimming, at most 500 characters, else `stats.exclusion_reason_required`. `stats.reinclude_dataset@1(dataset, record)` (W2) removes a `USER` exclusion; fails with `stats.not_excluded` if there is none and with `stats.auto_excluded{reason}` if the record is excluded automatically. Never modify the ResultRecord. CLI `fid stats exclude-dataset`, `fid stats reinclude-dataset`; MCP `stats_exclude_dataset`, `stats_reinclude_dataset`; `--json`.
>
> **Tests:** AC-1 to AC-6 above with fixtures `fixtures/rpt/stats/exclusion_cases.json` and `fixtures/rpt/stats/three_part_run`.
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`); C6 (reason text is data). Spec: `spec/rpt/stats/dset/excl/NODE.md`.

## Open questions

- Values recorded in another unit of the same quantity kind are excluded as `UNIT_MISMATCH` rather than converted. Converting would be a journaled rounding (CONSTITUTION §4); revisit when a user needs mixed-unit catalogs.
- Whether excluding a record should be `human_only` under the Regulated profile is not settled; it is W2 on an agent branch for now.

## Children

_Leaf: none._
