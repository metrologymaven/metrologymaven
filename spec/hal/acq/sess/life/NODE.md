---
id: HAL.ACQ.SESS.LIFE
title: Session lifecycle, controlling-writer lock and session context
level: 4
parent: HAL.ACQ.SESS
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ.SESS-001, REQ-HAL.ACQ.SESS-002, REQ-HAL.ACQ.SESS-005, REQ-HAL.ACQ.SESS-006]
covers: []
depends_on: [HAL.ACQ.SESS.REG, HAL.ACQ.SESS.GATE, HAL.IDL.NEG, HAL.IDL.BASE, HAL.ACQ.TIME, HAL.PAIR.GRADE, CORE.BUS.PIPE, CORE.JRN.ENTRY]
side_effect: [D1, D1, D1, R0]
determinism: D0
cli: [fid device open-session, fid device close-session, fid device resume-session, fid device show-session]
mcp: [device_open_session, device_close_session, device_resume_session, device_show_session]
journal: [device.open_session@1, device.close_session@1, device.resume_session@1, device.show_session@1]
updated: 2026-11-17
---

# HAL.ACQ.SESS.LIFE: Session lifecycle, controlling-writer lock and session context

## Purpose

Opens, closes, interrupts and resumes device sessions. At open it runs capability negotiation, anchors the session timebase, grades the source's trust and freezes all of it in a `SessionContext` that every later batch is stamped from. It also enforces one controlling principal per session while any number of read-only telemetry subscribers watch. This is where live sources (P2) get their trust grade from `HAL.PAIR.GRADE`; file sources never pass through here.

## Scope

**In:**
- `device.open_session@1`, `device.close_session@1`, `device.resume_session@1` (D1) and `device.show_session@1` (R0)
- Session states `OPEN`, `INTERRUPTED`, `CLOSED` and their transitions
- The controlling-writer lock and unlimited read-only telemetry subscriptions
- `SessionContext { session_id, source_id, source_kind, negotiation, config_digest, timebase, time_source, trust }` handed to ingest and stamping
- The `device.session_opened` event with negotiation result, configuration digest and time-source declaration
- Trust grading at open through `HAL.PAIR.GRADE`, with inputs supplied by the host layer

**Out:**
- The registry and source kinds (owned by `HAL.ACQ.SESS.REG`)
- The driver-call gate (owned by `HAL.ACQ.SESS.GATE`)
- Configuration commands (owned by `HAL.ACQ.SESS.CFG`)
- Detecting `COMM_LOST` and health tracking (owned by `HAL.ACQ.SESS.HEALTH`, which calls `interrupt`)
- Collecting verified signature, conformance and pairing statuses (owned by `HAL.HOST.SUPER` and `HAL.PAIR`)

## Requirements

- **REQ-HAL.ACQ.SESS.LIFE-001** When `device.open_session@1` runs, the system shall run `negotiate` on the source's declaration, anchor the session timebase once, grade trust, and journal `device.session_opened` with the negotiation result, configuration digest, time-source declaration and trust grade.
- **REQ-HAL.ACQ.SESS.LIFE-002** If negotiation fails with `UNSUPPORTED` for `Device`, then the open shall fail with `UNSUPPORTED`, no session shall exist and only the failure shall be journaled.
- **REQ-HAL.ACQ.SESS.LIFE-003** While a session is open, a second request for the controlling-writer lock shall fail with `session_busy`, and telemetry subscriptions shall remain unlimited and receive no control capability.
- **REQ-HAL.ACQ.SESS.LIFE-004** When the session is interrupted, it shall enter `INTERRUPTED`, keep batches already stamped valid with an `interrupted_after` flag on the session record, refuse new acquisition, and leave the session until an explicit `device.resume_session@1` or `device.close_session@1`.
- **REQ-HAL.ACQ.SESS.LIFE-005** When `device.resume_session@1` succeeds, the system shall re-run negotiation, and if the result or the configuration digest differs from the original, then the resume shall fail with `session_changed` and the session shall stay `INTERRUPTED`.
- **REQ-HAL.ACQ.SESS.LIFE-006** The trust value in the `SessionContext` shall be the output of `HAL.PAIR.GRADE` for the inputs supplied at open; if the host layer supplies no inputs, the trust shall be `declared` when the source asserts an identity and `unknown` otherwise, and never `attested`.
- **REQ-HAL.ACQ.SESS.LIFE-007** The three D1 commands shall be projected to MCP only as requests that a human confirms, and the controlling-writer lock shall be held by the confirming human principal, never by an agent principal (CONSTITUTION §3).
- **REQ-HAL.ACQ.SESS.LIFE-008** The `SessionContext` shall be immutable after open; a change of configuration shall produce a new configuration digest through `HAL.ACQ.SESS.CFG`, recorded as a new context version.

