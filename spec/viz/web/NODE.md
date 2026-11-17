---
id: VIZ.WEB
title: Web viewer build and viewer package
level: 2
parent: VIZ
kind: subsystem
status: open
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

## Open questions

- Phase: the concept places the browser viewer in P1b while the parent is P1a; tagged P1b here [unverified].
- Whether readouts in the browser use a kernel-formatter port compiled to WASM (formatting only, no evaluation) needs confirmation with KRN.

## Children

_None yet: status `open`._
