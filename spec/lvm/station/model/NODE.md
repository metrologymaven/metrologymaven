---
id: LVM.STATION.MODEL
title: Network document model and stations
level: 3
parent: LVM.STATION
kind: feature
status: leaf
phase: P3
size: M
owners: []
claimed_by:
satisfies: [REQ-LVM.STATION-001]
covers: []
depends_on: [CORE.PRJ, CORE.JRN, GEO.FRAME]
side_effect: W2
determinism: D0
cli: [fid net create-network, fid net add-station, fid net list-stations, fid net list-common-points]
mcp: [net_create_network, net_add_station, net_list_stations, net_list_common_points]
journal: [net.create_network@1, net.add_station@1]
updated: 2026-11-16
---

# LVM.STATION.MODEL: Network document model and stations

## Purpose

Defines the network document object that every LVM node reads: the network itself, its stations and its common points, plus the invariants that hold between them. It owns the commands that create a network and add stations, the station's pose slots (initial and adjusted), and the name-resolution rule that turns a point name into a common-point ID. Observation payloads, matching commands and imports are built on top of this model by sibling nodes.

## Scope

**In:**
- `Network` object inside the active project, with a stable ID, label and a derived `simulated` flag
- `Station` entity: stable ID, unique label, instrument reference, optional UNC instrument model reference, `simulated` flag, initial-pose slot and adjusted-pose slot
- `CommonPoint` entity: stable ID, canonical name, alias names, enabled flag
- Name resolution: look up a common point by normalised name or alias, or create it
- Invariant checks run on every load and every commit: referential integrity, unique labels and names
- Read queries listing stations and common points

**Out** (and which node owns it instead):
- Observation payload types, provenance and enable flags (`LVM.STATION.OBS`)
- User commands that merge, rename or exclude common points (`LVM.STATION.MATCH`)
- Imports and live capture (`LVM.STATION.IMPORT`, `LVM.STATION.CAPTURE`)
- Writing the initial pose (`LVM.MOVE.CHECK`) and the adjusted pose (`LVM.ADJ.SOLVE`)
- Frame and transform types (`GEO.FRAME`)

## Requirements

