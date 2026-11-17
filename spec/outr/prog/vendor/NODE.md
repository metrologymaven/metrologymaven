---
id: OUTR.PROG.VENDOR
title: Instrument-vendor program journey
level: 3
parent: OUTR.PROG
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.PROG-001, REQ-OUTR.PROG-004, REQ-OUTR.PROG-008]
covers: []
depends_on: [HAL.HOST, HAL.SIM, ECO.SDK]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.PROG.VENDOR: Instrument-vendor program journey

## Purpose

The vendor program page and process: the documented path from downloading the driver SDK to having a listed driver, the simulator requirement per device class, and the intake route for vendors. It explains and links; the SDK, simulators and conformance suites themselves belong to HAL and ECO.

## Scope

**In:**
- Vendor program page: steps from SDK to listed driver, each linked to its defining HAL or ECO node or doc page
- Simulator requirement per listed device class
- Intake process: how a vendor submits a driver and conformance report for listing
- Wording rules: participation is never presented as certification by the project

**Out** (and which node owns it instead):
- Conformance listing status logic (owned by `OUTR.PROG.CONF`)
- Driver SDK and host protocol (owned by `HAL.HOST`), simulators and conformance suites (owned by `HAL.SIM`), plugin packaging and signing (owned by `ECO.SDK`)
- Any fees or commercial terms (business documents outside this tree)

## Requirements

- **REQ-OUTR.PROG.VENDOR-001** The vendor program page shall list every step from SDK download to a listed driver, and each step shall link the HAL or ECO node or docs page that defines it.
- **REQ-OUTR.PROG.VENDOR-002** If a step's link does not resolve in the docs build of the current release, then the site build shall fail and name the step.
- **REQ-OUTR.PROG.VENDOR-003** A driver shall be accepted for listing only if a HAL simulator exists for each device class the driver supports, in line with REQ-FID-011.
- **REQ-OUTR.PROG.VENDOR-004** The intake process shall record, for each submission, the vendor, driver identifier, device classes, conformance report digest and the maintainer who accepted it.
- **REQ-OUTR.PROG.VENDOR-005** Vendor program pages shall pass the OUTR.BRAND claim lint and shall never describe listing as certification or endorsement by the project.

## Interfaces

- **Offers:** vendor program page; intake records consumed by OUTR.PROG.CONF.
- **Consumes:** HAL.HOST SDK docs; HAL.SIM simulators and conformance suites; ECO.SDK packaging; OUTR.BRAND.LINT; OUTR.SITE hosting.

## Decomposition hints

PAGE (journey page and link check), INTAKE (submission records), SIMREQ (simulator requirement check).

## Open questions

- Phase P2 follows HAL's live-device work; the page could be drafted earlier (carried from OUTR.PROG).

## Children

_None yet: status `open`._
