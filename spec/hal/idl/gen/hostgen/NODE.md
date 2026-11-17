---
id: HAL.IDL.GEN.HOSTGEN
title: Host stub generators (C#/.NET, C++, Python) and WIT for Tier-3 packages
level: 4
parent: HAL.IDL.GEN
kind: feature
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL.GEN-002, REQ-HAL.IDL.GEN-003]
covers: []
depends_on: []
uses_artifacts: [HAL.IDL.GEN.EXPORT, HAL.IDL.GEN.FMT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.GEN.HOSTGEN: Host stub generators and WIT for Tier-3 packages

## Purpose

Generates the language bindings that out-of-process driver hosts are written against: C#/.NET, C++ and Python stubs for every package, and WIT interfaces for the packages marked usable by Tier-3 (WASM) components. It is tooling: it reads only the exported `capability-package/v1` descriptions, never the Rust crates, so it sits in the Tooling layer (CONSTITUTION §2.1).

## Scope

**In:**
- One generator per target language, reading descriptions from `schemas/hal/packages/`
- WIT generation for packages with `tier3_usable: true`
- Committed generated stubs and their drift check in CI
- Generated stub tests (round-trip of every type through canonical JSON in each language)

**Out:**
- Host templates and the host SDK around the stubs (owned by `HAL.HOST.SDK`)
- WASM runtime (owned by `HAL.HOST.WASM`)
- Rust types and JSON Schemas (owned by `HAL.IDL.GEN.EXPORT`)

## Requirements

- **REQ-HAL.IDL.GEN.HOSTGEN-001** For every package description, the generators shall emit C#/.NET, C++ and Python stubs whose types round-trip every fixture of that package to byte-identical canonical JSON.
- **REQ-HAL.IDL.GEN.HOSTGEN-002** The WIT generator shall emit an interface only for packages with `tier3_usable: true`, and shall fail with `wit_unsupported_type` naming the type if a marked package uses a construct WIT cannot express.
- **REQ-HAL.IDL.GEN.HOSTGEN-003** When CI runs, the generators shall regenerate all stubs and fail if any committed stub differs.
- **REQ-HAL.IDL.GEN.HOSTGEN-004** The generators shall read only exported description files and shall link no product crate.

## Interfaces

- **Offers:** generated stubs under `bindings/{dotnet,cpp,python,wit}/`; generator CLIs for CI.
- **Consumes:** `capability-package/v1` descriptions (artifacts of `HAL.IDL.GEN.EXPORT`, format of `HAL.IDL.GEN.FMT`).

## Decomposition hints

CORE (shared description reader and naming rules), DOTNET, CPP, PY, WIT; each language generator is likely an S or M leaf.

## Open questions

- Target language versions (.NET LTS, C++17 or 20, Python 3.10+) are proposals [unverified]; `HAL.HOST.SDK` should confirm what vendor SDKs require.

## Children

_None yet: status `open`._
