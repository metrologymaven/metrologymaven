---
id: OUTR.DOCS.GUIDE.MAP
title: Dimensional Metrology Field Map dataset and validator
level: 4
parent: OUTR.DOCS.GUIDE
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS.GUIDE-002]
covers: []
depends_on: []
uses_artifacts: [OUTR.BRAND.LINT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# OUTR.DOCS.GUIDE.MAP: Dimensional Metrology Field Map dataset and validator

## Purpose

Maintains the structured taxonomy dataset (`guide/map/field-map.json`) covering all 10 major pillars of dimensional metrology across 12 orders of magnitude (0.1 nm to 100 m), linking each pillar to its governing standard designators (never standards text), scale range, primary instruments, D-054 scope tier (`core-1.0`, `core-post-1.0`, `plugin-companion`), and Metrology Maven spec domain IDs so the Field Guide renders an honest, live-counted map of the entire field.

## Scope

**In:**
- `guide/map/field-map.json`: structured schema `field-map/v1` with 10 dimensional metrology pillars, scale bands (`nm`, `um`, `mm`, `m`, `100m`), standard designators only, and mapped spec domain IDs
- Python unit tests (`tools/tests/test_guide.py`) validating schema integrity, valid spec domain IDs against `tools/scope.py`, D-054 tier alignment, and zero prohibited regulated-claim / incumbent-comparison strings

**Out:**
- Interactive chapter labs (`OUTR.DOCS.GUIDE.TOPICS`)
- HTML/CSS rendering (`OUTR.DOCS.GUIDE.SITE`)

## Requirements

- **REQ-OUTR.DOCS.GUIDE.MAP-001** `guide/map/field-map.json` shall classify all 10 major dimensional metrology pillars (coordinate/CMM & optical 3D scan, large-volume networks, GD&T & uncertainty, surface texture & precision form, calibration lab & verification, MSA & quality statistics, 2D/2.5D vision, industrial X-ray CT, on-machine CNC probing, micro/nano/semiconductor & optical interferometry, plus adjacent dimensional engineering noted distinctly) with scale range and D-054 scope tier.
- **REQ-OUTR.DOCS.GUIDE.MAP-002** Every spec domain ID referenced in `guide/map/field-map.json` shall exist in `spec/` as checked by `tools/scope.py`.
- **REQ-OUTR.DOCS.GUIDE.MAP-003** No field-map entry shall contain pasted standards text, unqualified regulated claim words (`certified`, `compliant`, `validated`, `traceable` without a designator), or named incumbent product comparisons.

## Acceptance criteria

- **AC-1** `python3 -m unittest tools/tests/test_guide.py` validates `guide/map/field-map.json` against schema rules and confirms every listed `spec_domains` entry exists in `spec/`.
- **AC-2** Every D-054 core expansion domain (`SURF`, `CAL`, `RPT.STATS.MSA`) appears with tier `core-post-1.0`, and CT/vision/CNC-OMV/nano/adjacent entries appear with tier `plugin-companion`.
- **AC-3** Automated vocabulary and standards-text hygiene checks pass with zero findings.

## Design

- **Files:** `guide/map/field-map.json` (UTF-8 JSON, `schema: "field-map/v1"`, `decision_ref: "D-054"`), validated by `tools/tests/test_guide.py`.
- **Determinism:** D0 (static structured data checked deterministically).

## Test fixtures

`guide/map/field-map.json` validated against the live repository `spec/` tree.

## Build prompt

> Author `guide/map/field-map.json` (`schema: "field-map/v1"`, `decision_ref: "D-054"`) describing the full landscape of dimensional metrology across 0.1 nm to 100 m, linking every pillar to standard designators only (never standards text), scale range, representative instruments, D-054 scope tier (`core-1.0`, `core-post-1.0`, `plugin-companion`), and existing `spec/` node/domain IDs. Add automated tests in `tools/tests/test_guide.py` checking schema validity, live `scope.py` node existence, D-054 tier accuracy, and vocabulary hygiene.

## Children

_Leaf: none._
