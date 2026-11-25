---
id: UX.COMP.REF
title: Component reference renderings and visual baselines
level: 3
parent: UX.COMP
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UX.COMP-010]
covers: []
depends_on: [UX.COMP.SCHEMA, UX.COMP.DATA, UX.COMP.CMD, UX.COMP.CONT, UX.COMP.WHY]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-24
---

# UX.COMP.REF: Component reference renderings and visual baselines

## Purpose

Reference renderings of every component in every applicable state, in all four themes, at 100 % and 200 % scale, exported from the design source into the package with a manifest. VIZ can use them as visual-regression baselines; reviewers use them to check contrast and focus visibility.

## Scope

**In:** `components/ref/<comp>/<theme>/<scale>/<state>.png`; `components/ref/manifest.json` (`ref-renderings@1`); `fid-design ref-check` (completeness and digests).
**Out:** pixel comparison of VIZ output against baselines (VIZ tests); the specs (sibling COMP nodes).

## Requirements

- **REQ-UX.COMP.REF-001** For every component spec and every state marked `applies: true`, the package shall contain a PNG for each theme (`light`, `dark`, `hc`, `floor`) at `1x` and `2x` scale.
- **REQ-UX.COMP.REF-002** The manifest shall record for each PNG its SHA-256, pixel size, design-source ID from `sources.json` and the package version it was exported for.
- **REQ-UX.COMP.REF-003** If a rendering is missing, its digest differs, or its pixel size at `2x` is not twice the `1x` size, then `fid-design ref-check` shall fail and name it.
- **REQ-UX.COMP.REF-004** When a component spec's `tokens` or `states` change in a release, `ref-check --since <old package>` shall report the component's renderings as stale until re-exported.

## Acceptance criteria

- **AC-1** Given the shipped renderings, when `fid-design ref-check --root design --json` runs, then it exits 0.
- **AC-2** (error path) Given fixture `fixtures/ux/comp/ref/missing-hc/`, when the check runs, then it exits 1 with `REF-MISSING comp.button hc 1x focus`.
- **AC-3** (error path) Given `bad-scale/` (2x image 1.5 times the 1x size), when the check runs, then it exits 1 with `REF-SCALE`.
- **AC-4** Given `old/` and `new/` packages where `comp.button` changed a token, when `ref-check --since old` runs on `new`, then it reports `REF-STALE comp.button`.

## Design

- **Command:** `fid-design ref-check --root <dir> [--since <old dir>] [--json]`. Tooling, nearest class R0.
- **PNG reading:** header-only parse for size; SHA-256 over the file bytes.
- **Errors:** `REF-MISSING`, `REF-DIGEST`, `REF-SCALE`, `REF-STALE`, `REF-SOURCE-UNKNOWN`, `REF-MANIFEST-SCHEMA`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `refs`.

## Test fixtures

`fixtures/ux/comp/ref/`: `missing-hc/`, `bad-scale/`, `old/`, `new/` with tiny generated PNGs; CC-BY-4.0.

## Build prompt

> Export reference renderings of every component spec and every applicable state in themes light, dark, hc and floor at 1x and 2x into `components/ref/<comp>/<theme>/<scale>/<state>.png`, and write `components/ref/manifest.json` (`ref-renderings@1`: path, sha256, width, height, source ID, package version). In the Rust tooling crate `fid-design`, add `fid-design ref-check --root <dir> [--since <old>] [--json]` checking completeness against the specs, digests, 2x = 2 × 1x size, and staleness when a spec's tokens or states changed since the old package. Errors `REF-MISSING|REF-DIGEST|REF-SCALE|REF-STALE|REF-SOURCE-UNKNOWN|REF-MANIFEST-SCHEMA`. Exit 0, 1, 2. Tests AC-1..AC-4 of spec node UX.COMP.REF.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; PNG header parse only, bounded; no network.

## Open questions

- Whether VIZ compares against these baselines with a tolerance is VIZ's choice; renderings from a design tool will not match a toolkit pixel for pixel.
- `components/TEMPLATE.md` (UX.COMP.SCHEMA) is a checked `component-spec@1` file with id `comp.template`. `ref-check`'s completeness rule (REQ-001) must either exclude `comp.template` explicitly or the template needs renderings like any other spec; excluding it is the smaller choice, since the template is an example, not a shipped component.

## Children

_Leaf: none._
