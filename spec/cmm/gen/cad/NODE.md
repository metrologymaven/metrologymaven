---
id: CMM.GEN.CAD
title: Measurement features from CAD selection
level: 3
parent: CMM.GEN
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.GEN-001, REQ-CMM.GEN-008]
covers: [PCD.CAD.03]
depends_on: [GEO, CMM.STRAT.FEAT, CMM.STRAT.LIB, CMM.GEN.PROV]
cli: [fid cmm generate-features-from-cad]
mcp: [cmm_generate_features_from_cad]
journal: [cmm.generate_features_from_cad@1]
updated: 2026-11-16
---

# CMM.GEN.CAD: Measurement features from CAD selection

## Purpose

Turns a selection of CAD faces and edges into nominal measurement features (plane, cylinder, cone, sphere, circle from a circular edge, line from a linear edge, surface points on freeform faces) with default strategies attached. It is the programmer's "click the hole, get a feature" step, and it reports every selection it could not classify instead of guessing.

## Scope

**In:**
- Primitive recognition from selected B-rep faces and edges (analytic surface types first, then tolerance-based recognition for NURBS faces that are near-analytic)
- Nominal feature creation (position, direction, size) in the part frame
- Default strategy attachment from the strategy defaults
- Unclassified-selection report
- `cmm.generate_features_from_cad@1` (W2; agents write to an agent branch)

**Out** (and which node owns it instead):
- CAD import and B-rep model (owned by `GEO` and `IO`)
- Selection and picking UI (owned by `VIZ`)
- Strategy types and defaults (owned by `CMM.STRAT.FEAT` and `CMM.STRAT.LIB`)
- Provenance tagging (owned by `CMM.GEN.PROV`)

## Requirements

- **REQ-CMM.GEN.CAD-001** When faces or edges are selected, the system shall create one nominal feature per recognised primitive, with the nominal taken from the CAD geometry in SI metres and the part frame.
- **REQ-CMM.GEN.CAD-002** The system shall recognise analytic plane, cylinder, cone, sphere and torus faces exactly, and NURBS faces as analytic only if their deviation from the fitted primitive is below a configurable tolerance (default 1 µm [unverified]).
- **REQ-CMM.GEN.CAD-003** The system shall attach to each new feature the default strategy for its type and record the strategy source.
- **REQ-CMM.GEN.CAD-004** The system shall report every selected entity it could not classify, with its CAD entity ID and a reason code, and shall create no feature for it.
- **REQ-CMM.GEN.CAD-005** Generation shall be deterministic: the same CAD digest, selection and generator version shall produce byte-identical features.

## Interfaces

- **Offers:** `cmm.generate_features_from_cad@1` (W2); FeatureGenerationReport type.
- **Consumes:** `GEO` B-rep faces and edges, frames, primitive fits; `CMM.STRAT.FEAT` and `CMM.STRAT.LIB` default strategies; `CMM.GEN.PROV` provenance.
- **Journal:** `cmm.generate_features_from_cad@1`.

## Decomposition hints

RECOG (primitive recognition), NOMINAL (nominal construction), CMD (command and report).

## Open questions

- Recognition of composite features (slots, pockets) from several faces may need its own child.

## Children

_None yet: status `open`._
