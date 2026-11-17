---
id: RPT.STATS.SPC.FREEZE
title: Frozen control limits
level: 4
parent: RPT.STATS.SPC
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.SPC-003, REQ-RPT.STATS.SPC-004]
covers: [GOM.TRD.05]
depends_on: [RPT.STATS.SPC.LIM, RPT.STATS.DSET.MAT, CORE.PRJ.CONT]
side_effect: [W2, W2]
determinism: D0
cli: [fid stats freeze-limits, fid stats unfreeze-limits]
mcp: [stats_freeze_limits, stats_unfreeze_limits]
journal: [stats.freeze_limits@1, stats.unfreeze_limits@1]
updated: 2026-11-17
---

# RPT.STATS.SPC.FREEZE: Frozen control limits

## Purpose

Lets a user fix the control limits of a characteristic and chart type at a moment in time (typically after a capable baseline run), so that later points are judged against those limits and not against limits recomputed with the new points included. A frozen-limits record keeps the values together with the data-set digest, algorithm ID and constants table they came from. This leaf also owns the lookup that tells chart builders which limits are active.

## Scope

**In:**
- `stats.freeze_limits@1` and `stats.unfreeze_limits@1` (W2)
- The `FrozenLimits` project entity
- `active_limits(char, chart)` lookup

**Out:**
- Computing limits (`RPT.STATS.SPC.LIM`)
- Plotting points against active limits and flagging rules (`RPT.STATS.SPC.CHART`)

## Requirements

- **REQ-RPT.STATS.SPC.FREEZE-001** When `stats.freeze_limits@1` runs with a data set, characteristic, chart type and subgrouping, the system shall compute the limits with `RPT.STATS.SPC.LIM` and store them as a `FrozenLimits` record holding the values, unit, subgroup size, data-set digest, algorithm ID, constants table reference and principal, as a journaled W2 command.
- **REQ-RPT.STATS.SPC.FREEZE-002** If the request carries an expected limits digest (from an earlier `stats.compute_limits@1` result) and the recomputed limits have a different digest, then the command shall fail with `stats.limits_changed` and nothing shall be journaled.
- **REQ-RPT.STATS.SPC.FREEZE-003** If the computed limits are `INSUFFICIENT_DATA`, then the freeze shall fail with `stats.insufficient_data` and nothing shall be journaled.
- **REQ-RPT.STATS.SPC.FREEZE-004** At most one frozen-limits record shall be active per (characteristic, chart type); freezing again shall supersede the previous record, which stays in the journal.
- **REQ-RPT.STATS.SPC.FREEZE-005** When `stats.unfreeze_limits@1` runs for a characteristic and chart type with active frozen limits, the system shall deactivate them as a journaled W2 command; otherwise it shall fail with `stats.not_frozen`.
- **REQ-RPT.STATS.SPC.FREEZE-006** `active_limits(char, chart)` shall return the active `FrozenLimits` record or `NONE`, and shall never recompute limits.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/spc_xbar_r_25x5`, when `fid stats freeze-limits --dataset <id> --char C1 --chart xbar-r --subgroup fixed:5 --json` runs, then one `stats.freeze_limits@1` event is journaled and `active_limits(C1, XBAR_R)` returns values bit-identical to `compute-limits` on the same inputs, with the same data-set digest.
- **AC-2** (expected digest) Given an expected digest from a compute run on a data set that has since gained a record, when the freeze runs with `--expect <digest>`, then it fails with `stats.limits_changed` and journals nothing.
- **AC-3** (supersede) Given AC-1's state, when limits are frozen again from a different data set, then `active_limits` returns the new record and the old record is still readable from the journal.
- **AC-4** (error path) Given 10 subgroups only, when the freeze runs, then it fails with `stats.insufficient_data`.
- **AC-5** (replay) Given the journal from AC-1 and AC-3, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:**
  - `stats.freeze_limits@1(dataset: DataSetId, char: CharId, chart: ChartType, subgrouping: SubgroupMode, expect: Option<Digest>) -> FrozenLimitsRef`, W2 (agents write to an agent branch).
  - `stats.unfreeze_limits@1(char: CharId, chart: ChartType) -> ()`, W2.
- **Data structures:** `FrozenLimits { id: UUIDv7, char, chart, limits: ControlLimits, frozen_by: PrincipalId, active: bool, supersedes: Option<Id> }`, stored as project element type `stats.frozen_limits`.
- **Algorithm:** recompute through the LIM kernel; compare the canonical digest of the result with `expect`; write the record and flip `active` on the superseded one in the same command.
- **Errors:** `stats.limits_changed`, `stats.insufficient_data`, `stats.not_frozen`, `stats.dataset_not_found`.
- **Determinism:** D0 (stores kernel results; no new arithmetic).
- **Performance budget:** freeze under 50 ms excluding materialisation [unverified until measured].
- **Crate:** `fid-stats-cmd`.

## Test fixtures

- `fixtures/rpt/stats/spc_xbar_r_25x5` (shared with `RPT.STATS.SPC.LIM`), plus a variant with one extra record for AC-2.

## Build prompt

> In crate `fid-stats-cmd` (layer L2, RPT), implement frozen control limits.
>
> **Commands:** `stats.freeze_limits@1(dataset, char, chart, subgrouping, expect: Option<Digest>) -> FrozenLimitsRef` (W2): compute limits with the SPC.LIM kernel (`subgroup`, `subgroup_stats`, `limits` in `fid-stats`); fail with `stats.limits_changed` if `expect` differs from the result digest, with `stats.insufficient_data` if limits are `INSUFFICIENT_DATA`; store `FrozenLimits { id, char, chart, limits, frozen_by, active: true, supersedes }` as project element type `stats.frozen_limits`, deactivating any previous active record for (char, chart) in the same command. `stats.unfreeze_limits@1(char, chart)` (W2) deactivates, or fails with `stats.not_frozen`. CLI `fid stats freeze-limits`, `fid stats unfreeze-limits`; MCP `stats_freeze_limits`, `stats_unfreeze_limits`; `--json`.
>
> **Lookup:** `active_limits(char, chart) -> Option<FrozenLimits>`; never recomputes.
>
> **Tests:** AC-1 to AC-5 above with `fixtures/rpt/stats/spc_xbar_r_25x5`.
>
> **Constraints:** CONSTITUTION §8; C1; nothing journaled on error. Spec: `spec/rpt/stats/spc/freeze/NODE.md`.

## Open questions

- Frozen limits live in the project. Limits shared across projects through the catalog (for example per part number) are not covered and would need `RPT.CATALOG`.
- Whether freezing should be `human_only` under the Regulated profile (it sets the basis for judging later parts) needs a maintainer view; agent-authored freezes already land on an agent branch.

## Children

_Leaf: none._
