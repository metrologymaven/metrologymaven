---
id: OUTR.PROG
title: Instrument-vendor and university programs
level: 2
parent: OUTR
kind: subsystem
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR-008]
covers: []
depends_on: [ECO, HAL, OUTR.CONTENT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.PROG: Instrument-vendor and university programs

## Purpose

Two outreach programs with their own audiences: one for instrument vendors (how to build a driver on the SDK and pass conformance), and one for universities (teaching kits and datasets). They are separate from general content because each has a defined partner journey, entry criteria and a public listing of participants.

## Scope

**In:**
- Vendor program page: driver SDK pointers, conformance suite steps, simulator requirement, how a driver gets listed
- Public list of vendors and drivers with their conformance status
- University program page: teaching kits (lesson plans, exercises, example projects), datasets, classroom install guide
- Public list of participating courses (with consent)
- Contact and intake process for both programs

**Out** (and which node owns it instead):
- The driver SDK, conformance suite and simulators (owned by `HAL` and `ECO`)
- Plugin packaging and signing (owned by `ECO`)
- General tutorials (owned by `OUTR.CONTENT`; teaching kits may reuse them)
- Any paid partnership, certification fee or sales terms (business documents outside this tree)

## Requirements

- **REQ-OUTR.PROG-001** The vendor program page shall describe the steps from SDK download to listed driver, and every step shall link to the HAL or ECO spec node or doc page that defines it.
- **REQ-OUTR.PROG-002** A driver shall be listed as "conformant" only when its latest conformance-suite report for a named release passes, and the listing shall show that release and link the report.
- **REQ-OUTR.PROG-003** If a listed driver's conformance report fails or is older than the two most recent releases, then its listing shall change to "not verified for current release" automatically.
- **REQ-OUTR.PROG-004** The vendor program shall require a simulator for each listed device class, consistent with REQ-FID-011.
- **REQ-OUTR.PROG-005** Each university teaching kit shall contain lesson goals, exercises with expected results, and example projects whose journals replay `IDENTICAL` on the release the kit names.
- **REQ-OUTR.PROG-006** Every dataset offered to universities shall state its licence, origin and digest, and shall be redistributable for teaching.
- **REQ-OUTR.PROG-007** Every teaching kit shall run entirely offline on the PLAT minimum hardware profile with no account, using simulators in place of instruments.
- **REQ-OUTR.PROG-008** Program pages shall pass the OUTR.BRAND claim lint, and shall never describe participation as certification by the project.
- **REQ-OUTR.PROG-009** The project shall run a user training and certification program (curriculum, hands-on exercises on published example projects, proctored or reproducible exams) whose levels map to the acceptance jobs.
- **REQ-OUTR.PROG-010** The project shall participate in the relevant standards and interoperability bodies (QIF/DMSC, ISO TC 213 through national mirrors, the I++ group) and publish a participation log with positions taken.
- **REQ-OUTR.PROG-011** The project shall run a partner-lab program giving maintainers hardware-in-the-loop test access (CMMs, trackers, arms, scanners) and organising physical round-robin measurements of reference artifacts, with results published.

## Interfaces

- **Offers:** vendor and university program pages, participant listings, teaching kits.
- **Consumes:** HAL driver SDK, conformance suite and simulators; ECO plugin packaging, releases and datasets; OUTR.CONTENT example projects; PLAT hardware profiles; OUTR.BRAND lint; OUTR.SITE hosting.

## Decomposition hints

VENDOR (vendor journey and listing), CONF (conformance status listing), UNI (university program), KIT (teaching kits), DATA (teaching datasets).

## Open questions

- Phase P1b is chosen because teaching kits need a usable app; the vendor program's conformance listing realistically follows HAL in P2. A maintainer may want to split phases between the two programs.

## Split rationale

The split follows the **two partner journeys**, each cut into its front door and its deliverables. Vendors: VENDOR (journey page, intake, simulator requirement) and CONF (the generated conformance status). Universities: UNI (program page, intake, course listing), KIT (teaching kits) and DATA (teaching datasets). This also lets the vendor side carry phase P2, following HAL, while the university side starts in P1b, which answers the parent's open question on phases without a phase move.

Alternatives considered:
- **Two children** (vendor, university). Rejected: each would hold both a process and a checkable deliverable, and the conformance status rule is a clean leaf on its own.
- **Merge DATA into KIT.** Rejected: datasets are offered on their own, not only inside kits, and need their own licence check.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.PROG.VENDOR](vendor/NODE.md) | Instrument-vendor program journey | 001 004 008 | none |
| [OUTR.PROG.CONF](conf/NODE.md) | Driver conformance listing status (leaf) | 002 003 | none |
| [OUTR.PROG.UNI](uni/NODE.md) | University program and course listing | 008 | none |
| [OUTR.PROG.KIT](kit/NODE.md) | University teaching kits | 005 007 | none |
| [OUTR.PROG.DATA](data/NODE.md) | Teaching datasets | 006 | none |
| OUTR.PROG.CERT | User training and certification program | 009 | none |
| OUTR.PROG.STDS | Standards-body participation | 010 | none |
| OUTR.PROG.LAB | Partner labs: hardware-in-the-loop access and round-robins | 011 | none |
