---
id: VIZ.INBOX.GATES.REG
title: Signature ceremony, second-person and step-confirm gates
level: 4
parent: VIZ.INBOX.GATES
kind: feature
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX.GATES-001, REQ-VIZ.INBOX.GATES-002]
covers: []
depends_on: [VIZ.INBOX.GATES.PLAN, TRUST.ESIG.CEREMONY, TRUST.ESIG.MULTI, TRUST.PROFILE.GATE]
uses_artifacts: []
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.INBOX.GATES.REG: Signature ceremony, second-person and step-confirm gates

## Purpose

The gates that need TRUST's signing: the `CeremonyProvider` implementation that launches the e-signature ceremony for promotion into a protected branch and returns the human assertion `journal.promote_protected_branch@1` needs; the `SecondPerson` step that requires a second, distinct human principal; and step-confirm prompts for agent W2 calls when tenant policy asks for them. It is out of launch-1 (TRUST.ESIG is excluded there) and comes with the Regulated profile.

## Scope

**In:**
- `CeremonyProvider` implementation over `TRUST.ESIG.CEREMONY`, showing its outcome
- `SecondPerson` step over `TRUST.ESIG.MULTI` sequences
- Step-confirm prompts for agent W2 calls through the bus hook in `TRUST.PROFILE.GATE`

**Out** (and which node owns it instead):
- Gate plan (`VIZ.INBOX.GATES.PLAN`); Lean steps (`VIZ.INBOX.GATES.LEAN`)
- Signing and meanings (`TRUST.ESIG`); profile hooks (`TRUST.PROFILE.GATE`)

## Requirements

- **REQ-VIZ.INBOX.GATES.REG-001** When the plan includes `Signature`, the step shall run the TRUST ceremony over the promotion pre-image and pass the returned assertion to `journal.promote_protected_branch@1`, and the Inbox shall never create an assertion itself.
- **REQ-VIZ.INBOX.GATES.REG-002** The `SecondPerson` step shall pass only when a human principal different from the first confirms through the TRUST multi-signer sequence.
- **REQ-VIZ.INBOX.GATES.REG-003** While step-confirm is enabled by tenant policy, each agent W2 call shall wait for a human confirm or reject in the Inbox before it commits to the agent branch.

## Interfaces

- **Offers:** a `CeremonyProvider` registered at app start-up; `SecondPerson` and step-confirm UIs.
- **Consumes:** `TRUST.ESIG.CEREMONY`, `TRUST.ESIG.MULTI`, `TRUST.PROFILE.GATE` bus hook; `VIZ.INBOX.GATES.PLAN`.

## Decomposition hints

CEREMONY (provider and outcome display), TWOPERS, STEPCONF.

## Open questions

- Left open (not a leaf) because TRUST.ESIG.CEREMONY and TRUST.PROFILE.GATE are still open and the step-confirm bus hook is not specified. The launch-1 milestone should exclude this node, since it depends on TRUST.ESIG, which launch-1 excludes.
- P1a uses OS-keystore user verification and WebAuthn arrives in P1b per the concept doc [unverified]; TRUST owns that phasing.

## Children

_None yet: status `open`._
