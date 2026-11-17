---
id: VIZ.INBOX.GATES
title: Governance-profile gates on promotion
level: 3
parent: VIZ.INBOX
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX-003]
covers: []
depends_on: [TRUST.PROFILE.SELECT, TRUST.PROFILE.DEFS, CORE.JRN.BR]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.INBOX.GATES: Governance-profile gates on promotion

## Purpose

The UI gates that governance profiles add on top of promotion: optional step-confirm of agent W2 calls, reason entry, two-person rules and e-signature ceremonies for protected targets (Lean, Regulated and Assistive-only profiles). The gates never change side-effect classes; they only add human steps before CORE promotion, and signing happens out of band through TRUST.

## Scope

**In:**
- Gate selection from the active governance profile and target protection
- Reason entry and two-person confirmation UI
- Launching the TRUST e-signature ceremony for W3 promotion and showing its outcome
- Step-confirm prompts for agent W2 calls when the tenant policy is on

**Out** (and which node owns it instead):
- Profiles and their rules (`TRUST.PROFILE`)
- Signing and e-signature meaning (`TRUST.ESIG`)
- Promotion (`CORE`)

## Requirements

- **REQ-VIZ.INBOX.GATES-001** When the target is protected, apply shall require the e-signature ceremony defined by the active profile before CORE promotion, and the Inbox shall never sign on the human's behalf.
- **REQ-VIZ.INBOX.GATES-002** Where the profile requires two persons, the gate shall require a second human principal distinct from the first.
- **REQ-VIZ.INBOX.GATES-003** Gates shall be unavailable to agent principals in every profile.
- **REQ-VIZ.INBOX.GATES-004** If a gate step is cancelled or fails, then no promotion shall occur and the proposal shall stay pending.

## Interfaces

- **Offers:** gate hooks used by `VIZ.INBOX.ACTIONS`.
- **Consumes:** `TRUST.PROFILE` rules; `TRUST.ESIG` ceremony; CORE principals.

## Decomposition hints

SELECT (gate selection), REASON, TWOPERS, ESIGUI, STEPCONF.

## Open questions

- Launch-1 excludes TRUST.ESIG, so the Lean profile must work without it: `VIZ.INBOX.GATES.PLAN` defines a `CeremonyProvider` trait that is optional, and `VIZ.INBOX.GATES.LEAN` needs no signing. Only `VIZ.INBOX.GATES.REG` (open, P1b) depends on TRUST.ESIG; the launch-1 milestone should exclude it.
- Without a ceremony, promotion into a protected branch (`main` after the first release) is unavailable in launch-1; TRUST must say how Lean obtains the human assertion that `CORE.JRN.BR.PROMO` requires [cross-domain gap].
- The dependency on `VIZ.INBOX.ACTIONS` was removed: GATES offers the plan, ACTIONS consumes it.
- P1a uses OS-keystore user verification and WebAuthn arrives in P1b per the concept doc [unverified]; TRUST owns that phasing.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.INBOX.GATES.PLAN | Gate plan selection from profile, protection and principal (leaf) | 001 003 004 | none |
| VIZ.INBOX.GATES.LEAN | Count-confirm and reason gate steps (leaf) | 003 004 | none |
| VIZ.INBOX.GATES.REG | Signature ceremony, second-person and step-confirm gates | 001 002 | none |
