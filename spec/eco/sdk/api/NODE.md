---
id: ECO.SDK.API
title: SDK crates, versioning and compatibility policy
level: 3
parent: ECO.SDK
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.SDK-001, REQ-ECO.SDK-002]
covers: []
depends_on: [ECO.REPO.LAYERS]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.SDK.API: SDK crates, versioning and compatibility policy

## Purpose

The public SDK crate(s) that plugins compile against, the semantic-versioning and deprecation policy that governs them, the automated public-API diff that catches breaking changes, and the compatibility rule that decides whether a plugin built for one SDK range may be enabled on a given host.

## Scope

**In:**
- SDK crate set and its place in the layer map (above the kernel, below apps)
- Semver and deprecation policy text, and the CI gate that enforces it (public-API diff against the last release)
- The host's SDK version and the plugin's declared SDK range; the compatibility evaluation used by LIFE
- Supported-version window (how many SDK majors a host accepts)

**Out** (and which node owns it instead):
- What the extension points are (owned by `ECO.SDK.EXT`)
- Package and manifest format (owned by `ECO.SDK.PKG`)
- Publishing SDK crates in releases (owned by `ECO.REL`)

## Requirements

- **REQ-ECO.SDK.API-001** If a change to an SDK crate's public API would break code compiled against the previous release under semantic-versioning rules, then the API gate shall fail unless the SDK major version is incremented.
- **REQ-ECO.SDK.API-002** An SDK item shall be removed or changed incompatibly only in a major release, and only after at least one minor release in which it was marked deprecated with a replacement named.
- **REQ-ECO.SDK.API-003** When asked whether a plugin's declared SDK range admits the host's SDK version, the compatibility function shall return yes or no and, for no, both the required range and the available version.
- **REQ-ECO.SDK.API-004** The SDK crates shall depend only on crates in layers at or below the SDK's layer and shall expose no kernel internals beyond the documented extension-point types.
- **REQ-ECO.SDK.API-005** The project shall publish, per SDK major version, the host versions that support it and the date support ends.

## Interfaces

- **Offers:** `fid-sdk` crate(s); compatibility function; API gate; support-window table.
- **Consumes:** ECO.REPO.LAYERS layer map; public-API diff tooling [unverified tool choice].

## Decomposition hints

CRATES (crate set and layering), POLICY (semver and deprecation text), GATE (public-API diff in CI), COMPAT (range evaluation and support window).

## Open questions

- Whether plugins are WASM components or native code (ECO.SDK.ISO) changes what "the SDK" is: a Rust crate set, a WIT-style interface, or both. API must follow ISO's outcome.

## Children

_None yet: status `open`._
