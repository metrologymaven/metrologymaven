---
id: HAL.PAIR.PRIN
title: Principal binding on paired channels
level: 3
parent: HAL.PAIR
kind: task
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-HAL.PAIR-004]
covers: []
depends_on: [HAL.PAIR.KEYS, CORE.BUS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.PAIR.PRIN: Principal binding on paired channels

## Purpose

Decides who is speaking on a paired channel. The principal of every message from a driver host, Approval Station or button is derived from the authenticated peer key of the channel, never from anything in the message. This closes the class of attacks where a request body claims to be a more privileged principal.

## Scope

**In:**
- `bind(peer_key_fingerprint, pairing_store) -> Principal | Refusal`
- Principal kinds `device` and `service`, with the pairing record's endpoint ID
- Rejection of message-level principal claims

**Out:**
- Authenticating the channel (owned by `HAL.HOST.PROTO` and `HAL.PAIR.KEYS`)
- Human and agent principals (owned by `CORE`)

## Requirements

- **REQ-HAL.PAIR.PRIN-001** The binder shall derive the principal of a paired channel only from the pairing record that matches the channel's authenticated peer-key fingerprint.
- **REQ-HAL.PAIR.PRIN-002** If no active pairing record matches the fingerprint, then binding shall fail with `principal_unpaired`.
- **REQ-HAL.PAIR.PRIN-003** If a message on a paired channel carries a principal field that differs from the bound principal, then the message shall be rejected with `principal_claim_rejected` and the event journaled.
- **REQ-HAL.PAIR.PRIN-004** A paired channel shall bind only to principal kinds `device` or `service`, never to a human or agent principal.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/pair/prin/store.json` (two active records, one revoked) and the fingerprint of the first active record, when `bind` runs, then it returns `Principal { kind: device, id: <endpoint ID> }`.
- **AC-2** (error path) Given the revoked record's fingerprint, when `bind` runs, then it fails with `principal_unpaired`.
- **AC-3** (error path) Given a bound `device` channel and a message carrying `principal: "user:admin"`, when the message is checked, then it is rejected with `principal_claim_rejected`.
- **AC-4** Given a message with no principal field on a bound channel, when checked, then it is accepted with the bound principal.
- **AC-5** (error path) Given a record whose kind is `human`, when `bind` runs, then it fails with `principal_kind_invalid`.

## Design

- **Registry commands:** none; used by the host protocol server side and the Approval Station link.
- **Data structures:** `Principal { kind: Device | Service, endpoint_id }`, `Fingerprint` (SHA-256 of the public key).
- **Algorithm:** lookup in a `BTreeMap<Fingerprint, PairingRecord>` snapshot; compare message principal if present.
- **Errors:** `principal_unpaired`, `principal_claim_rejected`, `principal_kind_invalid`.
- **Determinism:** D0 for a given store snapshot.
- **Performance budget:** < 5 µs per message [unverified until measured].
- **Crate:** `fid-hal-pair`.

## Test fixtures

Project-authored pairing-store snapshot `fixtures/hal/pair/prin/store.json` with test keys (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-pair`, implement `bind(fp: &Fingerprint, store: &PairingStoreSnapshot) -> Result<Principal, PrinError>` deriving the principal (`device` or `service`, plus endpoint ID) only from the active pairing record matching the authenticated peer-key fingerprint; fail `principal_unpaired` if none, `principal_kind_invalid` for any other kind. Implement `check_message(bound: &Principal, claimed: Option<&str>) -> Result<(), PrinError>` rejecting any differing claim with `principal_claim_rejected` (caller journals it). BTreeMap only; no panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-5 with `fixtures/hal/pair/prin/store.json`.

## Open questions

- None.

## Children

_Leaf: none._
