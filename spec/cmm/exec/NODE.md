---
id: CMM.EXEC
title: Motion Executive, Approval Station and run tickets
level: 2
parent: CMM
kind: subsystem
status: open
phase: P2
size: XXL
owners: []
claimed_by:
satisfies: [REQ-CMM-006, REQ-CMM-007, REQ-CMM-008]
covers: [PW.CMM.01]
depends_on: [HAL, AUTO, CORE, UNC, CMM.PROBE, CMM.PATH, CMM.COLL, CMM.SIM]
cli: [fid cmm request-run, fid cmm request-hold, fid cmm *-run*, fid cmm *-ticket*, fid cmm *-temperature*]
mcp: [cmm_request_hold, cmm_get_run*, cmm_list_run*, cmm_get_ticket*]
journal: [cmm.*_run*@*, cmm.*_ticket*@*, cmm.*_hold*@*, cmm.*_temperature*@*]
updated: 2026-11-16
---

# CMM.EXEC: Motion Executive, Approval Station and run tickets

## Purpose

The only path from a program to physical motion. It owns:
- the **run request** (an inert queued record);
- the **run ticket** and its fields;
- the **Approval Station** flow, in which a present human approves with a device-bound WebAuthn assertion;
- the **Motion Executive (ME)**, which verifies the ticket itself, runs deterministic validators, holds the single-writer lease, bounds look-ahead, handles heartbeats and holds, and drives the controller through the HAL I++ client;
- the **setup** and **run-batch** scopes and the speed policy;
- per-cycle placement checks;
- run-time context inputs such as temperature compensation;
- the run record.

It is separate because it is the trust boundary for motion (review zone Z3) and must stay small and isolated from the rest of the engine. It is **machine protection and process assurance, not a safety function**.

## Scope

**In:**
- Run request command (inert record) and its queue; pull by the Approval Station
- Run ticket schema, issue, expiry and voiding rules
- Approval Station flow: renders only ME-supplied data (diff against last released version, simulation report, ticket tuple, agent-authored flag, untrusted text badged)
- WebAuthn assertion verification by the ME against locally enrolled, device-bound credentials
- Release check: tickets only for released programs; try-out mode for unreleased programs at the setup cap
- Deterministic validators: travel limits, velocity and acceleration, collision (via `CMM.COLL`), probe and tool state (via `CMM.PROBE`), articulation, program structure, simulation gate (via `CMM.SIM`)
- Single-writer lease, continuous I++ session while armed, heartbeats (station to ME, operator presence), bounded look-ahead, hold
- Setup scope (first run, operator present, absolute velocity cap at most 30 % of the tenant maximum) and run-batch scope (cycle count and time window, per-cycle physical start, per-cycle placement check)
- Unlocking the approved cap for a (program, machine, setup, probe configuration) tuple after a confirmed first run, and resetting it on any digest change
- MCP hold request (authenticated, scope-gated, rate-limited, journaled, advisory)
- Temperature compensation inputs: part, machine and scale temperatures from Environment devices or manual entry, with material expansion coefficients, recorded in the run record and passed to the controller or kernel
- On-machine alignment execution: manual pre-alignment points recorded as observations (controller-side joystick motion is only observed), then DCC re-alignment
- Run record and transcript references journaled at completion, abort or fault
- Documentation obligations: instructions for use and installation-qualification text stating that no risk-reduction credit may be taken for this software, and the machine-side isolation checklist

**Out** (and which node owns it instead):
- The I++ DME client, simulator, transcripts and pairing ceremony (owned by `HAL`)
- Program language and release lifecycle (`script-release`, owned by `AUTO`)
- Building plans and velocity fields (owned by `CMM.PATH`)
- Collision computation (owned by `CMM.COLL`)
- Simulation runs and reports (owned by `CMM.SIM`)
- Probe qualification records (owned by `CMM.PROBE`)
- Thermal compensation uncertainty contribution (owned by `UNC`)
- Alignment solvers (owned by `ALN`)
- Signing of results (owned by `TRUST`)

## Requirements

