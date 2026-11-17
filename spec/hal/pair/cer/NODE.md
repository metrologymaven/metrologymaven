---
id: HAL.PAIR.CER
title: Pairing ceremony state machine and short authentication string
level: 3
parent: HAL.PAIR
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.PAIR-001, REQ-HAL.PAIR-002, REQ-HAL.PAIR-003]
covers: []
depends_on: [CORE.JRN, TRUST.SIGN]
cli: [fid device start-pairing]
mcp: []
journal: [device.start_pairing@1]
updated: 2026-11-16
---

# HAL.PAIR.CER: Pairing ceremony state machine and short authentication string

## Purpose

The in-person ceremony that introduces a remote endpoint (driver host on an instrument PC, Approval Station instance, station-paired button) to the engine: an administrator starts it, a person at the endpoint confirms physical presence within a time limit, and both screens show a short authentication string that the administrator compares. It is the root of every paired channel, so it is reviewed as a security protocol on its own.

## Scope

**In:**
- `device.start_pairing@1` (A0; never on MCP; never startable by an agent principal)
- Ceremony states: started, awaiting presence, awaiting SAS confirmation, completed, aborted (timeout, mismatch, cancel)
- Physical-presence confirmation: a pairing code on the endpoint's local screen or a press of the button being paired, within the time limit
- Key exchange and short-authentication-string derivation from both public keys and a fresh nonce
- Journaling of completion (with a human assertion) and of every failure
- Endpoint variants: driver host, Approval Station instance, hardware button

**Out** (and which node owns it instead):
- Storing pinned keys, unpairing and re-pairing (owned by `HAL.PAIR.KEYS`)
- Principal binding after pairing (owned by `HAL.PAIR.PRIN`)
- WebAuthn approval credentials enrolled through the ceremony (owned by `CMM` execution)
- Key manifest and signing primitives (owned by `TRUST`)

## Requirements

- **REQ-HAL.PAIR.CER-001** `device.start_pairing@1` shall be class A0, shall not be projected to MCP, and shall fail with `principal_not_permitted` when the caller is an agent or any non-administrator principal.
- **REQ-HAL.PAIR.CER-002** When a ceremony starts, it shall abort with `presence_timeout` and journal the failure if physical-presence confirmation does not arrive within 120 s [unverified].
- **REQ-HAL.PAIR.CER-003** Both endpoints shall derive the short authentication string from both public keys and a fresh nonce of at least 128 bits, and the string shall have at least 20 bits of entropy [unverified].
- **REQ-HAL.PAIR.CER-004** The ceremony shall complete only after the administrator confirms that the strings on both endpoints match; a mismatch report shall abort with `sas_mismatch` and journal the failure.
- **REQ-HAL.PAIR.CER-005** The presence confirmation shall be possible only through an input local to the endpoint (its own screen or the button being paired), never through a network request.

## Interfaces

- **Offers:** `device.start_pairing@1` (A0); ceremony result (peer public key, endpoint kind, human assertion) for `HAL.PAIR.KEYS`.
- **Consumes:** `CORE.JRN`; `TRUST.SIGN` primitives and key manifest.

## Decomposition hints

FSM (state machine and timeouts), SAS (key exchange and string derivation; needs a cryptographic review), PRESENCE (local confirmation per endpoint kind), UI (admin and endpoint screens, owned with `VIZ`).

## Open questions

- Some customers' safety policies may require approval on the controller HMI instead of a kiosk, which changes which endpoints are paired (concept doc 08, open question 8).
- The key-exchange and SAS construction must follow a published, reviewed design; the choice is for the SAS leaf with security review.

## Children

_None yet: status `open`._
