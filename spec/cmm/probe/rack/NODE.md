---
id: CMM.PROBE.RACK
title: Probe changer racks and port assignment
level: 3
parent: CMM.PROBE
kind: feature
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-CMM.PROBE-004]
covers: [PCD.PRB.04]
depends_on: [CMM.PROBE.MODEL]
side_effect: W2
determinism: D0
cli: [fid cmm define-rack, fid cmm assign-rack-port]
mcp: [cmm_define_rack, cmm_assign_rack_port]
journal: [cmm.define_rack@1, cmm.assign_rack_port@1]
updated: 2026-11-16
---

# CMM.PROBE.RACK: Probe changer racks and port assignment

## Purpose

Models probe changer racks as configuration: the rack pose in the machine frame, its ports, each port's pose and pick and drop approach data, and which probe configuration is parked in which port. Path planning uses it to generate tip-change moves, collision checking uses it as an obstacle, and validators use it to confirm rack positions are known.

## Scope

**In:**
- `RackModel` entity (ID, version, digest), rack pose, ports with pose, approach vector, approach distance and clearance distance
- Assigning a probe configuration to a port and releasing it
- Port conflict detection
- `cmm.define_rack@1` and `cmm.assign_rack_port@1` (both W2)

**Out** (and which node owns it instead):
- Tip-change moves in a plan (owned by `CMM.PATH.PLAN`)
- Rack geometry as a collision body (owned by `CMM.COLL`, which reads the rack pose and a rack mesh reference)
- Tool-change protocol calls (owned by `HAL`)

## Requirements

- **REQ-CMM.PROBE.RACK-001** The system shall model a rack with a pose in the machine frame and one to 32 ports, each port with a pose relative to the rack, a unit approach vector, an approach distance and a clearance distance in SI metres.
- **REQ-CMM.PROBE.RACK-002** If a port pose, approach distance or clearance distance is non-finite, or a distance is non-positive, then the system shall reject the rack with `RACK_PORT_INVALID` naming the port.
- **REQ-CMM.PROBE.RACK-003** When a probe configuration is assigned to a port that already holds a different configuration, the system shall reject the assignment with `RACK_PORT_CONFLICT` naming the port and both configuration IDs, and shall journal nothing.
- **REQ-CMM.PROBE.RACK-004** If one probe configuration is assigned to two ports of the same machine's racks, then the system shall reject the second assignment with `RACK_DUPLICATE_ASSIGNMENT`.
- **REQ-CMM.PROBE.RACK-005** The system shall compute the rack digest as SHA-256 over RFC 8785 canonical JSON of rack pose, ports and assignments (by probe configuration digest), so any port move or reassignment changes it.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/probe/rack6.json` (six-port rack), when `fid cmm define-rack --file rack6.json --json` runs, then a rack with six empty ports and the fixture's `expected_digest` is created.
- **AC-2** Given the AC-1 rack and the probe from `fixtures/cmm/probe/star5_config.json`, when `fid cmm assign-rack-port --rack <id> --port 3 --probe <probe_id> --json` runs, then port 3 holds the probe and the rack digest changes.
- **AC-3** (error path) Given AC-2, when a second probe is assigned to port 3, then the command fails with `RACK_PORT_CONFLICT` naming port 3 and both probe IDs, and the journal head is unchanged.
- **AC-4** (error path) Given AC-2, when the same probe is assigned to port 5, then the command fails with `RACK_DUPLICATE_ASSIGNMENT`.
- **AC-5** (error path) Given fixture `fixtures/cmm/probe/rack_bad_port.json` (approach distance 0), when defined, then it fails with `RACK_PORT_INVALID` naming the port.
- **AC-6** (replay) Given the journal from AC-1 and AC-2, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:** `cmm.define_rack@1` (W2; params `RackModelInput`, `machine_id`; result `{rack_id, version, digest}`); `cmm.assign_rack_port@1` (W2; params `rack_id`, `port: u8`, `probe_id: Option<ProbeId>` where `None` releases the port; result `{rack_id, version, digest}`).
- **Data structures:** `RackModel { id, version, machine_id, name: UntrustedText, pose: Pose, mesh_ref: Option<Digest>, ports: Vec<RackPort>, digest }`; `RackPort { index, pose, approach: [f64; 3], approach_m, clearance_m, assigned: Option<(ProbeId, Digest)> }`.
- **Algorithm:** validation, then conflict checks over all racks of the same machine in ascending rack-ID order; digest via JCS.
- **Errors:** `RACK_PORT_INVALID`, `RACK_PORT_CONFLICT`, `RACK_DUPLICATE_ASSIGNMENT`, `RACK_NOT_FOUND`, `PROBE_NOT_FOUND`.
- **Determinism:** D0.
- **Crate:** `fid-cmm-probe`.

## Test fixtures

Synthetic rack definitions in `fixtures/cmm/probe/` (`rack6.json`, `rack_bad_port.json`), hand-written, CC-BY-4.0; reuses `star5_config.json` from `CMM.PROBE.MODEL`.

## Build prompt

> In the Rust crate `fid-cmm-probe`, implement probe changer racks with commands `cmm.define_rack@1` and `cmm.assign_rack_port@1` (both W2, journaled; CLI `fid cmm define-rack`, `fid cmm assign-rack-port` with `--json`; MCP `cmm_define_rack`, `cmm_assign_rack_port`, generated).
> A rack has a machine-frame pose and 1-32 ports, each with pose, unit approach vector, approach distance and clearance distance (finite, positive; else `RACK_PORT_INVALID`). Assigning a probe to an occupied port fails with `RACK_PORT_CONFLICT` (both IDs); assigning one probe to two ports of the same machine fails with `RACK_DUPLICATE_ASSIGNMENT`; failures journal nothing. Digest = SHA-256 over RFC 8785 JCS of pose, ports and assignments by probe digest.
> Acceptance tests: the six criteria in spec node `CMM.PROBE.RACK`, fixtures in `fixtures/cmm/probe/`.
> Constraints: no panic, no `unsafe`, no `unwrap`/`expect`; no std HashMap or HashSet; determinism D0.

## Open questions

- None.

## Children

_Leaf: none._