- **REQ-CMM.EXEC-001** The system shall expose no MCP tool that starts, continues or resumes motion; the only MCP-originated motion effect shall be a hold request that is authenticated, scope-gated (`run:hold`), rate-limited, journaled and documented as advisory.
- **REQ-CMM.EXEC-002** Loss, presence or failure of MCP connectivity shall never start, continue or resume motion.
- **REQ-CMM.EXEC-003** A run request shall be an inert queued record (class M1, request only) that causes no device I/O until the Approval Station pulls it and a ticket is approved.
- **REQ-CMM.EXEC-004** The Motion Executive shall issue a run ticket only for a program in released state, and shall run unreleased programs only in try-out mode at or below the setup cap with the operator on the pendant.
- **REQ-CMM.EXEC-005** The Motion Executive shall verify the full WebAuthn assertion on every ticket itself, with the challenge derived from the canonical ticket body, against credentials enrolled locally in the Motion Executive; a station key alone shall never suffice.
- **REQ-CMM.EXEC-006** When arming, the Motion Executive shall void the ticket if any of the program, plan, release, machine-model, setup or probe-configuration digests, the controller identity, or the probe qualification validity differs from the ticket.
- **REQ-CMM.EXEC-007** The Motion Executive shall refuse to issue a ticket unless a clean simulation report exists for the byte-identical plan with the same machine, setup and probe configuration digests.
- **REQ-CMM.EXEC-008** The Motion Executive shall run every validator in its validator suite (travel limits with margin, velocity and acceleration, collision, probe and tool state, articulation, program structure) and shall refuse to arm if any validator fails.
- **REQ-CMM.EXEC-009** The first run of any (program, machine, setup, probe configuration) tuple shall run under a single-use setup ticket with an absolute commanded velocity cap of at most 30 % of the tenant maximum, compiled into the plan.
- **REQ-CMM.EXEC-010** While a setup-scope run is active, the Motion Executive shall require a live operator-presence heartbeat from the station and shall hold motion when it is missed.
- **REQ-CMM.EXEC-011** The system shall unlock the approved velocity cap for a tuple only after an operator-confirmed completed setup run, and shall reset it to the setup cap when any digest in the tuple changes.
- **REQ-CMM.EXEC-012** Tenants shall be able to lower the setup cap and shall not be able to raise it above 30 % of the tenant maximum.
- **REQ-CMM.EXEC-013** Under run-batch scope, each cycle shall start only from a physical controller or pendant input, a station-paired hardware button, or a presence tap verified by the Motion Executive, and shall begin with a reduced-speed placement check.
- **REQ-CMM.EXEC-014** If the placement check deviates from the nominal placement by more than its tolerance, then the Motion Executive shall stop the cycle before any full-speed move and journal the deviation.
- **REQ-CMM.EXEC-015** The Motion Executive shall hold a single-writer lease per machine and keep the I++ session open while armed, and shall raise an alarm and hold on any unexpected session loss.
- **REQ-CMM.EXEC-016** The Motion Executive shall never enqueue more moves or seconds of motion beyond the controller-confirmed position than the ticket's look-ahead bound, and shall use single-move look-ahead on controllers recorded as continuing motion after client disconnect.
- **REQ-CMM.EXEC-017** When a hold is requested or a heartbeat is missed, the Motion Executive shall issue the controller's stop or abort and journal the hold with its measured latency.
- **REQ-CMM.EXEC-018** The Motion Executive shall only observe the hardware emergency stop state as reported and shall never trigger, write or reset it.
- **REQ-CMM.EXEC-019** The system shall record temperature compensation inputs (part, machine and scale temperatures with source and timestamp, and material expansion coefficients) in each run record, and shall mark a run whose part temperature is outside the configured window.
- **REQ-CMM.EXEC-020** When a run completes, aborts or faults, the system shall journal a run record referencing the ticket, the I/O transcript digest, the final state and any validator or controller error.
- **REQ-CMM.EXEC-021** The product documentation, instructions for use and installation-qualification pack shall state that the Motion Executive is machine protection and process assurance, not a safety function, and that no risk-reduction credit may be taken for it.
- **REQ-CMM.EXEC-022** The Approval Station shall render only data supplied by the Motion Executive, including a diff against the last released version, the simulation report, the ticket tuple and a prominent agent-authored flag, with program and part names shown as badged untrusted text.

## Interfaces

- **Offers:** commands `cmm.request_run@1` (M1, request only, never on MCP), `cmm.request_hold@1` (MCP-exposed hold request), `cmm.approve_ticket@1` (station only, human, M1), `cmm.confirm_setup_run@1` (human), `cmm.get_run@1` and `cmm.list_runs@1` (R0), `cmm.record_temperature@1` (W2); data types RunRequest, RunTicket, RunRecord, ValidatorReport, TemperatureInputs; local IPC between Approval Station and Motion Executive.
- **Consumes:** `HAL` I++ client, Environment devices, pairing; `AUTO` released Command Programs; `CORE` journal and principals; `CMM.PATH` compiled plan; `CMM.COLL` verdicts and model digests; `CMM.SIM` clean-simulation query; `CMM.PROBE` qualification status; `UNC` thermal inputs format.
- **Journal:** `cmm.*_run*`, `cmm.*_ticket*`, `cmm.*_hold*`, `cmm.*_temperature*`.

## Decomposition hints

REQ (run requests and queue), TICKET (ticket schema, issue, voiding), STATION (Approval Station flow and rendering), AUTHN (WebAuthn verification and enrolment use), VALID (validator suite), LEASE (lease, heartbeats, look-ahead, hold), SCOPE (setup and batch scopes, speed policy, placement check), THERM (temperature inputs), DOCS (positioning, IFU and IQ text).

## Open questions

- **Released-by-other rule** (Design Baseline §8): whether a human other than the requester must release agent-authored programs, or all programs under the Regulated profile. Pending a founder and counsel decision; no decision record exists yet. REQ-CMM.EXEC-004 is written to hold under either outcome. The child that implements the release check should be blocked on a decision when this node is expanded.
- Whether an agent may file the inert run request at all, or only humans. The constitution says M1 is never on MCP, and the Design Baseline says a run request from any gateway is an inert queued record. This node takes the stricter reading (no MCP run request) until a decision says otherwise.
- I++ DME details (port number, fast-queue stop semantics) are taken from concept docs and are [unverified] against VDMA 8722:2026-10.
- The 30 % setup cap is taken from the Design Baseline §8 and is a policy choice, not a measured safety figure.

## Children

_None yet: status `open`._
