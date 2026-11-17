---
id: OUTR.LAUNCH.KIT
title: Conference and trade-show demo kits
level: 3
parent: OUTR.LAUNCH
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.LAUNCH-006, REQ-OUTR.LAUNCH-007]
covers: []
depends_on: [ECO.REL, ECO.DATA, HAL.SIM, OUTR.BRAND.LINT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.LAUNCH.KIT: Conference and trade-show demo kits

## Purpose

Self-contained demo kits for conferences and trade shows: an offline installer set, an example project and journal, a scripted demo that replays to an `IDENTICAL` result, a hardware list and printed material. It is separate because a kit is a physical and digital bundle that one contributor can own, and because demos in public places must never involve uncontrolled motion.

## Scope

**In:**
- Kit manifest: installer set, example project and journal, demo script, hardware list, printed material, checksums
- Offline operation and a replay check of the demo journal
- Simulator use for any device shown; rules for when real hardware and an approval station are part of the kit
- A pre-event checklist

**Out** (and which node owns it instead):
- Installers (owned by `ECO.REL`)
- Example projects and datasets (owned by `ECO.DATA`; `OUTR.CONTENT` may provide tutorial projects)
- Simulators (owned by `HAL.SIM`)
- Booth costs and sponsorship (business documents outside this tree)

## Requirements

- **REQ-OUTR.LAUNCH.KIT-001** Each demo kit shall have a manifest listing every file it contains with a SHA-256 checksum, the release it targets, and its hardware list.
- **REQ-OUTR.LAUNCH.KIT-002** Each demo kit shall install and run on a machine with networking disabled, using only the files in the kit.
- **REQ-OUTR.LAUNCH.KIT-003** The kit's scripted demo shall replay its example journal on the targeted release with outcome `IDENTICAL`, checked automatically when the kit is built.
- **REQ-OUTR.LAUNCH.KIT-004** Where a demo shows a device, the kit shall use the HAL simulator for that device class, unless the kit's manifest lists a human operator and an approval station, in which case motion follows the D-008 rules.
- **REQ-OUTR.LAUNCH.KIT-005** All printed and on-screen material in a kit shall pass the OUTR.BRAND claim lint and the OUTR.BRAND.REVIEW gate before the event.

## Interfaces

- **Offers:** demo kits and their manifests; the kit build and replay check.
- **Consumes:** ECO.REL installers; ECO.DATA example projects; HAL.SIM simulators; OUTR.CONTENT tutorial projects; OUTR.BRAND lint and review.

## Decomposition hints

MANIF (kit manifest and build), DEMO (scripted demo and replay check), HW (hardware list and motion rule), PRINT (printed material).

## Open questions

- Phase P1a: a demo needs a usable app; earlier events can use CLI and journal walkthroughs from OUTR.CONTENT.

## Children

_None yet: status `open`._
