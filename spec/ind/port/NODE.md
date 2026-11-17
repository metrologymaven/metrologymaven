---
id: IND.PORT
title: Portable-device workflow pack
level: 2
parent: IND
kind: subsystem
status: split
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND-001, REQ-IND-002, REQ-IND-004]
covers: [PCD.IND.03]
depends_on: [IND.CONTRACT, HAL, KRN, ALN, GDT]
cli: [fid pack *-portable*]
mcp: [pack_*_portable*]
journal: [pack.*_portable*]
updated: 2026-11-16
---

# IND.PORT: Portable-device workflow pack

## Purpose

Operator-guided measurement routines for portable instruments (articulated arms, laser trackers, handheld scanners): a step list with prompts and images, automatic feature recognition from the points the operator takes, and in-routine checks that tell the operator what to do next. It turns a live device session into a repeatable shop-floor procedure without the operator needing to program.

## Scope

**In:**
- Guided routine definition: ordered steps, each with operator prompt, optional image, expected feature type, point count and acceptance check
- Auto feature recognition from probed points (plane, circle, cylinder, sphere, line, point) with the operator able to override the type
- In-routine guidance: next-step prompts, retake prompts when a check fails, progress display data
- Re-running a recorded routine on a new part (re-targeted replay of the routine steps)
- Routine report template

**Out** (and which node owns it instead):
- Device drivers and the live session itself (owned by `HAL`)
- Device moves, leapfrog and network adjustment (owned by `LVM`)
- Live watch windows and build guidance (owned by `LVM`)
- Feature fits (owned by `KRN`) and alignments (owned by `ALN`)
- Pack manifest, verification harness and provenance (owned by `IND.CONTRACT`)

## Requirements

- **REQ-IND.PORT-001** The portable-device workflow pack shall register its commands, element types, checks and report templates only through the `IND.CONTRACT` manifest.
- **REQ-IND.PORT-002** A guided routine shall be a versioned document object whose steps each declare an operator prompt, an expected feature type, a minimum point count and an acceptance check.
- **REQ-IND.PORT-003** When the operator completes a step, the pack shall recognise the feature type from the probed points, and if the recognised type differs from the expected type, then the pack shall prompt the operator to accept, override or retake before continuing.
- **REQ-IND.PORT-004** If a step's acceptance check fails (for example form residual above the declared limit), then the pack shall prompt a retake and shall journal both the failed and the accepted attempts.
- **REQ-IND.PORT-005** When a recorded routine is run on a new part, the pack shall replay the steps in their recorded order with the same prompts, as a re-targeted replay under CONSTITUTION §4.
- **REQ-IND.PORT-006** The pack shall acquire points only through HAL session commands and shall never itself issue device I/O, so that the side-effect classes of HAL apply unchanged.
- **REQ-IND.PORT-007** The pack shall run every routine end to end against a HAL simulator for arms and trackers, and its manifest shall name a maintainer and the routines' method IDs.
- **REQ-IND.PORT-008** The pack shall ship reference datasets consisting of recorded simulator transcripts for at least one arm and one tracker routine, with expected feature results.

## Interfaces

- **Offers:** element type `GuidedRoutine`; commands such as `pack.create_portable_routine@1` (W2), `pack.run_portable_step@1` (W2, using HAL session points), `pack.recognize_portable_feature@1` (R1); routine report template.
- **Consumes:** HAL portable sessions, simulators and transcripts; KRN fits; ALN alignments; GDT characteristics; IND.CONTRACT manifest and provenance.

## Decomposition hints

ROUTINE (routine model and authoring), RECOG (auto feature recognition), GUIDE (in-routine prompts and retakes), REPLAY (re-run on new part), DATA, TPL.

## Open questions

- PCD.IND.03's row text is short [unverified]; a user of portable incumbents should confirm the expected workflow set before this node is split.
- Auto feature recognition could arguably be a core capability (useful on CMMs too). `CMM.STRAT.LEARN` already specifies the same inference, so decision **D-029** was opened; `IND.PORT.RECOG` is blocked on it.

## Split rationale

The split separates **authoring** a routine (ROUTINE) from **running** it step by step (GUIDE) and **re-running** it on new parts (REPLAY), with recognition (RECOG) isolated because its owner may change (D-029). DATA owns the simulator-based end-to-end proof and TPL the report. REQ-IND.PORT-003 is shared deliberately: RECOG computes the type, GUIDE owns the operator prompt that follows.

Alternatives considered:
- **Merge GUIDE and REPLAY.** Rejected: REPLAY is about binding a routine version to a new part and replay semantics (CORE), GUIDE about per-step interaction with HAL.
- **Keep RECOG inside GUIDE.** Rejected: it would bury a capability that D-029 may move to KRN.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IND.PORT.ROUTINE | Guided routine model and authoring | 001 002 | PCD.IND.03 |
| IND.PORT.RECOG | Auto feature recognition from probed points (decision D-029) | 003 | PCD.IND.03 |
| IND.PORT.GUIDE | In-routine guidance and retakes | 003 004 006 | PCD.IND.03 |
| IND.PORT.REPLAY | Re-running a routine on a new part | 005 | PCD.IND.03 |
| IND.PORT.DATA | Portable routine simulator runs and reference datasets | 007 008 | none |
| IND.PORT.TPL | Routine report template | 001 | none |
