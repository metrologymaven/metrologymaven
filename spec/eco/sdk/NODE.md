---
id: ECO.SDK
title: Plugin system and SDK
level: 2
parent: ECO
kind: subsystem
status: open
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-ECO-002]
covers: []
depends_on: [CORE, AUTO, TRUST, ECO.REPO]
cli: [fid plugin *]
mcp: [plugin_*]
journal: [plugin.*]
updated: 2026-11-16
---

# ECO.SDK: Plugin system and SDK

## Purpose

The public way for third parties to extend the platform without forking: the versioned SDK crate(s), the extension points a plugin may register against, the plugin package format and manifest, install and enable lifecycle, signing and trust, isolation of plugin code, and the compatibility policy between SDK and host versions. Industry packs (IND) and community instrument drivers are the first consumers.

## Scope

**In:**
- SDK crates and their semantic-versioning and deprecation policy
- Extension points: register commands, element types, checks, algorithms, importers and exporters, report templates, panels
- Plugin package format and manifest (identity, version, SDK range, declared capabilities, licence SPDX ID)
- Install, enable, disable, update and uninstall lifecycle as registry commands
- Plugin signing and trust policy (who may sign, what the user is told for unsigned plugins)
- Isolation and capability grants for plugin code
- Offline install from a local file or mirror (no account, no store needed)
- A plugin template and an example plugin

**Out** (and which node owns it instead):
- The pack contract and the reference industry packs (owned by `IND`)
- Driver host protocol and driver tiers (owned by `HAL`)
- Generated CLI, MCP and Python surfaces for plugin commands (owned by `AUTO`)
- The signing primitives and key storage (owned by `TRUST`)
- A hosted marketplace (P4; to be opened as its own node or decision when scheduled)
- Building and publishing the SDK crates in releases (owned by `ECO.REL`)

## Requirements

- **REQ-ECO.SDK-001** The SDK shall follow semantic versioning, and a breaking change to any extension point shall require a new major version plus at least one minor release in which the old form is marked deprecated.
- **REQ-ECO.SDK-002** When a plugin's declared SDK range does not include the host's SDK version, the host shall refuse to enable it and report the required and available versions.
- **REQ-ECO.SDK-003** A plugin shall register commands, element types, checks, algorithms, format handlers, report templates and panels only through public extension points, and every registered command shall appear in the registry manifest with a plugin-namespaced ID.
- **REQ-ECO.SDK-004** If plugin code requests a capability (file root, network, device, another plugin) not declared in its manifest and granted by the user, then the host shall deny the request and journal the denial.
- **REQ-ECO.SDK-005** If a plugin faults, hangs beyond its declared time budget or returns malformed data, then the host shall stop that call, keep the session running and report the plugin identity and fault.
- **REQ-ECO.SDK-006** Installing, enabling, updating and uninstalling a plugin shall be A0 registry commands, so agents can list plugins but cannot change which plugin code runs.
- **REQ-ECO.SDK-007** When a plugin package is unsigned or its signature does not verify against a trusted key, the host shall show that state before install and shall require an explicit user confirmation.
- **REQ-ECO.SDK-008** Every value of record produced by plugin code shall carry an algorithm ID that names the plugin identity and version, and shall replay like a built-in result.
- **REQ-ECO.SDK-009** Plugins shall install and run fully offline from a local package file, with no account.

## Interfaces

- **Offers:** SDK crates; extension-point catalogue; plugin manifest schema; `fid plugin list|inspect|install|enable|disable|uninstall` (R0 reads exposed on MCP as `plugin_*`; mutating commands A0, never on MCP); `plugin.*` journal events.
- **Consumes:** CORE registry, bus and journal; AUTO generators; TRUST signature verification; ECO.REPO layering rules (the SDK sits above the kernel and below apps).

## Decomposition hints

API (SDK crates and versioning policy), EXT (extension-point catalogue), PKG (package format and manifest), LIFE (install and enable commands), ISO (isolation and capability grants), SIGN (trust policy), TPL (template and example plugin).

## Open questions

- Isolation mechanism (WASM components, out-of-process host, or both by tier) is a design choice for the ISO child; it must keep the KNP-1 numeric profile for values of record [unverified feasibility for WASM floating point].
- Whether plugins may be native in-process code at all may need a decision once threat modelling is done.
- D-001 sets which plugin licences the project may distribute or list.

## Children

_None yet: status `open`._
