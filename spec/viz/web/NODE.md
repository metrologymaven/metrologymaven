---
id: VIZ.WEB
title: Web viewer build and viewer package
level: 2
parent: VIZ
kind: subsystem
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ-009]
covers: []
depends_on: [VIZ.RENDER, VIZ.VIEW, VIZ.OVERLAY, TRUST]
cli: [fid view export-package, fid view inspect-package]
mcp: [view_export_package, view_inspect_package]
journal: [view.export_package@*]
updated: 2026-11-16
---

# VIZ.WEB: Web viewer build and viewer package

## Purpose

The read-only browser viewer built to WASM from the same render, scene and overlay crates as the desktop, with WebGPU and a WebGL2 fallback, fed by a content-addressed, signed **ViewerPackage** (tessellations, cloud LOD, kernel-binned deviation display data, legends, NamedViews, characteristics). It contains no kernel and no CAD kernel, so it cannot produce a number the kernel did not. It is separate because its build target, package format and lack of a kernel give it constraints the desktop host does not have.

## Scope

**In:**
- WASM build of the shared render, scene and overlay crates
- Backend fallback chain in the browser (WebGPU, then WebGL2, then server-rendered stills from VIZ.RENDER headless)
- ViewerPackage format: parts, digests, kernel manifest reference, journal head, explicit redactions
- Export command that assembles a package from the working document (signing delegated to TRUST)
- Package loader with bounded, fail-closed parsing and in-browser signature check display
- Minimal read-only UI: navigation, NamedViews, legends, readouts from precomputed f64 package data
- Embedding hooks for reports and for a later MCP App host

**Out** (and which node owns it instead):
- Signature format and verification logic (`TRUST`)
- Supported browsers and versions (`PLAT`)
- The MCP App protocol and tools (`AUTO`)
- Website hosting of the viewer (`OUTR`)

## Requirements

- **REQ-VIZ.WEB-001** The web viewer shall be built from the same render, scene and overlay crates as the desktop viewer, with no browser-only fork of rendering logic.
- **REQ-VIZ.WEB-002** The web viewer build shall contain no metrology kernel and no CAD kernel, and CI shall fail if either is linked.
- **REQ-VIZ.WEB-003** When WebGPU is unavailable, the web viewer shall fall back to WebGL2 and shall display the active capability tier.
- **REQ-VIZ.WEB-004** Every number the web viewer shows shall come from f64 values stored in the ViewerPackage and formatted by the shared kernel formatter port, never from f32 display data.
- **REQ-VIZ.WEB-005** If a ViewerPackage part's digest or signature does not verify, then the viewer shall show a persistent mismatch banner and shall mark all values from that package unverified.
- **REQ-VIZ.WEB-006** The package loader shall reject a malformed or over-limit package without crashing, with a typed error.
- **REQ-VIZ.WEB-007** A NamedView restored in the web viewer from a package shall match the desktop headless render of the same view within the VIZ.RENDER golden-image tolerance.
- **REQ-VIZ.WEB-008** Every redaction applied at export shall be listed in the package and shown to the viewer user.
- **REQ-VIZ.WEB-009** The web viewer shall render a package of 10⁷ points at ≥ 30 fps on the PLAT reference browser profile [unverified].

## Interfaces

- **Offers:** the WASM viewer bundle; `view.export_package@1`; `view.inspect_package@1` (R0); the ViewerPackage format (consumed by RPT, TRUST verifier, OUTR).
- **Consumes:** VIZ.RENDER, VIZ.VIEW, VIZ.OVERLAY crates; TRUST signing and verification; IO for point-cloud LOD encodings.

## Decomposition hints

BUILD (WASM target and CI guards), FALLBACK, PKG (ViewerPackage format), EXPORT, LOADER, LITEUI.

## Split rationale

The dividing principle is **the package's path from desktop to browser**: EXPORT writes it, PKG defines it, LOADER reads it safely, LITEUI shows it, while BUILD and FALLBACK make the same crates run in the browser at all. Each child owns one of the node's guarantees: no kernel (BUILD), graceful degradation (FALLBACK), every number traceable to f64 package data (PKG, LITEUI), fail-closed loading and visible integrity (LOADER), visible redactions (EXPORT, PKG).

Alternatives considered:
- **PKG and EXPORT together.** Rejected: the format is consumed by RPT, TRUST and OUTR independently of the export command.
- **LOADER inside LITEUI.** Rejected: the loader is the untrusted-input boundary and needs its own fuzzing and limits.

## Open questions

- Phase: the concept places the browser viewer in P1b while the parent is P1a; tagged P1b here [unverified].
- Whether readouts in the browser use a kernel-formatter port compiled to WASM (formatting only, no evaluation) needs confirmation with KRN.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.WEB.BUILD | WASM build and no-kernel CI guards (leaf) | 001 002 | none |
| VIZ.WEB.FALLBACK | Browser backend fallback chain and budgets | 003 009 | none |
| VIZ.WEB.PKG | ViewerPackage format | 004 008 | none |
| VIZ.WEB.EXPORT | ViewerPackage export command | 008 | none |
| VIZ.WEB.LOADER | Bounded package loader and integrity display | 005 006 | none |
| VIZ.WEB.LITEUI | Read-only web viewer UI | 004 007 | none |
