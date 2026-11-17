---
id: OUTR.PROG.UNI
title: University program and course listing
level: 3
parent: OUTR.PROG
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.PROG-008]
covers: []
depends_on: [OUTR.PROG.KIT, OUTR.PROG.DATA]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.PROG.UNI: University program and course listing

## Purpose

The university program page and process: what the program offers (teaching kits, datasets, classroom install guide), how a course joins, and the public list of participating courses shown only with each course's consent. Kits and datasets are their own children; this node is the front door.

## Scope

**In:**
- University program page and classroom install guide
- Intake process for instructors
- Public course listing with consent records
- Wording rules: participation is not certification of the course or its students

**Out** (and which node owns it instead):
- Teaching kits (owned by `OUTR.PROG.KIT`)
- Teaching datasets (owned by `OUTR.PROG.DATA`)
- GD&T expert partnerships for the DMT (decision D-009; may be linked)

## Requirements

- **REQ-OUTR.PROG.UNI-001** The university program page shall list every available teaching kit and dataset with the release each targets.
- **REQ-OUTR.PROG.UNI-002** A course shall appear in the public listing only when a written consent from the instructor is on file, and shall be removed within 14 days [unverified target] of withdrawal.
- **REQ-OUTR.PROG.UNI-003** The classroom install guide shall describe an offline, account-free installation on the PLAT minimum hardware profile.
- **REQ-OUTR.PROG.UNI-004** University program pages shall pass the OUTR.BRAND claim lint and shall never describe participation as certification by the project.

## Interfaces

- **Offers:** university program page, course listing, install guide.
- **Consumes:** OUTR.PROG.KIT and OUTR.PROG.DATA lists; PLAT minimum hardware profile; OUTR.BRAND.LINT; OUTR.SITE hosting.

## Decomposition hints

PAGE, INTAKE, LIST (course listing and consent).

## Open questions

- D-009 recommends a university GD&T partnership; the program could be its home, but the DMT work itself stays in GDT.

## Children

_None yet: status `open`._
