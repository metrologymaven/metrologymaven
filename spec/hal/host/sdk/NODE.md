---
id: HAL.HOST.SDK
title: Driver SDK and generated host templates
level: 3
parent: HAL.HOST
kind: module
status: open
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-HAL.HOST-005, REQ-HAL.HOST-007]
covers: []
depends_on: [HAL.HOST.PROTO, HAL.HOST.PKG, HAL.IDL.GEN, HAL.SIM.CONF]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.HOST.SDK: Driver SDK and generated host templates

## Purpose

What a vendor or community developer starts from to write a Tier-2 driver host: generated templates in C#/.NET, C++, Python and Rust that already speak the host protocol, build batches, run the health state machine, map errors, record traces and run the conformance hook. The developer only adds calls into the vendor SDK. It is separate because it is the developer-facing product of the HAL and is versioned and documented for outsiders.

## Scope

**In:**
- Template generator per language from capability packages and the host protocol
- Batch builders with declared encodings; shared-memory writer
- Health state machine and error mapping helpers
- Trace recorder hook (feeds transcripts) and conformance hook (runs `HAL.SIM.CONF` suites)
- Packaging helper producing a driver package manifest
- Templates for 32-bit hosts with the same protocol
- Developer documentation and a worked example against a simulator

**Out** (and which node owns it instead):
- The protocol itself (owned by `HAL.HOST.PROTO`)
- Conformance suites (owned by `HAL.SIM.CONF`)
- Concrete drivers (owned by `HAL.DRV`)

## Requirements

- **REQ-HAL.HOST.SDK-001** For each supported language, the generated template, with no hand-written code beyond stubbed vendor-SDK calls, shall pass the `Device-core@1` conformance profile against a simulator.
- **REQ-HAL.HOST.SDK-002** The templates shall implement no path that accepts control from anything other than the runtime's paired channel, and shall contain no result-computation or caching helpers.
- **REQ-HAL.HOST.SDK-003** The C# and C++ templates shall ship first; the Python and Rust templates may follow in a later release [unverified ordering from concept doc 08].
- **REQ-HAL.HOST.SDK-004** Regenerating a template from a newer capability minor version shall not require changes to developer-written vendor-SDK code.
- **REQ-HAL.HOST.SDK-005** The SDK shall build a 32-bit host variant for each template language whose toolchain supports it, speaking the same protocol.

## Interfaces

- **Offers:** templates, generator, documentation, worked example.
- **Consumes:** `HAL.HOST.PROTO`, `HAL.HOST.PKG`, `HAL.IDL.GEN`, `HAL.SIM.CONF`.

## Decomposition hints

GEN (template generator core), CS (C#/.NET), CPP (C++), PY (Python), RS (Rust), DOCS (developer journey and worked example).

## Open questions

- Concept doc 08 lists Python and Rust templates as the first P2 cut if time runs short.

## Children

_None yet: status `open`._
