---
id: CMM.EXEC.TICKET
title: Run tickets, release check and arming-time voiding
level: 3
parent: CMM.EXEC
kind: module
status: open
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-CMM.EXEC-004, REQ-CMM.EXEC-006]
covers: []
depends_on: [AUTO.REC, CMM.GEN.PROV, CMM.PROBE.VALID, CMM.EXEC.REQ]
cli: [fid cmm show-ticket, fid cmm list-tickets]
mcp: [cmm_get_ticket]
journal: [cmm.issue_ticket@1, cmm.void_ticket@1]
updated: 2026-11-16
---

# CMM.EXEC.TICKET: Run tickets, release check and arming-time voiding

## Purpose

Owns the run ticket: its schema (program, plan and release digests; machine, controller identity and machine model; setup digests; probe configuration digest and qualification `valid_until`; speed scope and look-ahead bound; simulation report; validator suite version; scope; requester; approver and full WebAuthn assertion), its issue rules, its expiry and single use, and the check at arming that voids it if anything bound by it has changed. It also applies the release rules of D-008 before any ticket exists.

## Scope

**In:**
- Ticket schema and canonical body (the WebAuthn challenge is derived from it)
- Release check: released programs only; released-by-other for agent-authored programs and under the Regulated profile; otherwise the same person may release and approve as two distinct acts
- Try-out mode for unreleased programs (setup cap, operator on pendant)
- Ticket issue, time-to-live (default at most 15 minutes), single use, voiding
- Arming-time comparison of every bound digest, controller identity and qualification validity
- Read-only ticket status (`cmm.get_ticket@1`, R0)

**Out** (and which node owns it instead):
- WebAuthn verification (owned by `CMM.EXEC.AUTHN`)
- Validators and the simulation gate (owned by `CMM.EXEC.VALID`)
- Speed scope values (owned by `CMM.EXEC.SCOPE`)
- Program release lifecycle (owned by `AUTO.REC`)

## Requirements

- **REQ-CMM.EXEC.TICKET-001** The Motion Executive shall issue a ticket only for a program in released state, and shall otherwise offer only try-out mode at or below the setup cap with the operator on the pendant.
- **REQ-CMM.EXEC.TICKET-002** If the program's provenance has `agent_authored: true`, or the tenant runs the Regulated profile, then the Motion Executive shall refuse a ticket unless the releasing human differs from the requester (D-008).
- **REQ-CMM.EXEC.TICKET-003** Where released-by-other is not required, the system shall still record release and approval as two distinct journaled acts, each with its own assertion, even when one person performs both.
- **REQ-CMM.EXEC.TICKET-004** The system shall bind in every ticket the program, plan and release digests, controller identity, machine-model digest, fixture and part-placement digests, probe configuration digest and earliest `valid_until`, speed scope and look-ahead bound, simulation report digest, validator suite version, scope, requester and approver.
- **REQ-CMM.EXEC.TICKET-005** A ticket shall expire at its `expires_at` (default and maximum 15 minutes after issue [unverified]) and shall be usable for exactly one arming.
- **REQ-CMM.EXEC.TICKET-006** When arming, the Motion Executive shall void the ticket if any bound digest, the controller identity or the probe qualification validity differs from the ticket, and shall journal which field differed.

## Interfaces

- **Offers:** RunTicket type and canonical body; `cmm.get_ticket@1` (R0); ticket issue and void events.
- **Consumes:** `AUTO.REC` release state and releaser; `CMM.GEN.PROV` agent-authored flag; `CMM.PROBE.VALID` status; `CMM.EXEC.REQ` run requests; tenant profile from `CORE`.
- **Journal:** `cmm.issue_ticket@1`, `cmm.void_ticket@1`.

## Decomposition hints

SCHEMA (ticket body and canonical form), RELEASE (release and released-by-other check), ISSUE (issue, expiry, single use), ARM (arming-time comparison and voiding).

## Open questions

- **Revise for D-036/D-037 (accepted 2026-11-17):** a ticket may be issued for an agent-filed request when the program carries a valid H1 approval for the same envelope and tuple; any digest change invalidates it.
- The released-by-other question raised at level 2 is settled by D-008 (option B): released-by-other for agent-authored programs and the Regulated profile; same person otherwise as two distinct acts with mandatory simulation and a setup run. Counsel review is still recommended before the first live-machine release.
- The 15-minute default comes from the HAL concept doc and is a policy choice [unverified].

## Children

_None yet: status `open`._
