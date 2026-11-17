---
id: HAL.HOST.PKG
title: Driver package format, install and allowlist
level: 3
parent: HAL.HOST
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.HOST-001, REQ-HAL.HOST-006]
covers: []
depends_on: [ECO.SDK, CORE.REG]
cli: [fid device install-driver, fid device remove-driver, fid device list-drivers, fid device show-driver]
mcp: [device_list_drivers, device_show_driver]
journal: [device.install_driver@1, device.remove_driver@1, device.list_drivers@1, device.show_driver@1]
updated: 2026-11-16
---

# HAL.HOST.PKG: Driver package format, install and allowlist

## Purpose

What a driver is on disk and how it gets onto a machine: the package manifest (tier, capabilities, tested firmware range, binaries and their digests), the admin-only install, upgrade and removal commands, and the allowlist that the supervisor and the WASM loader check. It is separate because packaging and installation are an A0 trust boundary, distinct from running drivers.

## Scope

**In:**
- Driver package manifest: driver ID, version, tier (1, 2 or 3), capabilities and versions, tested firmware range, binary list with SHA-256 digests, target OS and architecture (including 32-bit hosts), signer, conformance-report digests
- `device.install_driver@1` and `device.remove_driver@1` (A0; never on MCP); upgrade as install of a newer version
- `device.list_drivers@1` and `device.show_driver@1` (R0)
- The installed allowlist store (package ID, version, binary digests)
- Rules for NDA drivers: vendor-built packages preferred; NDA packages are never linked into project binaries

**Out** (and which node owns it instead):
- Generic plugin packaging, marketplace and signing infrastructure (owned by `ECO`)
- Launch-time checks (owned by `HAL.HOST.SUPER` and `HAL.HOST.WASM`)
- Catalog status of devices (owned by `HAL.DRV.CAT`)

## Requirements

- **REQ-HAL.HOST.PKG-001** Every driver package manifest shall declare its tier, driver ID, version, capabilities with versions, tested firmware range and the SHA-256 digest of every binary it contains.
- **REQ-HAL.HOST.PKG-002** Installing, upgrading or removing a driver package shall be class A0, executable only by an administrator principal outside MCP, and shall be journaled with the package digest.
- **REQ-HAL.HOST.PKG-003** If a package's signature is invalid or a contained binary's digest differs from its manifest, then install shall fail with `package_invalid` and change nothing.
- **REQ-HAL.HOST.PKG-004** `device.list_drivers@1` and `device.show_driver@1` shall be R0 and shall list installed and compiled-in (Tier-1) drivers with tier, version, capabilities and allowlisted digests.
- **REQ-HAL.HOST.PKG-005** Removing a driver package shall remove its digests from the allowlist and shall be refused with `driver_in_use` while a session uses it.

## Interfaces

- **Offers:** manifest schema; the four commands; allowlist store read API for `HAL.HOST.SUPER` and `HAL.HOST.WASM`.
- **Consumes:** `ECO.SDK` plugin packaging and signature verification; `CORE.REG` command registration.

## Decomposition hints

MANIF (manifest schema and validator; likely a leaf), INSTALL (A0 install, upgrade, remove), ALLOW (allowlist store), LIST (R0 commands; likely a leaf).

## Open questions

- Whether driver packages are a kind of ECO plugin package or a separate format depends on how `ECO.SDK` splits; this node assumes a driver package is an ECO plugin with a HAL-specific manifest section.

## Children

_None yet: status `open`._
