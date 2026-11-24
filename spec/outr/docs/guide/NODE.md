---
id: OUTR.DOCS.GUIDE
title: Executable, WASM-interactive Metrology Field Guide
level: 3
parent: OUTR.DOCS
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS-008, REQ-OUTR.DOCS-011]
covers: []
depends_on: [OUTR.DOCS.FRAME]
uses_artifacts: [OUTR.SITE.GEN, OUTR.BRAND.LINT]
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# OUTR.DOCS.GUIDE: Executable, WASM-interactive Metrology Field Guide

## Purpose

An executable, WASM-interactive Metrology Field Guide (D-054) that explains dimensional metrology piece by piece strictly in lockstep with implemented kernel crates—starting with P0/P1a feature fits (`fid-fit`), GD&T zones and datums (`fid-gps`), uncertainty propagation (`fid-kunc`), ISO 14253 / JCGM 106 decision rules (`fid-kdec`), and RFC 8785 canonical replay (`fid-canon`)—paired with one concise Dimensional Metrology Field Map showing the full 0.1 nm to 100 m discipline and honest scope boundaries.

## Scope

**In:**
- Pure-Rust safe `wasm32-wasip1` field-guide runner crate (`fid-guide-wasm`) wrapping implemented L0 kernel libraries
- Structured Dimensional Metrology Field Map (`guide/map/field-map.json`) mapping all 10 pillars to live spec-tree counts and D-054 scope tiers
- Executable P0/P1a concept chapters (`guide/topics/*.json`) with clean-room math, clause citations only, D-045 provisional labels, and CI-verified kernel examples
- Static site `/guide/` pages rendered by `site/build.py` with a zero-dependency same-origin browser WASI sandbox (`site/src/guide-lab.js`), no-JS static fallbacks, and scoped CSP (`site/worker.js`)

**Out** (and which node owns it instead):
- App user-guide chapters (`OUTR.DOCS.BOOK`)
- Command reference pages (`OUTR.DOCS.REF`)
- Step-by-step recorded tutorials and videos (`OUTR.CONTENT`)
- GD&T interpretation governance and public rule roster (`OUTR.GDTC`)

## Requirements

- **REQ-OUTR.DOCS.GUIDE-001** The Field Guide shall provide a safe-Rust (`#[forbid(unsafe_code)]`) `wasm32-wasip1` binary (`fid-guide-wasm`) that executes implemented L0 kernel operations (`fit.circle2d`, `fit.plane_datum`, `fit.cylinder_mcc`, `gdt.zone_eval`, `unc.propagate`, `dec.evaluate`, `canon.digest`) over bounded JSON stdin/stdout with bit-exact KNP-1 determinism.
- **REQ-OUTR.DOCS.GUIDE-002** The Field Guide shall publish a machine-checked Dimensional Metrology Field Map covering all 10 major pillars across 0.1 nm to 100 m, cross-referenced to live spec-tree node counts and D-054 scope tiers (`live`, `roadmap`, `post-1.0 core`, `plugin / companion`).
- **REQ-OUTR.DOCS.GUIDE-003** Every interactive concept chapter shall map exclusively to implemented kernel crates, verify its worked examples against `fid-guide-wasm` in CI, visibly display D-045 `interpretation: provisional` badges where applicable, and cite standards by designator, edition and clause only (never pasting standards text).
- **REQ-OUTR.DOCS.GUIDE-004** Built `/guide/` static pages shall make zero third-party network requests (`REQ-OUTR.SITE-007`), stay within the 500 KB HTML page budget (`REQ-OUTR.SITE-010`), degrade cleanly with static verified SVG/table snapshots when JavaScript or WebAssembly is unavailable, and scope `script-src 'self' 'wasm-unsafe-eval'` strictly to `/guide/` while keeping `script-src 'none'` on all other site routes.

## Interfaces

- **Offers:** `crates/fid-guide-wasm` (`wasm32-wasip1` and native runner); `guide/map/field-map.json`; `guide/topics/*.json`; `/guide/` static site pages and `site/src/guide-lab.js`.
- **Consumes:** Pure L0 libraries (`fid-fit`, `fid-gps`, `fid-kunc`, `fid-kdec`, `fid-canon`, `fid-num`, `fid-types`); spec tree metadata via `tools/scope.py`; `site/build.py` and `site/worker.js`.

## Split rationale

The module splits cleanly along artifact boundaries so each leaf is independently testable in one PR: `WASM` (the pure-Rust kernel wrapper crate), `MAP` (the validated 10-pillar taxonomy dataset), `TOPICS` (the 6 verified executable concept chapters), and `SITE` (the static site generator, browser WASI sandbox and scoped CSP integration).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.DOCS.GUIDE.WASM](wasm/NODE.md) | Safe-Rust WASM Field Guide kernel runner (leaf) | 001 | none |
| [OUTR.DOCS.GUIDE.MAP](map/NODE.md) | Dimensional Metrology Field Map dataset and validator (leaf) | 002 | none |
| [OUTR.DOCS.GUIDE.TOPICS](topics/NODE.md) | Executable P0/P1a Field Guide topic chapters and verifier (leaf) | 003 | none |
| [OUTR.DOCS.GUIDE.SITE](site/NODE.md) | Static site /guide/ generator, browser WASI lab and scoped CSP (leaf) | 004 | none |