## Acceptance criteria

- **AC-1** Given the gauge simulator registered as `simulator` and a human principal, when `fid device open-session --json` runs, then `device.session_opened` records the negotiation result, configuration digest, time source and trust, and the context's source kind is `simulator`.
- **AC-2** (error path) Given `fixtures/hal/idl/neg/decl_no_device.json` as the declaration, when opening, then it fails with `UNSUPPORTED` and no session exists.
- **AC-3** (error path) Given an open session held by principal A, when principal B requests control, then it fails with `session_busy`; B can subscribe to telemetry and receives no control handle.
- **AC-4** Given an open session, when `interrupt(COMM_LOST)` is called by the health supervisor, then the state is `INTERRUPTED`, acquisition start fails with `session_interrupted`, and earlier batches remain valid.
- **AC-5** (error path) Given an interrupted session whose source reconnects with a different firmware declaration, when `resume-session` runs, then it fails with `session_changed` and the session stays `INTERRUPTED`.
- **AC-6** Given grade inputs `fixtures/hal/pair/grade/all_good.json` supplied by a host test double, when opening, then the context trust is `attested`; given no inputs, it is `declared` for a source with identity.
- **AC-7** Given the generated MCP tool list, then `device_open_session`, `device_close_session` and `device_resume_session` are request-only and `device_show_session` is exposed.

## Design

- **Registry commands:** `device.open_session@1` (D1; params `source_id`, `config: Option<ConfigRef>`; result `SessionHandle`), `device.close_session@1` (D1; params `session_id`), `device.resume_session@1` (D1; params `session_id`), `device.show_session@1` (R0; params `session_id`; result `SessionView`).
- **Data structures:** `Session { id, state, context_versions: Vec<SessionContext>, controller: PrincipalId, subscribers }`, `SessionContext`, `GradeInputs` (type of `HAL.PAIR.GRADE`), `SessionTimebase` (from `HAL.ACQ.TIME`).
- **Algorithm:** open = registry lookup -> `negotiate` -> `SessionTimebase::anchor` -> `grade(inputs)` or ungraded fallback -> gated driver `open_session` -> journal. Interrupt is an internal call from `HAL.ACQ.SESS.HEALTH`, journaled as `device.event`.
- **Errors:** `UNSUPPORTED`, `session_busy`, `session_interrupted`, `session_changed`, `session_unknown`, `source_unknown`, driver errors of the `Device@1` model.
- **Determinism:** D0 for the journaled open record given the same declaration, configuration, grade inputs and anchor; the anchor itself is an observed input.
- **Performance budget:** open under 100 ms excluding the device [unverified until measured].
- **Crate:** `fid-hal-runtime` (module `session`), layer L2.

## Test fixtures

Simulators from `HAL.SIM.GAUENV` (or a test double until available); declarations from `fixtures/hal/idl/neg/`; grade inputs from `fixtures/hal/pair/grade/`.

## Build prompt

> In crate `fid-hal-runtime`, module `session`, implement registry commands `device.open_session@1`, `device.close_session@1`, `device.resume_session@1` (all D1, MCP request-only, lock held by the confirming human) and `device.show_session@1` (R0). Open: look up the source in the registry, run `fid-hal-idl::negotiate` (Device unsupported -> fail `UNSUPPORTED`), anchor the session timebase once, compute trust with `fid-hal-pair::grade` from host-supplied `GradeInputs` (no inputs -> `declared` if identity asserted else `unknown`, never `attested`), make the gated driver `open_session` call, freeze an immutable `SessionContext { session_id, source_id, source_kind, negotiation, config_digest, timebase, time_source, trust }`, and journal `device.session_opened`. One controlling principal (`session_busy` for others); unlimited read-only telemetry subscribers. `interrupt(reason)` (called by the health supervisor) moves to `INTERRUPTED`, refuses acquisition (`session_interrupted`) and keeps earlier batches valid; resume re-negotiates and fails `session_changed` if negotiation or configuration digest differ. No panic, unwrap, expect or unsafe; BTreeMap only (CONSTITUTION §8). Tests: AC-1..AC-7.

## Open questions

- How the host layer gathers verified `GradeInputs` (driver signature, conformance report, pairing) is owned by `HAL.HOST.SUPER` and `HAL.PAIR`; until they exist, sessions run on the ungraded fallback.
- Whether request-only D1 tools ship on MCP in P2 or later (concept doc 08 H-D18) is inherited from `HAL.ACQ`.

## Children

_Leaf: none._
