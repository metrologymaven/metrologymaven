---
id: CMM.EXEC.SCOPE
title: Setup and run-batch scopes, speed policy and placement check
level: 3
parent: CMM.EXEC
kind: module
status: open
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-CMM.EXEC-009, REQ-CMM.EXEC-010, REQ-CMM.EXEC-011, REQ-CMM.EXEC-012, REQ-CMM.EXEC-013, REQ-CMM.EXEC-014]
covers: []
depends_on: [CMM.PATH.CAP, CMM.EXEC.TICKET, CMM.EXEC.DRIVE]
cli: [fid cmm confirm-setup-run, fid cmm set-setup-cap]
mcp: []
journal: [cmm.confirm_setup_run@1, cmm.set_setup_cap@1, cmm.check_placement@1]
updated: 2026-11-16
---

# CMM.EXEC.SCOPE: Setup and run-batch scopes, speed policy and placement check

## Purpose

Owns the speed policy and run scopes. The first run of any (program, machine, setup, probe configuration) tuple runs under a single-use setup ticket at an absolute commanded velocity cap of at most 30 % of the tenant maximum, with a live operator-presence heartbeat. A completed, operator-confirmed setup run unlocks the approved cap for that tuple; any digest change resets it. Run-batch scope bounds cycle count and time window, starts each cycle only from a physical input or verified presence tap, and begins each cycle with a reduced-speed placement check.

## Scope

**In:**
- Tuple state: setup required, or approved cap unlocked, per (program, machine, setup, probe) digests
- Setup cap: tenant setting, at most 30 % of the tenant maximum, may only be lowered (`cmm.set_setup_cap@1`, A0, never on MCP)
- Setup-run confirmation by a human (`cmm.confirm_setup_run@1`, human only)
- Operator-presence heartbeat requirement during setup runs
- Run-batch scope: cycle count, time window, per-cycle start sources
- Per-cycle placement check: reduced-speed touch-off on two or three safe points, or a fixture or part ID check, against nominal placement with a tolerance
- Supplying the cap to `CMM.PATH.CAP` for compilation

**Out** (and which node owns it instead):
- Clamping velocities in the plan (owned by `CMM.PATH.CAP`)
- Executing the hold (owned by `CMM.EXEC.DRIVE`)
- Ticket schema (owned by `CMM.EXEC.TICKET`)

## Requirements

- **REQ-CMM.EXEC.SCOPE-001** The first run of any (program, machine, setup, probe configuration) tuple shall run under a single-use setup ticket whose absolute commanded velocity cap is at most 30 % of the tenant maximum, compiled into the plan.
- **REQ-CMM.EXEC.SCOPE-002** While a setup-scope run is active, the Motion Executive shall require the station's operator-presence heartbeat at the configured interval and shall hold motion when one is missed.
- **REQ-CMM.EXEC.SCOPE-003** The system shall unlock the approved cap for a tuple only after a human confirms a completed setup run, and shall reset the tuple to setup scope when any digest in it changes.
- **REQ-CMM.EXEC.SCOPE-004** The system shall let a tenant administrator lower the setup cap and shall reject any value above 30 % of the tenant maximum with `SETUP_CAP_TOO_HIGH`.
- **REQ-CMM.EXEC.SCOPE-005** Under run-batch scope, each cycle shall start only from a physical controller or pendant input, a station-paired hardware button, or a presence tap verified by the Motion Executive, and never from a kiosk click or engine command.
- **REQ-CMM.EXEC.SCOPE-006** Each batch cycle shall begin with a reduced-speed placement check, and if the measured placement deviates from nominal by more than its tolerance, the Motion Executive shall stop the cycle before any full-speed move and journal the deviation.

## Interfaces

- **Offers:** tuple speed state; `cmm.confirm_setup_run@1` (human only), `cmm.set_setup_cap@1` (A0), `cmm.check_placement@1` (inside the Motion Executive); cap values for compilation.
- **Consumes:** `CMM.PATH.CAP`; `CMM.EXEC.TICKET`; `CMM.EXEC.DRIVE` for holds and placement touch-off moves.
- **Journal:** `cmm.confirm_setup_run@1`, `cmm.set_setup_cap@1`, `cmm.check_placement@1`.

## Decomposition hints

TUPLE (tuple state and reset), CAP (setup cap policy), PRESENCE (heartbeat requirement), BATCH (batch scope and cycle start), PLACE (placement check).

## Open questions

- **Revise for D-036/D-037 (accepted 2026-11-17):** unchanged by D-037: per-cycle physical start, placement check and setup runs remain mandatory safety interlocks.
- The 30 % setup cap comes from the Design Baseline §8 and is a policy choice, not a measured safety figure.
- Placement-check point selection (which two or three safe points) needs a rule; the leaf may take them from the program's alignment features.

## Children

_None yet: status `open`._
