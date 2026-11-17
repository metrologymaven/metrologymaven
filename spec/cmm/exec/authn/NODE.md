---
id: CMM.EXEC.AUTHN
title: WebAuthn verification of ticket approvals
level: 3
parent: CMM.EXEC
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.EXEC-005]
covers: []
depends_on: [HAL.PAIR, CMM.EXEC.TICKET]
cli: []
mcp: []
journal: [cmm.approve_ticket@1]
updated: 2026-11-16
---

# CMM.EXEC.AUTHN: WebAuthn verification of ticket approvals

## Purpose

Verifies, inside the Motion Executive, the full WebAuthn assertion a present human makes at the Approval Station for a ticket: the challenge is derived from the canonical ticket body, and the credential must be one enrolled directly into the Motion Executive during the local pairing ceremony, device-bound, with user presence and verification. A station key alone never suffices. The assertion is stored whole in the ticket and the journal.

## Scope

**In:**
- Challenge derivation from the canonical ticket body (domain-separated hash)
- Assertion verification: signature, authenticator data flags (user present, user verified), signature counter, credential ID membership in the Motion Executive's local enrolment, device-bound attestation policy
- `cmm.approve_ticket@1` (M1, station only, human only; never on MCP)
- Storage of the full assertion with the ticket

**Out** (and which node owns it instead):
- Credential enrolment and the pairing ceremony (owned by `HAL.PAIR`)
- Station UI (owned by `CMM.EXEC.STATION`)
- Ticket schema (owned by `CMM.EXEC.TICKET`)

## Requirements

- **REQ-CMM.EXEC.AUTHN-001** The Motion Executive shall derive the WebAuthn challenge as SHA-256 over a fixed domain-separation prefix followed by the RFC 8785 canonical ticket body.
- **REQ-CMM.EXEC.AUTHN-002** The Motion Executive shall accept an approval only if the assertion's signature verifies against a credential enrolled locally in the Motion Executive, the user-present and user-verified flags are set, and the signature counter (when non-zero) increases.
- **REQ-CMM.EXEC.AUTHN-003** If the credential is not device-bound under the enrolment policy, or the only proof offered is a station transport key, then the Motion Executive shall reject the approval with `APPROVAL_UNVERIFIED`.
- **REQ-CMM.EXEC.AUTHN-004** The system shall store the full assertion (authenticator data, client data, signature, credential ID) in the ticket and the journal, not only a hash.
- **REQ-CMM.EXEC.AUTHN-005** `cmm.approve_ticket@1` shall be class M1, accepted only over the local station channel, and never projected to MCP.

## Interfaces

- **Offers:** `cmm.approve_ticket@1` (M1, station only); assertion verifier used at issue and at arming.
- **Consumes:** `HAL.PAIR` local credential store; `CMM.EXEC.TICKET` canonical body.
- **Journal:** `cmm.approve_ticket@1`.

## Decomposition hints

CHAL (challenge derivation), VERIFY (assertion verification), STORE (assertion storage).

## Open questions

- Platform support for device-bound authenticators differs by OS. D-011 (signing user verification where the OS offers none) raises a similar gap for Linux; roaming hardware security keys may cover it here [unverified]. Confirm when this node is expanded.

## Children

_None yet: status `open`._
