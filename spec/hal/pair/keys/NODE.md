---
id: HAL.PAIR.KEYS
title: Pinned keys, enrolment, unpairing and re-pairing
level: 3
parent: HAL.PAIR
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.PAIR-004, REQ-HAL.PAIR-006]
covers: []
depends_on: [HAL.PAIR.CER, TRUST.SIGN, CORE.STO]
cli: [fid device remove-pairing, fid device list-pairings]
mcp: [device_list_pairings]
journal: [device.remove_pairing@1, device.list_pairings@1]
updated: 2026-11-16
---

# HAL.PAIR.KEYS: Pinned keys, enrolment, unpairing and re-pairing

## Purpose

Holds the outcome of every ceremony: the pinned public keys of both ends (hardware-bound where the platform allows), their enrolment under the project key manifest, and the rules for unpairing, key loss and re-pairing. It is what every later connection is checked against.

## Scope

**In:**
- Pairing records: endpoint ID, endpoint kind, pinned peer key, local key reference, host binary digest at pairing, creation journal entry
- Hardware-bound local keys (TPM or platform keystore) where available, with a recorded fallback when not
- Enrolment of keys under the key manifest
- `device.remove_pairing@1` (A0), `device.list_pairings@1` (R0)
- Refusal of connections presenting a different key or changed binary; re-pairing through a new ceremony

**Out** (and which node owns it instead):
- The ceremony itself (owned by `HAL.PAIR.CER`)
- Re-allowlisting a changed host binary (owned by `HAL.HOST.PKG`)
- Mapping a channel to a principal (owned by `HAL.PAIR.PRIN`)

## Requirements

- **REQ-HAL.PAIR.KEYS-001** When a ceremony completes, the system shall store a pairing record with both public keys pinned and journal it with the ceremony's human assertion.
- **REQ-HAL.PAIR.KEYS-002** If an endpoint presents a key different from its pinned key, then the connection shall be refused with `pairing_key_mismatch`, journaled, and the pairing record left unchanged.
- **REQ-HAL.PAIR.KEYS-003** If a paired driver host reports a binary digest different from the one recorded at pairing and not on the allowlist, then the connection shall be refused with `host_binary_changed`.
- **REQ-HAL.PAIR.KEYS-004** `device.remove_pairing@1` shall be class A0, never on MCP, and shall revoke the record so that the next connection from that endpoint is refused.
- **REQ-HAL.PAIR.KEYS-005** The local private key shall be generated inside a hardware-backed keystore when the platform offers one, and the pairing record shall state whether it is hardware-bound.
- **REQ-HAL.PAIR.KEYS-006** `device.list_pairings@1` shall be R0 and shall return endpoint ID, kind, key fingerprints, hardware-bound flag and creation time, never private key material.

## Interfaces

- **Offers:** pinned-key store for `HAL.HOST.PROTO`, `HAL.PAIR.PRIN` and `CMM` execution; the two commands.
- **Consumes:** `HAL.PAIR.CER` results; `TRUST.SIGN` key manifest; `CORE.STO`.

## Decomposition hints

STORE (pairing records), HWKEY (hardware-bound keys per OS), CHECK (connection-time key and digest checks), CMDS (remove and list; likely a leaf).

## Open questions

- Hardware-bound key support differs per OS; the support matrix belongs to `PLAT`.

## Children

_None yet: status `open`._
