---
id: IO.CAD.PMI.READ
title: AP242 PMI entity graph extraction and semantic or graphic classification
level: 4
parent: IO.CAD.PMI
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.PMI-003, REQ-IO.CAD.PMI-004]
covers: []
depends_on: [IO.CAD.STEP.P21, IO.CAD.STEP.HDR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.PMI.READ: AP242 PMI entity graph extraction and classification

## Purpose

Walks a parsed AP242 file and extracts every PMI item into a neutral `PmiGraph`: semantic tolerances, dimensions, datum features, datums and datum systems with the shape aspects and faces they reference, and every presentation (graphic) item with its link to semantics, if any. It classifies each item as `semantic`, `graphic_only` or `unsupported` before any QIF mapping happens, so nothing can be dropped silently later.

## Scope

**In:**
- Semantic entities: `shape_aspect` and its subtypes, `geometric_tolerance` subtypes and the `_with_datum_reference`, `_with_modifiers`, `_with_defined_unit`, `_with_defined_area_unit` complex forms, `datum_feature`, `datum`, `datum_target`, `datum_system`, `datum_reference_compartment`, `datum_reference_element`, `dimensional_size`, `dimensional_location`, `dimensional_characteristic_representation`, `plus_minus_tolerance`, `tolerance_value`, `limits_and_fits` [unverified: complete entity list to confirm against CAx-IF recommended practices]
- Links: `item_identified_representation_usage` and `geometric_item_specific_usage` from shape aspects to faces and edges by entity number
- Presentation: annotation occurrences (tessellated and polyline) and `draughting_model_item_association` linking presentation to semantics
- Classification: `semantic` (supported construct), `graphic_only` (presentation with no semantic link), `unsupported` (semantic construct the reader does not model), each with entity number and type
- Tolerance and dimension values kept as authored lexemes with their unit context

**Out:**
- Mapping to QIF (`IO.CAD.PMI.TOL`, `.DIM`, `.DAT`)
- Rendering graphic PMI (VIZ)

## Requirements

- **REQ-IO.CAD.PMI.READ-001** Every PMI-related entity in the file shall appear in the `PmiGraph` exactly once with a classification of `semantic`, `graphic_only` or `unsupported`, its entity number and its entity type.
- **REQ-IO.CAD.PMI.READ-002** A presentation item with no association to a semantic item shall be classified `graphic_only` and shall carry no characteristic data.
- **REQ-IO.CAD.PMI.READ-003** A semantic item whose entity type or complex-entity combination is not in the reader's supported table shall be classified `unsupported` with its full entity-type list.
- **REQ-IO.CAD.PMI.READ-004** Each semantic item shall list the entity numbers of the faces and edges its shape aspects resolve to, and if a reference cannot be resolved, then the item shall carry `link_unresolved` with the dangling entity number.
- **REQ-IO.CAD.PMI.READ-005** Numeric tolerance and dimension values shall be carried as the authored lexeme from the file plus the unit of their representation context.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/pmi/read/flatness_semantic.stp` (one flatness tolerance on one face, with linked presentation), when read, then the graph has one `semantic` tolerance linked to the face's entity number, value lexeme `0.05`, unit mm, and its presentation is linked, not `graphic_only`.
- **AC-2** Given `fixtures/io/cad/pmi/read/graphic_only.stp` (polyline annotation with no semantics), then exactly one `graphic_only` item exists and no semantic item.
- **AC-3** Given `fixtures/io/cad/pmi/read/unknown_complex.stp` (a tolerance with an unmodelled complex-entity leaf), then one `unsupported` item lists all leaf types.
- **AC-4** Given `fixtures/io/cad/pmi/read/dangling_aspect.stp`, then the item carries `link_unresolved` with the dangling number.
- **AC-5** Given any fixture, the count of graph items equals the count of PMI-related entities found by an independent entity-type scan (completeness check).

## Design

- **Registry commands:** none (library; runs inside the CAD worker when PMI is requested).
- **Data structures:** `PmiGraph { items: BTreeMap<u64, PmiItem> }`; `PmiItem { entity: u64, types: Vec<EntityName>, class: Semantic|GraphicOnly|Unsupported, kind: Tolerance|Dimension|DatumFeature|Datum|DatumTarget|DatumSystem|Presentation, links: Vec<u64>, values: Vec<Lexeme>, unit_ctx: Option<u64>, raw: Vec<Param> }`.
- **Algorithm:** index instances by entity type; walk semantic roots; resolve shape aspects to geometry through usage entities; then mark presentations linked via associations; finally sweep all remaining PMI-typed instances into `unsupported` or `graphic_only`.
- **Errors:** `malformed` for structurally impossible references; everything else is a classification.
- **Determinism:** D0 (ascending entity numbers).
- **Performance budget:** 10^4 PMI items in < 1 s [unverified until measured].
- **Crate:** `fid-io-pmi`.

## Test fixtures

Hand-written AP242 fragments under `fixtures/io/cad/pmi/read/` (CC-BY-4.0); NIST MBE PMI models through the corpus (see `IO.CAD.PMI.CMD`).

## Build prompt

> In crate `fid-io-pmi`, implement `read_pmi(&P21File, &StepHeaderInfo) -> PmiGraph`. Extract semantic PMI (shape aspects and subtypes, geometric tolerances incl. complex forms with datum reference, modifiers and defined units, datum features, datums, datum targets, datum systems and compartments, dimensional size and location with plus-minus, value and limits-and-fits tolerances), resolve shape-aspect links to face and edge entity numbers (`link_unresolved` if dangling), and collect presentation items with their semantic associations. Classify every PMI entity exactly once as `semantic`, `graphic_only` or `unsupported` (with full entity-type list). Keep numeric values as authored lexemes plus unit context. No standards or recommended-practice text in code; entity names only. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-5.

## Open questions

- Exact supported-entity table to be built from CAx-IF recommended practices [unverified: edition and licence of those documents; cite by title only].

## Children

_Leaf: none._
