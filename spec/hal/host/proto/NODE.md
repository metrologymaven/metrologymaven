---
id: HAL.HOST.PROTO
title: Driver host protocol (control and data channels)
level: 3
parent: HAL.HOST
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.HOST-001, REQ-HAL.HOST-007]
covers: []
depends_on: [HAL.IDL.GEN, HAL.ACQ.INGEST, HAL.PAIR.PRIN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.HOST.PROTO: Driver host protocol (control and data channels)

## Purpose

The wire protocol between the runtime and a Tier-2 driver host, local or remote: a control channel with idempotency keys, deadlines and trace-ID propagation; a separate data channel carrying batches under credit-based flow control; and heartbeats. It is separate because it is the published contract that third-party and vendor-built hosts implement, independent of how the runtime supervises them.

## Scope

**In:**
- Control channel: request and response framing, idempotency key, deadline, trace ID in metadata, progress handles for long operations
- Data channel: batch stream with credits, sequence numbers and end-of-stream summary; shared memory for same-host, a network stream for remote hosts
- Heartbeat messages and their timing
- Transport security for remote hosts: mutually authenticated channel using keys pinned by pairing
- Host obligations: accept control only from the runtime over its paired channel, never compute metrology results, never cache data outside batches
- Protocol versioning and a protocol conformance profile

**Out** (and which node owns it instead):
- Launching and supervising hosts (owned by `HAL.HOST.SUPER`)
- Pairing and key pinning (owned by `HAL.PAIR`)
- Capability payload schemas (owned by `HAL.IDL`)

## Requirements

- **REQ-HAL.HOST.PROTO-001** Every control message shall carry an idempotency key, a deadline and the trace ID of the journaled command that caused it.
- **REQ-HAL.HOST.PROTO-002** The data channel shall be separate from the control channel, so that a saturated data stream cannot delay a control response or a heartbeat.
- **REQ-HAL.HOST.PROTO-003** A host shall send a heartbeat at a fixed interval of 1 s [unverified], and the protocol shall define three consecutive missed heartbeats as host loss.
- **REQ-HAL.HOST.PROTO-004** A remote host shall accept connections only over a mutually authenticated channel whose peer key matches the key pinned at pairing, and shall reject every other peer.
- **REQ-HAL.HOST.PROTO-005** The protocol shall define no message by which a host returns a computed metrology result; device-computed quantities shall travel only as fields declared for `ExternalClaim` classification.
- **REQ-HAL.HOST.PROTO-006** The protocol shall carry its own semantic version, negotiated at connect, independently of capability versions.

## Interfaces

- **Offers:** protocol specification and conformance profile; Rust client side used by the runtime; server skeleton consumed by `HAL.HOST.SDK`.
- **Consumes:** `HAL.IDL.GEN` stubs; `HAL.ACQ.INGEST` `BatchSink`; `HAL.PAIR.PRIN` channel principal binding.

## Decomposition hints

CTRL (control framing, idempotency, deadlines, progress), DATA (batch stream, credits, shared memory), HB (heartbeats), SEC (remote transport security), CONF (protocol conformance profile).

## Open questions

- Concept doc 08 proposes gRPC with Arrow IPC streams (Arrow Flight pattern); the transport is an implementation choice for the CTRL and DATA leaves.

## Children

_None yet: status `open`._
