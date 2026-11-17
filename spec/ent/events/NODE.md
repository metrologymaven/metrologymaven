---
id: ENT.EVENTS
title: Outbound event API: webhooks and published event schema
level: 2
parent: ENT
kind: subsystem
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ENT-004]
covers: []
depends_on: [ENT.SRV, ENT.IAM, TRUST.SIGN.DSSE, AUTO.SURF.ENV, CORE.REG.VER]
cli: [fid integrate *-webhook, fid integrate *-event-schema, fid integrate *-delivery]
mcp: [integrate_*_webhook, integrate_*_event_schema, integrate_*_delivery]
journal: [integrate.*_webhook@*, integrate.*_delivery@*]
updated: 2026-11-16
---

# ENT.EVENTS: Outbound event API: webhooks and published event schema

## Purpose

Lets other systems react to what happens on the server without polling: result release, nonconformance, program release and similar events are sent as versioned, signed payloads to subscribed HTTPS endpoints. The schema is published so third parties can build receivers. It is a separate unit because it is the one outbound channel every connector and external system shares.

## Scope

**In:**
- Event catalogue: event types (at least result released, nonconformance raised, program released), each with a versioned JSON Schema
- Mapping server journal entries to events
- Webhook subscriptions: endpoint, event types, site filter, secret
- Delivery: at-least-once with retries and backoff, ordering per subscription, delivery log, replay of a failed delivery
- Payload authentication (signature over the payload) and event IDs for receiver de-duplication
- Schema versioning and deprecation tied to registry versioning

**Out** (and which node owns it instead):
- Connectors that consume events and talk to MES, QMS, ERP, PLM and SPC (`ENT.CONNECT`)
- The server journal and catalog (`ENT.SRV`)
- Service credentials and roles (`ENT.IAM`)
- Desktop-side exports to SPC files (`RPT.OUTPUT.SPC`)

## Requirements

- **REQ-ENT.EVENTS-001** The project shall publish a JSON Schema for every event type, with a version, and every delivered payload shall validate against the schema version it names.
- **REQ-ENT.EVENTS-002** When a result is released, a nonconformance is recorded or a program is released on the server, the server shall emit exactly one event of the matching type carrying a stable event ID and the source journal entry hash.
- **REQ-ENT.EVENTS-003** The server shall deliver each event at least once to every matching subscription, retrying failed deliveries with backoff for at least 24 h [unverified] before marking them failed.
- **REQ-ENT.EVENTS-004** Every delivered payload shall carry a signature that a receiver can check with the subscription secret or a published server key.
- **REQ-ENT.EVENTS-005** Creating, changing or deleting a webhook subscription shall be an A0 command and shall never be projected to MCP; listing subscriptions and reading the delivery log shall be R0.
- **REQ-ENT.EVENTS-006** Each delivery shall be recorded as an X1 egress in the Access Ledger with destination, event ID and payload digest.
- **REQ-ENT.EVENTS-007** Event payloads shall contain no content typed as untrusted text unless it is marked as untrusted in the schema.
- **REQ-ENT.EVENTS-008** A breaking change to an event schema shall publish a new major version and keep delivering the previous major version to existing subscriptions for at least one server release.

## Interfaces

- **Offers:** event catalogue and schemas; `integrate.*_webhook`, `integrate.*_delivery` commands; delivery hook used by `ENT.CONNECT`.
- **Consumes:** server journal tail and catalog (`ENT.SRV`), service credentials (`ENT.IAM`), payload signing primitives (`TRUST.SIGN.DSSE`), shared envelope and error codes (`AUTO.SURF.ENV`), version rules (`CORE.REG.VER`).

## Decomposition hints

SCHEMA (catalogue and JSON Schemas), MAP (journal entry to event), SUB (subscriptions), DELIVER (queue, retries, ordering, log), AUTH (payload signatures).

## Open questions

- Whether payload signatures use DSSE with a server key or an HMAC with a per-subscription secret, or both, is left to the leaf.
- "Nonconformance" as a server event assumes nonconformance records exist on the server; QMS-side nonconformance workflows stay out of scope.

## Children

_None yet: status `open`._
