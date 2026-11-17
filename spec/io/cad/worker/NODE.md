---
id: IO.CAD.WORKER
title: Isolated CAD kernel worker process
level: 3
parent: IO.CAD
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.CAD-010]
covers: []
depends_on: [IO.REG.LIMITS, IO.REG.ENV]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.WORKER: Isolated CAD kernel worker process

## Purpose

Runs the CAD kernel used for STEP and IGES (the concept docs name OCCT, LGPL, dynamically linked [unverified: allowlist status]) in a separate process with memory and time caps, and speaks a small typed protocol to the document process. A crash, hang or memory blow-up in native CAD code then fails one import cleanly instead of taking the app down.

## Scope

**In:**
- Worker lifecycle: spawn, handshake with version check, request, response, kill on timeout
- OS-level caps: memory (job objects on Windows, rlimits or cgroups on Linux, equivalent on macOS), wall time, no network, read access only to the handed-over source bytes
- The worker protocol: bounded, length-prefixed messages carrying B-rep, PMI and report data as typed records
- Mapping worker failures to `reader_crashed` and `limit_exceeded`
- Validation of worker output in the document process before staging

**Out:**
- What the worker computes (`IO.CAD.STEP`, `IO.CAD.PMI`, `IO.CAD.IGES`, `IO.CAD.EXPORT`)
- Plugin translators (`IO.CAD.XLATE` via `IO.REG.PLUGIN`)

## Requirements

- **REQ-IO.CAD.WORKER-001** If the worker exits abnormally, then the import shall fail with `reader_crashed` and leave the document unchanged.
- **REQ-IO.CAD.WORKER-002** If the worker exceeds its memory cap or wall-time cap, then the document process shall kill it and fail the import with `limit_exceeded` naming the cap.
- **REQ-IO.CAD.WORKER-003** The worker shall receive the source as bytes over its protocol and shall have no file-system write access and no network access.
- **REQ-IO.CAD.WORKER-004** Every message from the worker shall be bounded in size and validated against the protocol schema, and an invalid message shall fail the import with `reader_crashed`.
- **REQ-IO.CAD.WORKER-005** The worker's kernel name and version shall be recorded in the import provenance as the reader version.

## Interfaces

- **Offers:** `CadWorker` client API (`load_step`, `load_iges`, `write_step`, `write_iges`, `tessellate`) and the protocol schema.
- **Consumes:** `IO.REG.LIMITS` error taxonomy; OS process APIs.

## Decomposition hints

PROTO (protocol and validation), HOST (spawn, caps, kill per OS), SHIM (worker-side binding to the CAD kernel).

## Open questions

- Resolved by D-024 (accepted, option C): the worker runs the first-party pure-Rust STEP and IGES readers; OCCT is never part of this worker and may only appear as an optional, separately installed healer/translator plugin hosted by `IO.REG.PLUGIN`. The Purpose's mention of OCCT is superseded.
- In the WASM web build, processes cannot be spawned; whether CAD import is offered there at all is open [unverified: web build import scope].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.CAD.WORKER.PROTO | CAD worker protocol, framing and message validation (leaf) | 003 004 | none |
| IO.CAD.WORKER.HOST | Worker spawning, OS caps, kill on limit and failure mapping (leaf) | 001 002 005 | none |
| IO.CAD.WORKER.BIN | Sandboxed CAD worker executable (leaf) | 003 005 | none |
