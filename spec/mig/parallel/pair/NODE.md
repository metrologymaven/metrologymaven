---
id: MIG.PARALLEL.PAIR
title: Result pairing by external ID and confirmed pairing table
level: 3
parent: MIG.PARALLEL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.PARALLEL-001, REQ-MIG.PARALLEL-006]
covers: []
depends_on: [IO.RES.TRANS, GDT.CHAR.IDENT, MIG.PROJ.XID, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# MIG.PARALLEL.PAIR: Result pairing by external ID and confirmed pairing table

## Purpose

Decides which incumbent result belongs to which of our ResultRecords, one-to-one, without guessing. The primary key is the incumbent characteristic identifier that `MIG.PROJ` kept as an external ID when it imported the same measurement; where identifiers do not line up, the user supplies a pairing table (`mig-pairing@1`) that a human confirmed. Exact name matches are offered only as unconfirmed suggestions in the output, never applied.

## Scope

**In:**
- Pairing by external ID: incumbent result row's characteristic identifier (from the transcription record) equals the external ID of one of our characteristics (via the `MIG.PROJ.XID` index) in the same incumbent namespace
- The `mig-pairing@1` table: rows of (incumbent identifier, our characteristic ID), with `confirmed_by` (human principal) and date; rows override external-ID pairs for the same items
- Conflict detection: any item paired twice
- Unpaired lists for both sides, and unconfirmed suggestions (exact case-insensitive name equality only)
- Read-only access to both sides

**Out:**
- Difference computation (owned by `MIG.PARALLEL.DIFF`)
- The report entity and commands (owned by `MIG.PARALLEL.REPORT`)
- The external-ID index (owned by `MIG.PROJ.XID`)

## Requirements

- **REQ-MIG.PARALLEL.PAIR-001** The pairer shall pair an incumbent result with our ResultRecord when the result's incumbent characteristic identifier equals the external ID of our characteristic in the same incumbent namespace.
- **REQ-MIG.PARALLEL.PAIR-002** When a pairing table is supplied, each table row shall pair its two items, replacing any external-ID pair that involves either item.
- **REQ-MIG.PARALLEL.PAIR-003** If any incumbent result or ResultRecord would be in more than one pair, then pairing shall fail with `pairing_conflict` naming the item and both partners.
- **REQ-MIG.PARALLEL.PAIR-004** If a pairing-table row lacks `confirmed_by` naming a human principal, then pairing shall fail with `pairing_unconfirmed` naming the row.
- **REQ-MIG.PARALLEL.PAIR-005** The output shall list every unpaired incumbent result and every unpaired ResultRecord of the run, each sorted by identifier, and may list name-equality suggestions marked `UNCONFIRMED`, which shall never be counted as pairs.
- **REQ-MIG.PARALLEL.PAIR-006** Pairing shall read both sides through immutable views and shall not create, modify or delete any ResultRecord or transcription record.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/parallel/pair/xid/` (12 incumbent results, 12 of our records, 10 matching external IDs), when pairing runs without a table, then there are 10 pairs and 2 unpaired items on each side, with 2 name suggestions marked `UNCONFIRMED`.
- **AC-2** Given the same fixture and `table.json` confirming the two remaining pairs by human `qa-1`, then there are 12 pairs and no unpaired items.
- **AC-3** (error path) Given `table-dup.json` pairing one incumbent result to two records, then pairing fails with `pairing_conflict`.
- **AC-4** (error path) Given `table-agent.json` with `confirmed_by: agent-1`, then pairing fails with `pairing_unconfirmed`.
- **AC-5** Given AC-1, then the digests of all ResultRecords and transcription records are unchanged after pairing.

## Design

- **Registry command(s):** none; library used by `correlate.run_parallel@1` (`MIG.PARALLEL.REPORT`).
- **Data structures:** `PairingTable { schema: "mig-pairing@1", rows: Vec<{ incumbent_id, char_id, confirmed_by, at }> }`; `Pairing { pairs: Vec<(IncumbentRowRef, ResultRecordRef, PairBasis: ExternalId | Table)>, unpaired_incumbent, unpaired_ours, suggestions }`.
- **Algorithm:** build BTreeMap from incumbent identifier to rows; look up our characteristics through the XID index; apply table rows (confirmed-by must resolve to a human principal in the project principal register); detect conflicts; sort pairs by our characteristic ID.
- **Errors:** `pairing_conflict`, `pairing_unconfirmed`, `pairing_table_schema`, `pairing_unknown_item`.
- **Determinism:** D0.
- **Performance budget:** 10,000 results paired in under 100 ms [unverified until measured].
- **Crate:** `fid-mig` (module `parallel::pair`).

## Test fixtures

`fixtures/mig/parallel/pair/`: synthetic transcription records and ResultRecords with generated identifiers, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `parallel::pair` implementing spec node MIG.PARALLEL.PAIR. Pair incumbent transcribed results with our ResultRecords one-to-one: first by equality of the incumbent characteristic identifier with our characteristic's external ID (lookup through the MIG.PROJ.XID index, same incumbent namespace), then apply an optional `mig-pairing@1` table whose rows replace external-ID pairs for the same items; every table row must have `confirmed_by` resolving to a human principal (`pairing_unconfirmed`); any item in two pairs fails `pairing_conflict`. Output pairs sorted by our characteristic ID, unpaired lists for both sides, and exact case-insensitive name-equality suggestions marked `UNCONFIRMED` that are never applied. Read both sides through immutable views only. Tests for AC-1..AC-5. No fuzzy matching; no `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- None.

## Children

_Leaf: none._
