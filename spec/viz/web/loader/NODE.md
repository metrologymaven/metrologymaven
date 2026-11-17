---
id: VIZ.WEB.LOADER
title: Bounded package loader and integrity display
level: 3
parent: VIZ.WEB
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.WEB-005, REQ-VIZ.WEB-006]
covers: []
depends_on: [VIZ.WEB.PKG, VIZ.WEB.BUILD]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.WEB.LOADER: Bounded package loader and integrity display

## Purpose

Load ViewerPackages in the browser safely: bounded, fail-closed parsing with typed errors, lazy streaming of parts, digest checks on every part, signature status display using TRUST's verification logic, and a persistent mismatch banner that marks all values unverified when anything does not check out.

## Scope

**In:**
- Bounded manifest and part parsing with typed errors
- Lazy part streaming (range reads)
- Per-part digest verification
- Signature status display (signed by, kernel manifest, journal head) from TRUST's verifier
- Mismatch banner and "unverified" marking

**Out** (and which node owns it instead):
- Signature verification logic (`TRUST.VERIFY`)
- Format (`VIZ.WEB.PKG`)

## Requirements

- **REQ-VIZ.WEB.LOADER-001** If a part's digest or the package signature does not verify, then the viewer shall show a persistent mismatch banner and shall mark every value from that package unverified.
- **REQ-VIZ.WEB.LOADER-002** The loader shall reject a malformed or over-limit package with a typed error and without crashing.
- **REQ-VIZ.WEB.LOADER-003** The loader shall verify each part's digest before any of its content is displayed.
- **REQ-VIZ.WEB.LOADER-004** The loader shall be fuzzed in CI with no crash or hang over a fixed corpus and time budget.

## Interfaces

- **Offers:** loaded package model for `VIZ.WEB.LITEUI`.
- **Consumes:** `VIZ.WEB.PKG`; TRUST verification library (wasm build).

## Decomposition hints

PARSE, STREAM, DIGEST, SIGSTAT, BANNER, FUZZ.

## Open questions

- The TRUST verifier's wasm build is TRUST.VERIFY.WEB's; the loader consumes the library, not the page.

## Children

_None yet: status `open`._
