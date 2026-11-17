---
id: VIZ.VIEW.NAMED.EXPORT
title: NamedView package format for reports and web packages
level: 4
parent: VIZ.VIEW.NAMED
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW.NAMED-002]
covers: []
depends_on: [VIZ.VIEW.NAMED.STORE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.VIEW.NAMED.EXPORT: NamedView package format for reports and web packages

## Purpose

Publish NamedViews as a versioned, self-describing file (`namedviews.json`, format `fid.namedviews` version 1) that RPT and VIZ.WEB packages embed, so a report or browser package can restore views without the project. The file keeps the canonical ViewState bytes unchanged, so a restore from a package yields the same digest as a restore from the project.

## Scope

**In:** the file schema `schemas/namedviews-1.json`; writer and bounded reader; digest checks on read.
**Out:** package containers (`VIZ.WEB.PKG`, `RPT.OUTPUT`); restore semantics (`VIZ.VIEW.NAMED.RESTORE`).

## Requirements

- **REQ-VIZ.VIEW.NAMED.EXPORT-001** The writer shall emit RFC 8785 JSON with format `fid.namedviews`, version 1, and per view its ID, name, digest and the canonical ViewState or layout embedded verbatim.
- **REQ-VIZ.VIEW.NAMED.EXPORT-002** If an embedded state's SHA-256 does not equal its recorded digest, then the reader shall reject that view with `DigestMismatch` and keep the others.
- **REQ-VIZ.VIEW.NAMED.EXPORT-003** The reader shall reject files above 64 MiB or with more than 10 000 views with `LimitExceeded`.
- **REQ-VIZ.VIEW.NAMED.EXPORT-004** A view read from the file shall re-serialise to the same canonical bytes and digest as in the project.

## Acceptance criteria

- **AC-1** Given `fixtures/viz/named-views`, when exported and read back, then every view's bytes and digest equal the project's.
- **AC-2** (error path) Given a file with one tampered ViewState, then that view is rejected with `DigestMismatch` and four are returned.
- **AC-3** Given the export, then it validates against `schemas/namedviews-1.json`.

## Design

- **Registry command(s):** none; library used by RPT and VIZ.WEB exporters (their commands stage the file).
- **Data structures:** `NamedViewsFile { format, version: 1, views: [{ id, name, digest, state }] }`.
- **Errors:** `DigestMismatch`, `LimitExceeded`, `UnsupportedVersion`, `Malformed`.
- **Determinism:** D0.
- **Performance budget:** 1 000 views under 50 ms.
- **Crate:** `fid-view` (module `named::export`).

## Test fixtures

`fixtures/viz/named-views` (shared with RESTORE).

## Build prompt

> In crate `fid-view`, add `named::export`: write `namedviews.json` (RFC 8785, format `fid.namedviews`, version 1, views with ID, name, digest and the canonical state embedded verbatim) and publish `schemas/namedviews-1.json`; the bounded reader (≤ 64 MiB, ≤ 10 000 views) rejects views whose SHA-256 differs from the recorded digest (`DigestMismatch`) and keeps the rest. Tests AC-1..AC-3 with `fixtures/viz/named-views`. Rules: CONSTITUTION §8 (bounded, fail-closed parsers).

## Open questions

- None.

## Children

_Leaf: none._
