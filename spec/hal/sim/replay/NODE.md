---
id: HAL.SIM.REPLAY
title: L3 controller replay from recorded sessions
level: 3
parent: HAL.SIM
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.SIM-002]
covers: []
depends_on: [HAL.SIM.IPPSIM, HAL.XSCR.PLAY, HAL.XSCR.FMT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.SIM.REPLAY: L3 controller replay from recorded sessions

## Purpose

Fidelity level L3: the I++ simulator acts as the controller recorded in a real session, answering the client from a recording. It turns a field problem on a partner's controller into a regression test anyone can run. It is separate because it needs wire-level recordings and a policy for what recorded behaviour may be published.

## Scope

**In:**
- Wire-level recording of I++ exchanges as transcript entries (a recording mode requested from the I++ client)
- Replay server: presents the recording on a TCP socket, answering by matching client lines
- Divergence reporting at the protocol-line level
- Rules for which recordings may enter the public corpus (no NDA-learned behaviour)

**Out** (and which node owns it instead):
- Capability-level playback (owned by `HAL.XSCR.PLAY`)
- Recording at the HAL boundary (owned by `HAL.XSCR.REC`)
- The client that produces wire-level recordings (owned by `HAL.IPP`)

## Requirements

- **REQ-HAL.SIM.REPLAY-001** Given a wire-level recording, the replay server shall answer each client line that matches the next recorded client line with the recorded server lines and their recorded event ordering.
- **REQ-HAL.SIM.REPLAY-002** If a client line diverges from the recording, then the replay server shall send an I++ error, stop, and report the first differing line index.
- **REQ-HAL.SIM.REPLAY-003** A replay session shall be declared simulated, so every resulting batch is stamped `simulated = true`.
- **REQ-HAL.SIM.REPLAY-004** A recording shall enter the public corpus only with a provenance note stating it contains no behaviour learned under NDA.

## Interfaces

- **Offers:** L3 mode for `fid-ipp-sim`; wire-recording format extension to transcripts.
- **Consumes:** `HAL.SIM.IPPSIM` protocol engine; `HAL.XSCR.FMT`, `HAL.XSCR.PLAY`.

## Decomposition hints

WIRE (wire-level entry kind in transcripts), SERVE (replay server), CORPUS (corpus rules and provenance notes).

## Open questions

- Whether wire-level I++ lines are recorded by `HAL.XSCR.REC` taps or by the I++ client itself is not yet settled; it affects `HAL.IPP` and `HAL.XSCR`.

## Children

_None yet: status `open`._
