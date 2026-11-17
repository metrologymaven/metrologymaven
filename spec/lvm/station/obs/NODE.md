---
id: LVM.STATION.OBS
title: Observation types, provenance and enable flags
level: 3
parent: LVM.STATION
kind: feature
status: leaf
phase: P3
size: M
owners: []
claimed_by:
satisfies: [REQ-LVM.STATION-002, REQ-LVM.STATION-006, REQ-LVM.STATION-007]
covers: []
depends_on: [LVM.STATION.MODEL, KRN.NUM]
side_effect: W2
determinism: D0
cli: [fid net add-observation, fid net set-observation-enabled, fid net list-observations]
mcp: [net_add_observation, net_set_observation_enabled, net_list_observations]
journal: [net.add_observation@1, net.set_observation_enabled@1]
updated: 2026-11-16
---

# LVM.STATION.OBS: Observation types, provenance and enable flags

## Purpose

Defines how one instrument observation is stored: in the native form the instrument reported, with its station, common point, time, source digest and provenance. It validates each observation, propagates the `simulated` flag to the station, and owns the journaled enable/disable flag on observations so that excluded data stays in the document. Import and live capture both create observations only through this node's validated constructor.

## Scope

**In:**
- Observation payload types: polar (horizontal angle, zenith angle, distance), direction-only (horizontal angle, zenith angle), Cartesian with a 3×3 covariance
- Per-observation fields: ID, station ID, common-point ID, instrument timestamp, host timestamp, source digest, provenance, enabled flag, authored units
- Validation of a single observation record, returning a typed field error
- Setting the station's `simulated` flag from provenance
- Enable/disable command and observation listing

**Out** (and which node owns it instead):
- Record-index reporting and atomic multi-record import (`LVM.STATION.IMPORT`)
- Intake from live HAL streams (`LVM.STATION.CAPTURE`)
- Image-coordinate observations for image-level bundles (`LVM.ADJ.IMG`, extension of this type set)
- Weights and covariance derived from UNC models (`LVM.ADJ.WEIGHT`)

## Requirements

