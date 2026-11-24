---
id: CAL.GAUGE
title: Master, gauge, thread, surface-plate and hand-tool calibration
level: 2
parent: CAL
kind: subsystem
status: open
phase: P4
size: XL
owners: []
claimed_by:
satisfies: [REQ-CAL-003]
covers: []
depends_on: [KRN.NUM, KRN.LSQ, UNC.PROP, UNC.DEC]
cli: [fid cal eval-gauge]
mcp: [cal_eval_gauge]
journal: [cal.eval_gauge]
updated: 2026-11-22
---

# CAL.GAUGE: Master, gauge, thread, surface-plate and hand-tool calibration

## Purpose

Clean-room mathematical calibration procedures for dimensional masters, reference artifacts and shop-floor hand tools: gauge blocks (ISO 3650), cylindrical plugs/rings, screw-thread plug/ring gauges (three-wire pitch diameter, ASME B1.2 / ISO 1502), surface plates (ISO 8512 / Moody method), hand tools (ISO 13385 calipers, ISO 3611 micrometers, ISO 463 dial gauges), and angle standards (polygons, sine bars, levels, autocollimators with circle closure).

## Scope

**In:** Differential thermal expansion and elastic-deformation contact corrections (Hertzian contact), three-wire pitch-diameter reduction with rake/elastic terms, Moody Union-Jack surface-plate flatness grid solver, circle-closure error separation for polygon/indexing calibration, and grade/MPE evaluation for hand tools and gauge blocks.
**Out:** Optical interferometric fringe demodulation (`ECO.SDK` nano/optics plugins per D-054); certificate formatting (`CAL.CERT`).

## Requirements

- **REQ-CAL.GAUGE-001** The subsystem shall compute dimensional calibration reductions (gauge blocks, three-wire thread pitch diameter, Moody surface-plate flatness, angle closure and hand-tool MPE errors) in pure-Rust `f64` under KNP-1 with explicit physical correction terms.
- **REQ-CAL.GAUGE-002** Every calibration procedure shall propagate master, thermal and repeatability contributors through `UNC.PROP` before passing to `CAL.CERT`.

## Children

_None yet: status `open`._
