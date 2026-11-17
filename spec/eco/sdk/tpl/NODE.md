---
id: ECO.SDK.TPL
title: Plugin template and example plugin
level: 3
parent: ECO.SDK
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.SDK-003]
covers: []
depends_on: [ECO.SDK.EXT, ECO.SDK.PKG, ECO.SDK.LIFE]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.SDK.TPL: Plugin template and example plugin

## Purpose

A ready-to-copy plugin template and one complete example plugin that exercises the main extension points end to end (a command, an element type, an algorithm with a value of record, a report template), built, packaged, signed with a test key, installed offline and replayed in CI. It is the first thing a third-party developer reads and the integration test that keeps the SDK honest.

## Scope

**In:**
- Plugin template (project layout, manifest skeleton, build and package script, test harness)
- Example plugin with a trivial but real algorithm and its replay test
- CI job: build, package, sign with a test key, install into a fresh host, run, replay, uninstall

**Out** (and which node owns it instead):
- Reference industry packs (owned by `IND`)
- Developer documentation pages (owned by `OUTR`)

## Requirements

- **REQ-ECO.SDK.TPL-001** A plugin created from the template without changes shall build, package and pass its own tests against the current SDK.
- **REQ-ECO.SDK.TPL-002** The example plugin shall register at least one command, one element type, one algorithm producing a value of record and one report template, each through the public extension points only.
- **REQ-ECO.SDK.TPL-003** On every SDK change, CI shall build the example plugin, install it offline into a fresh host, run its command, replay the journal with outcome `IDENTICAL`, and uninstall it.
- **REQ-ECO.SDK.TPL-004** The template and the example shall be licensed so third parties may copy them without obligations beyond the project licence (D-001).

## Interfaces

- **Offers:** template repository or directory; example plugin; SDK integration job.
- **Consumes:** ECO.SDK.EXT, ECO.SDK.PKG, ECO.SDK.LIFE; a test signing key (ECO.SDK.SIGN).

## Decomposition hints

TEMPLATE, EXAMPLE, ITEST (integration job).

## Open questions

- Whether the template is a separate repository (instantiated like ECO.REPO.TPL) or a directory in the monorepo is a maintainer call tied to the repository set.

## Children

_None yet: status `open`._
