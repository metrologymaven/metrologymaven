---
id: CMM.PROBE
title: Probe and sensor configuration and qualification
level: 2
parent: CMM
kind: subsystem
status: open
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-CMM-001]
covers: [PCD.PRB.*]
depends_on: [HAL, KRN, UNC, AUTO]
cli: [fid cmm *-probe*, fid cmm *-sensor*, fid cmm *-rack*]
mcp: [cmm_*_probe*, cmm_*_sensor*, cmm_*_rack*]
journal: [cmm.*_probe*@*, cmm.*_sensor*@*, cmm.*_rack*@*]
updated: 2026-11-16
---

# CMM.PROBE: Probe and sensor configuration and qualification

## Purpose

Owns the probe model: what is mounted on the machine, how it is built up (head, extensions, stylus, star tips, scanning or non-contact sensor), where the tool racks are, and whether each tip or sensor is currently qualified. Every other CMM subsystem asks this one "which tip, at which angle, with which effective radius and offset, and is it still valid?". It is separate because the probe model is a long-lived, versioned entity shared by strategies, paths, collision checks, simulation and run tickets, and because its validity rules gate execution.

## Scope

**In:**
- Probe configuration entities: head, extensions, stylus, star and disc tips, tip diameter, length and material, as journaled, content-addressed configurations
- Articulating and indexing heads: angle sets, index grids, reachable-angle tables (the head's kinematics as configuration data)
- Probe changer racks: ports, port poses, pick and drop approach data
- Scanning-probe parameters (analog and touch-trigger distinction, deflection limits) as configuration
- Non-contact sensor configuration: laser line and vision sensors, standoff, field of view, exposure parameters
- The qualification workflow: reference sphere definition and pose, qualification program template, reduction of qualification hits to tip offset, effective radius and form residual, and the qualification record
- Qualification validity: expiry time, temperature drift window, configuration digest, and the `valid_until` used by run tickets
- Probe and sensor configuration digest used by run tickets and validators

**Out** (and which node owns it instead):
- Driving the machine during qualification (owned by `CMM.EXEC`; qualification is a Command Program executed there)
- Device drivers and tool-change protocol calls (owned by `HAL`)
- Sphere fitting itself (owned by `KRN`)
- Probing uncertainty models (owned by `UNC`)
- Swept stylus and head volumes for collision checking (owned by `CMM.COLL`, which consumes the configuration)
- Which tip and angle a feature uses (owned by `CMM.STRAT` and `CMM.PATH`)

## Requirements

- **REQ-CMM.PROBE-001** The system shall represent every probe configuration (head, extensions, stylus, tips, sensor) as a versioned entity with a stable ID and a content digest that changes when any geometric or sensor parameter changes.
- **REQ-CMM.PROBE-002** The system shall store tip and stylus dimensions in SI metres with their authored unit, and shall reject a configuration whose tip diameter, stylus length or extension length is non-positive or non-finite.
- **REQ-CMM.PROBE-003** The system shall model articulating and indexing heads with a discrete or continuous angle set, and shall reject any requested head angle outside that set.
- **REQ-CMM.PROBE-004** The system shall model probe changer racks with port poses and assigned configurations, and shall report a conflict when two configurations claim the same port.
- **REQ-CMM.PROBE-005** The system shall model non-contact sensors (laser line, vision) with standoff range, field of view and acquisition parameters, all bounded and validated at entry.
- **REQ-CMM.PROBE-006** When qualification hits on a reference sphere are supplied, the system shall compute each tip's offset vector, effective radius and form residual through the kernel sphere fit and journal one qualification record per tip.
- **REQ-CMM.PROBE-007** If a qualification's form residual exceeds the user-configured limit, then the system shall mark that tip unqualified and shall not issue it a validity period.
- **REQ-CMM.PROBE-008** The system shall attach to every qualified tip a validity period (expiry time and temperature drift window), and shall report the tip unqualified once either bound is exceeded.
- **REQ-CMM.PROBE-009** When asked by the run-ticket validator, the system shall return for a probe configuration its digest, per-tip qualification status and the earliest `valid_until`, as a read-only (R0) query.
- **REQ-CMM.PROBE-010** The system shall provide a qualification program template per head type that produces a Command Program, so qualification motion runs only through the Motion Executive.

## Interfaces

- **Offers:** commands `cmm.define_probe@1`, `cmm.define_sensor@1`, `cmm.define_rack@1` (W2), `cmm.qualify_probe@1` (W2: reduces supplied hits to a qualification record; no motion), `cmm.get_probe_status@1` (R0); data types ProbeConfig, HeadAngleSet, RackModel, SensorConfig, QualificationRecord.
- **Consumes:** `KRN` sphere fit; `HAL` TactileProbe and Rotary capability descriptors; `UNC` probing uncertainty inputs; `AUTO` Command Program format for the qualification template. Qualification programs are run by `CMM.EXEC`, which depends on this node.
- **Journal:** `cmm.*_probe*`, `cmm.*_sensor*`, `cmm.*_rack*` events with result digests.

## Decomposition hints

MODEL (tips, stylus, extensions, stars), HEAD (articulating and indexing heads), RACK (changers), SENSOR (scanning and non-contact sensors), QUAL (qualification reduction and records), VALID (expiry, drift, digest queries).

## Open questions

- Whether qualification on the controller side (server-owned compensation, as the run ticket's `compensation_owner: server` suggests) or client side is the default per controller type [unverified].
- Default qualification form-residual limit and expiry period need a domain expert; no default is set here.

## Children

_None yet: status `open`._
