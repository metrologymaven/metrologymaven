---
id: HAL.HOST
title: Driver tiers, driver host protocol and driver SDK
level: 2
parent: HAL
kind: subsystem
status: split
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-HAL-001, REQ-HAL-006]
covers: []
depends_on: [HAL.IDL, HAL.ACQ, HAL.PAIR, ECO]
cli: [fid device *-driver, fid device *-drivers]
mcp: [device_list_drivers, device_show_driver]
journal: [device.*_driver]
updated: 2026-11-16
---

# HAL.HOST: Driver tiers, driver host protocol and driver SDK

## Purpose

How drivers run and how third parties write them. It defines the three driver tiers (in-process Rust, out-of-process driver hosts, sandboxed WASM readers), the host protocol between the runtime and a host, supervision and crash isolation, driver packaging and installation, and the driver SDK with generated templates. It is separate because isolation, licensing and supervision are one concern that every driver shares, regardless of device class.

## Scope

**In:**
- Tier 1: in-process Rust drivers for protocols and formats the project owns
- Tier 2: out-of-process driver hosts (local, or remote on an instrument PC over a mutually authenticated channel), including 32-bit hosts
- Tier 3: sandboxed WASM components for third-party format readers and sensor error models, with no device I/O (from P3)
- The host protocol: control channel with idempotency keys and deadlines, data channel, heartbeats, trace-ID propagation
- Host supervision: launch, signature and digest allowlist check, heartbeat, restart with back-off, `INTERRUPTED` marking on crash
- Driver packages: manifest, signature, digest, declared capabilities, tested firmware range; install, list, remove
- Driver SDK: generated host templates (C#/.NET, C++, Python, Rust), Arrow-style batch builders, health machine, error mapping, trace recorder hook, conformance hook
- Rules for NDA drivers (vendor-built hosts preferred; never linked into project binaries)

**Out** (and which node owns it instead):
- Capability contracts and stubs (owned by `HAL.IDL`)
- Stamping and ingest of what hosts send (owned by `HAL.ACQ`)
- Pairing a remote host to the engine (owned by `HAL.PAIR`)
- Concrete device drivers (owned by `HAL.DRV` and `HAL.IPP`)
- Generic plugin packaging, marketplace and signing infrastructure (owned by `ECO`)
- First-party file readers' format logic (owned by `IO`)

## Requirements

- **REQ-HAL.HOST-001** The HAL shall support three driver tiers (in-process Rust, out-of-process host, WASM component), and every driver shall declare its tier in its package manifest.
- **REQ-HAL.HOST-002** If a Tier-2 host process crashes, hangs past three missed heartbeats or exits, then the runtime shall mark its sessions `INTERRUPTED`, keep already-stamped batches valid, and continue running without engine restart.
- **REQ-HAL.HOST-003** Before launching a Tier-2 host or loading a Tier-3 component, the runtime shall verify the package signature and the binary digest against the installed allowlist, and shall refuse to start on mismatch.
- **REQ-HAL.HOST-004** A Tier-3 component shall run with no filesystem, network, clock or random-source access, within fuel and memory limits, and its output shall be a pure function of its input bytes (determinism class D0).
- **REQ-HAL.HOST-005** The driver SDK shall generate a host template for each supported language that passes the Device-base conformance profile against a simulator with no hand-written code beyond the vendor-SDK calls.
- **REQ-HAL.HOST-006** Installing, upgrading or removing a driver package shall be an admin (A0) command that is never exposed on MCP; listing and showing drivers shall be R0.
- **REQ-HAL.HOST-007** A driver host shall accept control only from the runtime over its paired channel, and shall never compute metrology results or cache data outside batches.

## Interfaces

- **Offers:** host protocol specification; driver package format; driver SDK and templates; `device.install_driver@1` (A0), `device.remove_driver@1` (A0), `device.list_drivers@1` (R0), `device.show_driver@1` (R0).
- **Consumes:** HAL.IDL stubs; HAL.ACQ ingest endpoints; HAL.PAIR pinned keys for remote hosts; ECO plugin packaging and signing.

## Decomposition hints

TIER1 (in-process driver API), PROTO (host protocol and data channel), SUPER (launch, allowlist, supervision), WASM (Tier-3 sandbox), PKG (driver package format and install), SDK (templates per language).

## Open questions

- Vendor SDK terms (cost, NDA scope, redistribution) are unverified for every portable-device vendor; the Tier-2 default is chosen to stay safe under any outcome.

## Split rationale

The split follows **the three questions every driver raises**: how it runs (TIER1 for in-process Rust, PROTO for the out-of-process contract, WASM for sandboxed components), how it is kept safe while running (SUPER), and how it is packaged and written (PKG, SDK). The protocol is separated from supervision because outside host authors implement the protocol but never the supervisor.

Alternatives considered:
- **One child per tier.** Rejected: Tier 2 alone would hold protocol, supervision, packaging and SDK, which is XL, while packaging is shared by all tiers.
- **Packaging inside ECO.** Rejected for the HAL-specific manifest fields and allowlist; generic plugin packaging stays in `ECO`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| HAL.HOST.TIER1 | In-process (Tier-1) driver API and registration — leaf | 001 | none |
| HAL.HOST.PROTO | Driver host protocol (control and data channels) | 001 007 | none |
| HAL.HOST.SUPER | Driver host launch, allowlist check and supervision — leaf | 002 003 | none |
| HAL.HOST.WASM | Tier-3 WASM component sandbox for readers and sensor models | 003 004 | none |
| HAL.HOST.PKG | Driver package format, install and allowlist | 001 006 | none |
| HAL.HOST.SDK | Driver SDK and generated host templates | 005 007 | none |
