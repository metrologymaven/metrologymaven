---
id: HAL.PAIR
title: Pairing ceremony and driver trust grades
level: 2
parent: HAL
kind: subsystem
status: split
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL-007, REQ-HAL-004]
covers: []
depends_on: [CORE, TRUST]
cli: [fid device *-pairing, fid device *-pairings]
mcp: [device_list_pairings]
journal: [device.*_pairing]
updated: 2026-11-16
---

# HAL.PAIR: Pairing ceremony and driver trust grades

## Purpose

Establishes who a remote endpoint is, in person, and how much its data is trusted. It owns the local pairing ceremony (administrator, physical presence, short-authentication-string comparison) for driver hosts on instrument PCs, the Approval Station instance, and station-paired buttons; the resulting pinned keys; and the rules that grade a source's provenance as `attested`, `declared` or `unknown`. It is separate because it is the HAL's security boundary and must be reviewed as one unit.

## Scope

**In:**
- The pairing ceremony: admin start, physical-presence confirmation within a short time limit, short-authentication-string derivation and comparison on both endpoints
- Endpoints: remote driver hosts ↔ engine; Approval Station instance ↔ machine-side process; station-paired hardware buttons
- Pinned keys (hardware-bound where available), enrolment under the key manifest, unpairing and re-pairing
- Principal binding: the paired channel, never a request body, defines the device or service principal
- Trust-grade rules: inputs (driver signature, valid conformance report, firmware in tested range, pairing state) and the resulting grade `attested | declared | unknown`
- Firmware-drift handling: graded downgrade of trust, and a motion block signal for out-of-range controller firmware

**Out** (and which node owns it instead):
- Writing the grade into stamps (owned by `HAL.ACQ`)
- Driver signature verification at launch (owned by `HAL.HOST`)
- WebAuthn approval credentials, run tickets and the Approval Station itself (owned by `CMM` execution)
- Key manifest, signing keys and verification tooling (owned by `TRUST`)

## Requirements

- **REQ-HAL.PAIR-001** Pairing shall be an admin (A0) command that is never exposed on MCP and can never be started by an agent principal.
- **REQ-HAL.PAIR-002** When pairing starts, the ceremony shall require a physical-presence confirmation at the endpoint within 120 s [unverified], and shall abort and journal a failure if it does not arrive.
- **REQ-HAL.PAIR-003** Both endpoints shall display a short authentication string derived from both public keys and a fresh nonce, and pairing shall complete only after the administrator confirms the strings match.
- **REQ-HAL.PAIR-004** When pairing completes, the system shall pin both public keys, write a journal entry with a human assertion, and from then on accept the endpoint only over a mutually authenticated channel using the pinned keys.
- **REQ-HAL.PAIR-005** The trust-grade rules shall assign `attested` only when the driver signature is valid, a valid conformance report covers the driver and capability, and the device firmware is within the tested range; otherwise `declared` when the source asserts its identity, else `unknown`.
- **REQ-HAL.PAIR-006** If a paired endpoint presents a different key or a changed host binary, then the system shall refuse the connection and require a new ceremony or re-allowlisting.

## Interfaces

- **Offers:** `device.start_pairing@1` (A0), `device.remove_pairing@1` (A0), `device.list_pairings@1` (R0); pinned-key store for HAL.HOST and CMM; trust-grade function for HAL.ACQ; firmware-drift signal for the Motion Executive.
- **Consumes:** CORE principals and journal; TRUST key manifest.

## Decomposition hints

CER (ceremony state machine and SAS), KEYS (pinned keys, enrolment, unpairing), PRIN (principal binding on paired channels), GRADE (trust-grade and firmware-drift rules).

## Open questions

- Whether some customers' safety policies require approval on the controller HMI instead of the kiosk affects which endpoints are paired (concept doc 08, open question 8).

## Split rationale

The split follows **the order of a trust relationship**: it is established once in person (CER), its result is held and checked on every connection (KEYS), every message on it is attributed to a principal (PRIN), and the data that flows over it is graded (GRADE). PRIN and GRADE are pure functions and are finished as leaves; CER and KEYS carry the cryptographic and platform choices that need review.

Alternatives considered:
- **Ceremony and keys as one child.** Rejected: the ceremony is reviewed as a protocol, the key store as platform code with per-OS hardware keys.
- **Trust grading in HAL.ACQ.** Rejected by the parent split: the grade is a security rule and stays with pairing; ACQ only writes it.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| HAL.PAIR.CER | Pairing ceremony state machine and short authentication string | 001 002 003 | none |
| HAL.PAIR.KEYS | Pinned keys, enrolment, unpairing and re-pairing | 004 006 | none |
| HAL.PAIR.PRIN | Principal binding on paired channels — leaf | 004 | none |
| HAL.PAIR.GRADE | Trust-grade rules and firmware-drift signal — leaf | 005 | none |
