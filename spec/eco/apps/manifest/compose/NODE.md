---
id: ECO.APPS.MANIFEST.COMPOSE
title: Order-independent manifest composition
level: 4
parent: ECO.APPS.MANIFEST
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS.MANIFEST-006]
covers: []
depends_on: [ECO.APPS.MANIFEST.SCHEMA]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.APPS.MANIFEST.COMPOSE: Order-independent manifest composition

## Purpose

The hook for a future suite host (D-004 option C) that runs several apps in one window: a pure function that combines two or more validated manifests into one composed selection, with the same result for any input order, and that reports conflicting defaults or layouts instead of picking a winner. Nothing ships a suite yet; this leaf keeps the rule fixed so per-app manifests are written in a composable way from the start.

## Scope

**In:**
- `compose(&[AppManifest]) -> Result<Composed, Vec<Conflict>>`
- Union and conflict rules for every manifest key
- A property test that composition is order-independent

**Out:**
- The suite host itself (later; not scheduled)
- Single-manifest validation (owned by `ECO.APPS.MANIFEST.SCHEMA`)

## Requirements

- **REQ-ECO.APPS.MANIFEST.COMPOSE-001** The composed selection shall enable the union of the inputs' namespaces, panels and templates, and the union of their `forbidden_classes`.
- **REQ-ECO.APPS.MANIFEST.COMPOSE-002** If the union of `forbidden_classes` forbids a class held by a command in the union of namespaces, then composition shall fail with conflict `COMPOSE-FORBIDDEN` naming the command.
- **REQ-ECO.APPS.MANIFEST.COMPOSE-003** If two inputs set the same `defaults` key to different values, or name different `layout` values, then composition shall fail with `COMPOSE-DEFAULT` or `COMPOSE-LAYOUT` listing every conflicting app and value.
- **REQ-ECO.APPS.MANIFEST.COMPOSE-004** The composed selection shall keep each input's `id`, `status`, `status_scope` and `known_gaps` per app, not merge them.
- **REQ-ECO.APPS.MANIFEST.COMPOSE-005** For every permutation of the same inputs, composition shall return equal results, including the order of conflicts.

## Acceptance criteria

- **AC-1** Given `fixtures/manifest/compose/report.toml` and `inspect.toml` with disjoint defaults and the same layout, when composed in both orders, then the results are equal and enable the union of namespaces.
- **AC-2** (error path) Given `fixtures/manifest/compose/clash-a.toml` and `clash-b.toml` setting `units.length` to `mm` and `in`, then composition fails with one `COMPOSE-DEFAULT units.length` conflict listing both apps.
- **AC-3** (error path) Given a Report manifest forbidding M1 and a Probe manifest enabling `cmm` with the fixture registry, then composition fails with `COMPOSE-FORBIDDEN`.
- **AC-4** (property) For 200 seeded random sets of 2 to 4 fixture manifests, every permutation yields byte-identical serialised output.

## Design

- **Registry command(s):** none; library function only.
- **Data structures:** `Composed { apps: BTreeMap<AppId, AppStatus>, namespaces, panels (sorted, deduplicated), templates, forbidden_classes, defaults: BTreeMap, layout }`; `Conflict { code, key, values: BTreeMap<AppId, String> }`.
- **Algorithm:** sort inputs by `id`; fold with set unions; collect conflicts in BTreeMaps; check forbidden classes against the registry after the union.
- **Errors:** `COMPOSE-DEFAULT`, `COMPOSE-LAYOUT`, `COMPOSE-FORBIDDEN`, `COMPOSE-DUP-ID` (same app twice).
- **Determinism:** D0.
- **Performance budget:** negligible (under 1 ms for 10 manifests) [unverified until measured].
- **Crate:** `fid-app-manifest` (module `compose`).

## Test fixtures

Fixture manifests under `fixtures/manifest/compose/` and a seeded generator for the property test (MIT OR Apache-2.0).

## Build prompt

> In crate `fid-app-manifest` (implementation repository), add module `compose` with `compose(manifests: &[AppManifest], registry: &RegistryView) -> Result<Composed, Vec<Conflict>>`. Sort inputs by id first; union namespaces, panels, templates and forbidden classes; keep each app's id, status, status_scope and known_gaps separately; conflicts: `COMPOSE-DEFAULT` (same key, different values), `COMPOSE-LAYOUT` (different layouts), `COMPOSE-FORBIDDEN` (a command in the namespace union has a class in the forbidden union), `COMPOSE-DUP-ID`. Results and conflict lists must be identical for every permutation of the input. Tests for AC-1..AC-4 of spec node ECO.APPS.MANIFEST.COMPOSE, including a seeded property test over permutations. BTreeMap/BTreeSet only; no `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- Whether a suite host may resolve a layout conflict by offering a choice to the user is for the suite host node when it is scheduled.

## Children

_Leaf: none._
