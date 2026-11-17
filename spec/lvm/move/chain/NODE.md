---
id: LVM.MOVE.CHAIN
title: Move history and accumulated chain uncertainty
level: 3
parent: LVM.MOVE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.MOVE-005]
covers: []
depends_on: [LVM.MOVE.CHECK, LVM.MOVE.PROP]
cli: [fid net list-moves, fid net report-move-chain]
mcp: [net_list_moves, net_report_move_chain]
journal: []
updated: 2026-11-16
---

# LVM.MOVE.CHAIN: Move history and accumulated chain uncertainty

## Purpose

Large networks are often built by leapfrogging: each new station ties to the previous one. This node keeps the history of accepted moves as a chain (or tree) back to the reference station and reports how pose uncertainty accumulates along it, so the user can see when the chain needs to be closed or re-anchored.

## Scope

**In:**
- Move history from accepted moves: from-station, to-station, points used, check report reference
- The move graph from the reference station to every station
- Accumulated pose covariance along each path, with correlation through shared stations
- Report of accumulated uncertainty per station

**Out** (and which node owns it instead):
- Accepting moves (`LVM.MOVE.CHECK`)
- Per-move pose covariance (`LVM.MOVE.PROP`)
- Closing the chain by full network adjustment (`LVM.ADJ`)

## Requirements

- **REQ-LVM.MOVE.CHAIN-001** When `net.list_moves@1` runs, the system shall list every accepted move in acceptance order with its from-station, to-station, point count and check-report reference.
- **REQ-LVM.MOVE.CHAIN-002** When `net.report_move_chain@1` runs for a station, the system shall return the path of moves from the reference station and the accumulated pose covariance along it.
- **REQ-LVM.MOVE.CHAIN-003** If any move on the path has no pose covariance (UEC-0), then the accumulated uncertainty for that station shall also be UEC-0.
- **REQ-LVM.MOVE.CHAIN-004** If a station is reachable by more than one path, the report shall state each path and shall not combine them, since combining is the job of the network adjustment.

## Interfaces

- **Offers:** `net.list_moves@1` (R0), `net.report_move_chain@1` (R1); chain data for `LVM.RPT`.
- **Consumes:** `LVM.MOVE.CHECK` acceptance records; `LVM.MOVE.PROP` pose covariance.

## Decomposition hints

HIST (move history and graph), ACCUM (covariance accumulation).

## Open questions

- None yet.

## Children

_None yet: status `open`._
