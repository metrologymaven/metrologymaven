---
id: IND.PATTERN.SUM
title: Pattern result summary
level: 3
parent: IND.PATTERN
kind: feature
status: leaf
phase: P3
size: S
owners: []
claimed_by:
satisfies: [REQ-IND.PATTERN-006]
covers: [GOM.SPE.06]
depends_on: [IND.PATTERN.INST]
side_effect: R1
determinism: D0
cli: [fid pack summarize-pattern]
mcp: [pack_summarize_pattern]
journal: [pack.summarize_pattern@1]
updated: 2026-11-16
---

# IND.PATTERN.SUM: Pattern result summary

## Purpose

Summarises the results of an instantiated pattern: how many members conform, do not conform, are indeterminate, are not decidable or have no result, which member is worst, and a per-member table. It reads members' ResultRecords and never changes them.

## Scope

**In:**
- Command `pack.summarize_pattern@1` (R1)
- Counting member outcomes, ranking members, picking the worst member
- Tolerance utilisation for ranking within an outcome

**Out:**
- Instantiation and member lists (owned by `IND.PATTERN.INST`)
- Evaluating member characteristics and their decisions (owned by core GDT and the element's pack)
- Presenting the summary in reports (owned by `IND.PATTERN.TPL`)

## Requirements

- **REQ-IND.PATTERN.SUM-001** When `pack.summarize_pattern@1` runs on a pattern, the pack shall report the member count and the number of members whose worst characteristic outcome is `CONFORMS`, `NONCONFORMS`, `INDETERMINATE` and `NOT_DECIDABLE`, plus the number of members with no result (`NO_RESULT`).
- **REQ-IND.PATTERN.SUM-002** A member's outcome shall be the most severe outcome over its characteristics in the order `NONCONFORMS` > `INDETERMINATE` > `NOT_DECIDABLE` > `CONFORMS` (method `pack.pattern.summary.worst@1.0.0`).
- **REQ-IND.PATTERN.SUM-003** Tolerance utilisation of a characteristic shall be `|v - (lo + hi)/2| / ((hi - lo)/2)` for a two-sided tolerance and `v / hi` for an upper-only tolerance with `hi > 0`; for any other tolerance form it shall be undefined.
- **REQ-IND.PATTERN.SUM-004** The worst member shall be the member with the most severe outcome, then the highest defined utilisation (defined ranks above undefined), then the lowest member index; the summary shall name its member index, element ID, characteristic ID, outcome and utilisation.
- **REQ-IND.PATTERN.SUM-005** The pack shall report a per-member table in member-index order with each member's outcome, worst characteristic and utilisation.
- **REQ-IND.PATTERN.SUM-006** The command shall not create, modify or delete any ResultRecord, element or characteristic.
- **REQ-IND.PATTERN.SUM-007** If the pattern ID is unknown or has no members, then the command shall fail with `E_PATTERN_UNKNOWN` or `E_PATTERN_EMPTY` and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/pack/pattern/sum-12` (a 12-member hole pattern with diameter tolerance +/-1e-4 m; members 4 and 9 nonconform with utilisation 1.2 and 1.5, member 7 is `NOT_DECIDABLE`, member 11 is `INDETERMINATE`, member 12 has no result, all others conform), when `fid pack summarize-pattern P1 --json` runs, then counts are CONFORMS 7, NONCONFORMS 2, INDETERMINATE 1, NOT_DECIDABLE 1, NO_RESULT 1, and the worst member is 9 with utilisation 1.5 within 1e-12.
- **AC-2** (tie) Given `sum-12-tie` (members 4 and 9 both nonconform with utilisation exactly 1.5), when the command runs, then the worst member is 4.
- **AC-3** (multi-characteristic) Given `sum-multi` (each member has diameter and position; member 2's diameter conforms and position nonconforms), when the command runs, then member 2 counts as `NONCONFORMS` and its worst characteristic is position.
- **AC-4** (undefined utilisation) Given a nonconforming member whose characteristic has a lower-only tolerance and another nonconforming member with utilisation 1.1, when the command runs, then the member with utilisation 1.1 is worst.
- **AC-5** (no side effects) Given `sum-12`, when the command runs, then the project document digest is unchanged.
- **AC-6** (errors) Given an unknown pattern ID or a pattern with no members, when the command runs, then it fails with `E_PATTERN_UNKNOWN` or `E_PATTERN_EMPTY` and journals nothing.
- **AC-7** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `pack.summarize_pattern@1`, class R1. Parameters: `pattern: PatternRef`. Result: `PatternSummary { pattern_id, member_count, counts: OutcomeCounts, worst: Option<WorstMember>, members: Vec<MemberRow> }`; `MemberRow { index, element_id, outcome: Outcome | NoResult, worst_characteristic: Option<CharId>, utilisation: Option<f64> }`.
- **Algorithm:** read the pattern's member list (from `IND.PATTERN.INST`'s stored pattern) in member-index order; for each member read the latest ResultRecord of each characteristic; compute utilisation per REQ-003 (NaN or infinite values are errors); member outcome per REQ-002, worst characteristic by (severity, utilisation, characteristic order); count; pick the worst member by the REQ-004 key with a single fixed-order pass.
- **Errors:** `E_PATTERN_UNKNOWN`, `E_PATTERN_EMPTY`, `E_NAN`.
- **Determinism:** D0: only comparisons, subtraction and division in a fixed order.
- **Performance budget:** 10 000 members with 5 characteristics each in under 50 ms [unverified until measured].
- **Crate:** `fid-pack-pattern`.

## Test fixtures

- `fixtures/pack/pattern/sum-12`, `sum-12-tie`, `sum-multi`: small synthetic projects built by a test helper with hand-set ResultRecords (no geometry evaluation needed). Licence: project.

## Build prompt

> In the Rust crate `fid-pack-pattern` (feature-pattern industry pack), implement registry command `pack.summarize_pattern@1` (side-effect class R1; CLI `fid pack summarize-pattern <pattern> [--json]`; MCP tool `pack_summarize_pattern`; journal event `pack.summarize_pattern@1` with result digest), registered only through the pack manifest.
>
> Read the pattern's members in member-index order and the latest ResultRecord of each member characteristic; never write anything. Utilisation: two-sided `[lo, hi]` gives `|v - (lo+hi)/2| / ((hi-lo)/2)`; upper-only with `hi > 0` gives `v/hi`; otherwise undefined. Member outcome = most severe of its characteristics in the order NONCONFORMS > INDETERMINATE > NOT_DECIDABLE > CONFORMS; members without results are `NO_RESULT`. Worst member key: severity, then defined utilisation (higher first; defined beats undefined), then lowest member index. Return `PatternSummary { pattern_id, member_count, counts {conforms, nonconforms, indeterminate, not_decidable, no_result}, worst {member_index, element_id, characteristic_id, outcome, utilisation}, members: [{index, element_id, outcome, worst_characteristic, utilisation}] }`. Errors: unknown pattern `E_PATTERN_UNKNOWN`, empty `E_PATTERN_EMPTY`, non-finite value `E_NAN`; on error journal nothing. Method ID for the ranking: `pack.pattern.summary.worst@1.0.0`.
>
> Tests: `sum-12` gives counts 7/2/1/1/1 and worst member 9 (utilisation 1.5); tie picks the lower index; a member with one nonconforming characteristic of two counts as NONCONFORMS with that characteristic as worst; undefined utilisation ranks below defined; document digest unchanged; error cases; exact replay `IDENTICAL`.
>
> Constraints: KNP-1 (no FMA, fixed-order loops, no std HashMap or HashSet, total-order comparisons, NaN is an error); no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded input (at most 100 000 members).

## Open questions

- Whether `NOT_DECIDABLE` should rank above `INDETERMINATE` is a user preference [unverified]; a later method version could make the order configurable.

## Children

_Leaf: none._