- **REQ-LVM.STATION.OBS-001** The system shall store each observation as exactly one of the payload types polar (horizontal angle in rad, zenith angle in rad, distance in m), direction-only (horizontal angle in rad, zenith angle in rad) or Cartesian (x, y, z in m with a 3×3 covariance in m²), in the station's own frame.
- **REQ-LVM.STATION.OBS-002** Each observation shall carry a UUIDv7 ID (or a UUIDv5 ID supplied by an importer), station ID, common-point ID, instrument timestamp, host timestamp, SHA-256 source digest, provenance (`live`, `transcript` or `file`, device identity, `simulated` bit) and an enabled flag.
- **REQ-LVM.STATION.OBS-003** When an observation is authored in other units (for example degrees, gon or millimetres), the system shall keep the authored value and unit and convert to SI with a single rounding that is journaled.
- **REQ-LVM.STATION.OBS-004** If a record has a non-finite value, a distance that is not greater than zero, a zenith angle outside [0, π], or a covariance that is not symmetric or not positive definite, then validation shall return `NET_OBS_INVALID` with the field name and the observation shall not be stored.
- **REQ-LVM.STATION.OBS-005** When an observation whose provenance has the `simulated` bit is stored, the system shall set its station's `simulated` flag, and that flag shall never be cleared, even if the observation is later disabled.
- **REQ-LVM.STATION.OBS-006** When `net.set_observation_enabled@1` runs, the system shall change only the enabled flag, keep the payload unchanged and journal one event, so that undo and replay restore the prior state.
- **REQ-LVM.STATION.OBS-007** When `net.list_observations` runs, the system shall return observations filtered by station, common point or enabled state, in creation order.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/lvm/net-small-3st`, when `fid net add-observation --station ST1 --point P1 --polar "hz=12.5deg v=88.0deg d=4.2105m" --json` runs, then the stored observation shows hz, v in rad and d in m, keeps the authored strings, and the journal event lists one rounding per converted value.
- **AC-2** (error path) Given a polar record with `d = 0`, when it is added, then the command fails with `NET_OBS_INVALID` naming field `distance` and the journal length is unchanged.
- **AC-3** (error path) Given a Cartesian record whose covariance has eigenvalue −1e-12, when it is added, then it fails with `NET_OBS_INVALID` naming field `covariance`.
- **AC-4** (simulated) Given a station with only live observations, when one observation with provenance `simulated = true` is added and then disabled, then the station and the network still report `simulated = true`.
- **AC-5** (enable flag) Given fixture `fixtures/lvm/net-small-3st`, when an observation is disabled, then `fid net list-observations --enabled false --json` lists it with an unchanged payload digest; after journal undo it is enabled again.
- **AC-6** (replay) Given the journal from AC-1 to AC-5, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:**
  - `net.add_observation@1` (W2): `network, station: StationId, point: String (name, resolved by LVM.STATION.MODEL), payload: ObsPayloadAuthored, instrument_time: Option<Timestamp>, provenance: Provenance` → `ObservationId`.
  - `net.set_observation_enabled@1` (W2): `observation: ObservationId, enabled: bool` → `()`.
  - `net.list_observations@1` (R0): filters → list.
- **Data structures:** `enum ObsPayload { Polar { hz, v, d }, Direction { hz, v }, Cartesian { p: [f64; 3], cov: [[f64; 3]; 3] } }` (non-exhaustive, so `LVM.ADJ.IMG` can add a variant later); `Provenance { source: Live|Transcript|File, device: DeviceRef, simulated: bool }`; `Authored { text, unit }` kept beside each converted field.
- **Conventions:** horizontal angle measured counter-clockwise from the station's +x axis about +z; zenith angle from +z. Importers convert vendor conventions to this one and record the conversion.
- **Validation:** finite check; d > 0; v in [0, π]; hz normalised to (−π, π] with the normalisation journaled; covariance symmetric within 1e-15 relative and Cholesky succeeds (positive definite).
- **Unit conversion:** through the shared units facility (`GEO.FRAME` units); angles via `fid-num` constants.
- **Errors:** `NET_OBS_INVALID { field }`, `NET_UNKNOWN_STATION`, `NET_UNKNOWN_OBSERVATION`.
- **Determinism:** D0. Only exact parsing, one multiplication per unit conversion and a Cholesky test; no FMA.
- **Crate:** `fid-lvm` (proposed name).

## Test fixtures

- `fixtures/lvm/net-small-3st` (shared with `LVM.STATION.MODEL`), seeded synthetic generator, CC-BY-4.0.
- Hand-written invalid records (zero distance, non-PD covariance, NaN angle) in `tests/fixtures/obs-invalid.json`, CC-BY-4.0.

## Build prompt

> In the crate `fid-lvm`, add observation storage for large-volume networks. Define `ObsPayload` (non-exhaustive enum: `Polar { hz, v, d }` in rad/rad/m, `Direction { hz, v }`, `Cartesian { p, cov }` in m and m²), `Provenance { source: Live|Transcript|File, device, simulated }` and the `Observation` struct with ID, station ID, common-point ID, instrument and host timestamps, SHA-256 source digest, provenance, enabled flag and the authored text and unit of each value. Provide one validated constructor used by every caller; it rejects non-finite values, d ≤ 0, zenith outside [0, π], and non-symmetric or non-positive-definite covariance with `NET_OBS_INVALID { field }`. Horizontal angles are normalised to (−π, π]. Unit conversion from degrees, gon, mm and similar is one journaled rounding per value (CONSTITUTION §4). Storing an observation with `simulated = true` sets the station flag permanently. Register `net.add_observation@1` (W2), `net.set_observation_enabled@1` (W2) and `net.list_observations@1` (R0) with CLI `fid net add-observation|set-observation-enabled|list-observations` and `--json`; MCP names are generated. Point names are resolved through the existing `LVM.STATION.MODEL` resolver. No panics, no `unwrap`, no `unsafe`, no FMA (CONSTITUTION §8, §4). Tests: the six acceptance criteria above, including an exact replay. Determinism class D0.

## Open questions

- Do some trackers report a per-observation covariance for polar measurements that should be stored? If so, add an optional covariance to polar and direction payloads in a later version.

## Children

_Leaf: none._
