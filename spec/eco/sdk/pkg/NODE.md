---
id: ECO.SDK.PKG
title: Plugin package format and manifest
level: 3
parent: ECO.SDK
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.SDK-009]
covers: []
depends_on: [ECO.SDK.API]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.SDK.PKG: Plugin package format and manifest

## Purpose

The single file a plugin ships as: its archive layout, the manifest (identity, version, SDK range, profile, declared registrations, requested capabilities, licence, entry points, content digests), extension sections that other domains add for their plugin kinds, and the bounded, fail-closed reader that the host uses before anything is installed.

## Scope

**In:**
- Archive format and layout; content digests per file
- Manifest schema with typed extension sections (for example a pack section owned by IND.CONTRACT, a driver section owned by HAL.HOST)
- Bounded reader: size, entry-count and path limits; no path traversal; no symlinks
- Local package sources: a file path chosen by the user, or a local or LAN mirror directory with an index

**Out** (and which node owns it instead):
- Signature format and trust decision (owned by `ECO.SDK.SIGN`, using `TRUST` primitives)
- Installing and enabling (owned by `ECO.SDK.LIFE`)
- Contents of the extension sections (owned by `IND.CONTRACT` and `HAL.HOST`)

## Requirements

- **REQ-ECO.SDK.PKG-001** A plugin manifest shall state plugin ID, namespace, version (semver), SDK range, extension-point profile, every registration it will make, every capability it requests, licence as an SPDX expression, entry points and a SHA-256 for every file in the package.
- **REQ-ECO.SDK.PKG-002** If a package exceeds the configured size or entry-count limits, contains an absolute path, a `..` segment, a symlink, or a file whose digest does not match the manifest, then the reader shall reject the whole package and install nothing.
- **REQ-ECO.SDK.PKG-003** The reader shall accept unknown extension sections only if they are namespaced, and shall pass them unvalidated to the owning domain's validator by section name.
- **REQ-ECO.SDK.PKG-004** The host shall install a package from a local file or a local mirror directory with no network access and no account.
- **REQ-ECO.SDK.PKG-005** The manifest reader shall be fuzzed, and no input shall make it panic or exceed its memory limit.

## Interfaces

- **Offers:** package format specification; manifest schema; `read_package` (bounded); mirror index format.
- **Consumes:** ECO.SDK.API version types; IO safe-read conventions for bounded parsing.

## Decomposition hints

FORMAT (archive and layout), MANIFEST (schema and extension sections), READER (bounded reader and fuzzing), MIRROR (local mirror index).

## Open questions

- Whether the manifest is TOML or JSON, and whether the archive is zip or tar, are leaf choices; both must have a bounded reader on the licence allowlist [unverified].

## Children

_None yet: status `open`._