- **REQ-LVM.STATION.MODEL-001** When `net.create_network@1` runs, the system shall create a network object in the active project with a UUIDv7 ID and a user label, and journal one event.
- **REQ-LVM.STATION.MODEL-002** When `net.add_station@1` runs, the system shall create a station with a UUIDv7 ID, a label unique within the network, an instrument reference (device class, make, model and serial as untrusted text), an optional UNC instrument model ID with version, an empty initial-pose slot and an empty adjusted-pose slot.
- **REQ-LVM.STATION.MODEL-003** If a station label or common-point canonical name already exists in the network, then the command shall fail with `NET_DUPLICATE_LABEL` and journal nothing.
- **REQ-LVM.STATION.MODEL-004** The model shall reject any write to a station's adjusted-pose slot that does not come from an adjustment-result commit, and any write to the initial-pose slot that does not come from a move-acceptance or adjustment commit, with `NET_POSE_SLOT_READONLY`.
- **REQ-LVM.STATION.MODEL-005** When a point name is resolved, the model shall normalise it (Unicode NFC, leading and trailing whitespace trimmed, case preserved) and return the common point whose canonical name or alias equals it, or create a new common point with a UUIDv7 ID if none exists.
- **REQ-LVM.STATION.MODEL-006** When a network is loaded or a command commits, the model shall verify that every observation references an existing station and common point; if not, then it shall fail closed with `NET_DANGLING_REFERENCE` naming the first offending observation ID.
- **REQ-LVM.STATION.MODEL-007** The network's `simulated` flag shall be true if and only if at least one station's `simulated` flag is true.
- **REQ-LVM.STATION.MODEL-008** When `net.list_stations` or `net.list_common_points` runs, the system shall return entities in creation order with their IDs, labels or names, flags and observation counts.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/lvm/net-small-3st` (3 stations, 12 common points, built by commands), when `fid net list-stations --json` runs, then it returns 3 stations in creation order with the expected labels, instrument references and empty adjusted-pose slots.
- **AC-2** (error path) Given the same fixture, when `fid net add-station --label ST2` runs and `ST2` already exists, then the command fails with `NET_DUPLICATE_LABEL` and the journal length is unchanged.
- **AC-3** (name resolution) Given point names `"P1"`, `" P1 "` and the NFD form of `"Pé"`, when they are resolved in a fresh network, then `"P1"` and `" P1 "` resolve to the same common point, and `"Pé"` in NFC and NFD forms resolve to one point; `"p1"` resolves to a different point.
- **AC-4** (read-only slot) Given a station, when any command other than an adjustment or move-acceptance commit tries to set its adjusted pose, then it fails with `NET_POSE_SLOT_READONLY`.
- **AC-5** (integrity) Given fixture `fixtures/lvm/net-dangling` (a project file whose observation references a removed station), when it is loaded, then loading fails with `NET_DANGLING_REFERENCE` naming that observation.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:**
  - `net.create_network@1` (W2): `label: String` → `NetworkId`.
  - `net.add_station@1` (W2): `network: NetworkId, label: String, instrument: InstrumentRef, unc_model: Option<ModelRef>` → `StationId`.
  - `net.list_stations@1` (R0), `net.list_common_points@1` (R0): `network: NetworkId` → list.
- **Data structures:** `Network { id, label, stations: Vec<Station>, points: Vec<CommonPoint>, observations: Vec<Observation> }` with `Vec` in creation order (no std HashMap, per KNP-1 style ordering); `Station { id, label, instrument: InstrumentRef, unc_model: Option<ModelRef>, simulated: bool, initial_pose: Option<PoseRecord>, adjusted_pose: Option<PoseRecord> }`; `PoseRecord { frame: GEO frame transform, source_event: JournalHash }`; `CommonPoint { id, name, aliases: Vec<String>, enabled: bool }`. `Observation` is an opaque envelope here whose payload is defined by `LVM.STATION.OBS`.
- **Pose slot guard:** slot setters take a capability token that only the adjustment-commit and move-acceptance command handlers can construct.
- **Errors:** `NET_DUPLICATE_LABEL`, `NET_POSE_SLOT_READONLY`, `NET_DANGLING_REFERENCE`, `NET_UNKNOWN_NETWORK`.
- **Determinism:** D0. Pure data manipulation; IDs come from the journaled UUIDv7 source, so replay reproduces them.
- **Performance budget:** name resolution in under 1 µs per lookup for 10 000 points using a sorted index (BTreeMap) [unverified until measured].
- **Crate:** `fid-lvm` (proposed name).

## Test fixtures

- `fixtures/lvm/net-small-3st`: generated by a seeded synthetic network script (seed 1) that issues the commands; project-authored, CC-BY-4.0.
- `fixtures/lvm/net-dangling`: hand-edited copy of the above with one station removed from the file; CC-BY-4.0.

## Build prompt

> In the implementation repository, create the crate `fid-lvm` (if it does not exist) and implement the large-volume network document model. Add types `Network`, `Station`, `CommonPoint`, `InstrumentRef`, `ModelRef` and `PoseRecord`; keep collections as `Vec` in creation order and use `BTreeMap` for indexes (no std HashMap or HashSet). Register four commands in the command registry: `net.create_network@1` (W2), `net.add_station@1` (W2), `net.list_stations@1` (R0) and `net.list_common_points@1` (R0); CLI `fid net create-network|add-station|list-stations|list-common-points` with `--json`; MCP names are generated (`net_create_network` and so on). Requirements: IDs are UUIDv7 from the journaled ID source; station labels and point names are unique per network (`NET_DUPLICATE_LABEL`, journal nothing); point-name resolution normalises to Unicode NFC and trims whitespace but keeps case, matching canonical names and aliases, else creates a point; pose slots are writable only through a capability token held by the adjustment-commit and move-acceptance handlers (`NET_POSE_SLOT_READONLY`); integrity check on load and commit (`NET_DANGLING_REFERENCE` with the observation ID); `network.simulated` is the OR of station flags. Instrument strings are untrusted text (CONSTITUTION C6). No panics, no `unwrap`/`expect`, no `unsafe` (CONSTITUTION §8). Tests: the six acceptance criteria above, using a seeded fixture generator in `tests/fixtures/` and an exact-mode replay test. Determinism class D0.

## Open questions

- Should a station also be removable, or only disabled? This leaf offers neither; removal can be added later as a journaled command that keeps observations recoverable.

## Children

_Leaf: none._
