---
id: OUTR.DOCS.GUIDE.TOPICS
title: Executable P0/P1a Field Guide topic chapters and verifier
level: 4
parent: OUTR.DOCS.GUIDE
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS.GUIDE-003]
covers: []
depends_on: [OUTR.DOCS.GUIDE.WASM, OUTR.DOCS.GUIDE.MAP]
uses_artifacts: [OUTR.BRAND.LINT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# OUTR.DOCS.GUIDE.TOPICS: Executable P0/P1a Field Guide topic chapters and verifier

## Purpose

Authors the six initial P0/P1a interactive Metrology Field Guide chapters (`guide/topics/*.json`), strictly scoped to already-implemented kernel crates (`fid-fit`, `fid-gps`, `fid-kunc`, `fid-kdec`, `fid-canon`), each paired with executable request payloads and kernel-verified expected digests checked automatically against `fid-guide-wasm`.

## Scope

**In:**
- Six structured topic definitions under `guide/topics/`:
  1. `fits.json`: Feature fitting — Least-Squares vs. Tangential Datum Plane & Minimum Circumscribed Cylinder (`fid-fit`)
  2. `gdt.json`: GD&T fundamentals — Datums, Tolerance Zones & ASME vs. ISO Dialects (`fid-gps`)
  3. `uncertainty.json`: Measurement Uncertainty — GUM LPU, Adaptive Monte Carlo & UEC-0..3 (`fid-kunc`)
  4. `decisions.json`: Conformance Decision Rules — ISO 14253-1 & JCGM 106 Guardbanding and Risk (`fid-kdec`)
  5. `numerics-replay.json`: Determinism, Units & Cryptographic Replay — KNP-1 & RFC 8785 (`fid-canon`, `fid-num`)
  6. `field-map.json`: The Complete Map of Dimensional Metrology (0.1 nm to 100 m) (`guide/map/field-map.json`)
- Each executable topic carries preset lab scenarios (`lab_presets[]`) whose exact `expected_result` and `expected_digest` are verified against `fid-guide-wasm` in `tools/tests/test_guide.py`
- Clean-room prose paraphrasing principles in our own words with standard designators/clauses only and explicit D-045 `interpretation: provisional` badges on GD&T/decision rules

**Out:**
- Unimplemented P2–P4 chapters (never authored ahead of built kernel crates per D-054)
- HTML/JS site rendering (`OUTR.DOCS.GUIDE.SITE`)

## Requirements

- **REQ-OUTR.DOCS.GUIDE.TOPICS-001** Every executable topic shall name the implemented workspace crate(s) and spec leaf ID(s) it backs, and all named crates shall exist in `crates/`.
- **REQ-OUTR.DOCS.GUIDE.TOPICS-002** Every `lab_presets` entry in `guide/topics/*.json` shall execute cleanly through `fid-guide-wasm` and match its stored `expected_digest` bit-for-bit in automated tests.
- **REQ-OUTR.DOCS.GUIDE.TOPICS-003** Topics covering GD&T or standards-dependent decision defaults shall declare `provisional: true` (D-045), cite standards by designator, edition and clause only, and pass vocabulary hygiene checks with zero errors.

## Acceptance criteria

- **AC-1** `python3 -m unittest tools/tests/test_guide.py` verifies all 6 topic files exist, reference real crates and spec leaves, and pass vocabulary/standards hygiene checks.
- **AC-2** Running every topic's `lab_presets` through `fid-guide-wasm` returns `ok: true` and matches every recorded `expected_digest` bit-for-bit.
- **AC-3** Every topic includes a runnable CLI command snippet (`fid canon` or `cargo run -q -p fid-guide-wasm`) verified against the live workspace.

## Design

- **Files:** `guide/topics/{fits,gdt,uncertainty,decisions,numerics-replay,field-map}.json` (`schema: "guide-topic/v1"`).
- **Fields:** `slug`, `order`, `title`, `subtitle`, `phase`, `status` (`live-kernel` | `taxonomy-map`), `provisional`, `crates`, `spec_nodes`, `standards` (designator + edition + topic, never text), `sections`, `lab` (`op`, `widget_kind`, `presets` with `request`, `expected_digest`, `expected_summary`), `cli_example`.
- **Determinism:** D0 (kernel outputs and digests are bit-exact).

## Test fixtures

The six `guide/topics/*.json` files verified live against `fid-guide-wasm` in `tools/tests/test_guide.py`.

## Build prompt

> Author the six P0/P1a Field Guide chapter definitions under `guide/topics/{fits,gdt,uncertainty,decisions,numerics-replay,field-map}.json` (`schema: "guide-topic/v1"`), strictly limited to implemented crates (`fid-fit`, `fid-gps`, `fid-kunc`, `fid-kdec`, `fid-canon`). Write original clean-room explanations, cite standards by designator/edition/clause only (never paste standards text), set `provisional: true` on GD&T and decision-rule topics (D-045), and record verified `expected_digest` and `expected_summary` values produced by running `fid-guide-wasm` on every preset. Verify all presets end-to-end in `tools/tests/test_guide.py`.

## Children

_Leaf: none._
