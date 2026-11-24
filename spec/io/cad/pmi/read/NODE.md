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
built_in: [fid-io-pmi]
built_pr: 597
satisfies: [REQ-IO.CAD.PMI-003, REQ-IO.CAD.PMI-004]
covers: []
depends_on: [IO.CAD.STEP.P21, IO.CAD.STEP.HDR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-24
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

### Pinned at build (crate `fid-io-pmi`)

These pins were made when the leaf was built. Where one differs from the text above, the pin wins, and the reason is given.

- **API.** `read_pmi(&P21File, &StepHeaderInfo) -> Result<PmiGraph, PmiError>` and `read_pmi_with(.., &PmiOptions)` (deviation: a `Result`, because structurally impossible references are `malformed` and the walk is bounded). Bounds: `max_items` (1,000,000), `max_alloc_bytes` (1 GiB of tracked allocation: items, raw parameters, lists, values, indexes and transient visited sets, each charged a nominal cost at least its real size, asserted by a test) and `max_work` (200,000,000 steps). Exceeding one is `limit_exceeded` naming the bound. The walk is iterative; nothing panics.
- **Item.** `PmiItem { entity, types, class, kind, unsupported: Option<reason>, links, link_unresolved, refs, presented_by, values: Vec<PmiValue>, raw }`. Deviations: `link_unresolved`, `refs` (the PMI items an item refers to) and `presented_by` are explicit lists; the unit context sits on each value (`PmiValue { measure, measure_type, lexeme, unit_entity, unit_ctx, unit }`), not once per item, because one tolerance can carry values in two units. `kind` adds `dimension_representation`, `plus_minus`, `tolerance_value`, `limits_and_fits`, `shape_aspect`, `datum_reference` and `unknown`, so every PMI-related entity has a kind.
- **Which instances are items.** An instance is an item when any part name is PMI-related. A presentation name is `DRAUGHTING_CALLOUT`, any `*_CALLOUT`, any `*ANNOTATION_OCCURRENCE`, or `ANNOTATION_*_OCCURRENCE`. A semantic name is any other name that is in the supported table, in a short extra list (`ANGULAR_LOCATION`, `ANGULAR_SIZE`, `APEX`, `CENTRE_OF_SYMMETRY`, `EXTENSION`, `GEOMETRIC_ALIGNMENT`, `GEOMETRIC_INTERSECTION`, `PARALLEL_OFFSET`, `PERPENDICULAR_TO`, `TANGENT`), ends in `SHAPE_ASPECT`, or has the word `TOLERANCE`, `DATUM` or `DIMENSIONAL` between underscores, except `DIMENSIONAL_EXPONENTS` [unverified: lists to confirm against the CAx-IF recommended practices]. Link entities (`*_USAGE`, the draughting-model item associations, `SHAPE_ASPECT_RELATIONSHIP`, measures, representations) are traversed, never items.
- **Supported table.** Kinds: the 15 geometric tolerance types and `GEOMETRIC_TOLERANCE` with its `_WITH_DATUM_REFERENCE`, `_WITH_MODIFIERS`, `_WITH_DEFINED_UNIT` and `_WITH_DEFINED_AREA_UNIT` forms (tolerance); `DIMENSIONAL_SIZE`, `DIMENSIONAL_LOCATION` (dimension); `DIMENSIONAL_CHARACTERISTIC_REPRESENTATION`; `PLUS_MINUS_TOLERANCE`; `TOLERANCE_VALUE`; `LIMITS_AND_FITS`; `SHAPE_ASPECT`, `COMPOSITE_SHAPE_ASPECT`, `COMPOSITE_GROUP_SHAPE_ASPECT`, `ALL_AROUND_SHAPE_ASPECT` (shape aspect); `DATUM_FEATURE`; `DATUM`; `DATUM_TARGET`; `DATUM_SYSTEM`; `DATUM_REFERENCE_COMPARTMENT`, `DATUM_REFERENCE_ELEMENT`, `GENERAL_DATUM_REFERENCE` (datum reference). A semantic instance is supported when every part name is in the table, the names agree on one kind (`shape_aspect` yields to any other), and it has at most one tolerance type. Otherwise it is `unsupported` with reason `type_not_supported`, `mixed_kinds` or `two_tolerance_types`, its full type list, and the kind its supported names agree on (else `unknown`). Presentation and semantic names in one instance: `unsupported`, `mixed_presentation`.
- **Attributes.** An attribute is read from the part that declares it in a complex instance, or from the only part of a simple one. A tolerance's toleranced aspect, a size dimension's aspect, a location dimension's two aspects, a usage's definition and identified items (one reference or a list), a relationship's two aspects, a measure's value and unit, a dimension representation's items and a linked callout's contents must have the right form, or the read fails `malformed` naming the instance.
- **Geometry (REQ-004).** For tolerances, dimensions, shape aspects, datum features, datums and datum targets, the reader walks from the item: the identified items of every non-association usage whose definition is the node go to `links`; a tolerance continues to its toleranced aspect, a dimension to its aspects, a composite aspect to the related side of its relationships, a datum to the relating side (its datum features). Each node is visited once. Undefined numbers met anywhere go to `link_unresolved`.
- **References and values (REQ-005).** Every reference in an item's parameters (depth-first, in order) goes to `link_unresolved` (undefined), `refs` (a PMI item) or, for supported items only, `values` (a measure with unit). A dimension representation's values are its representation's measure items. The lexeme is the authored real token, or the decimal form of an integer (the parser keeps no integer spelling). `unit_ctx` is the representation's context for those, else the lowest-numbered context whose unit assignment lists the measure's unit. The unit is that context's effective length unit or plane-angle unit (from `IO.CAD.STEP.HDR`), and `unresolved` when the context does not assign that unit or the unit is undefined.
- **Presentations (REQ-002).** A draughting-model item association whose definition is a PMI item links its presentation items (and, for a callout, the presentation items among its contents) to it: the presentation's `refs` and the item's `presented_by`. A linked presentation is `semantic`; an unlinked one is `graphic_only` with no links, refs or values (an association to an undefined definition leaves `link_unresolved` on it). An undefined presentation in an association goes to the definition's `link_unresolved`.
- **Fixtures.** `fixtures/io/cad/pmi/read/make_cases.py` (stdlib only) writes 5 invented `.stp` files and `expected.txt` from its own instance model and oracle, plus an independent top-level name scan of the emitted text for AC-5. CI runs it with `--check`. Malformed and bound cases are hand-labelled in the tests. No third-party CAD files and no standards text (D-038).

## Test fixtures

Hand-written AP242 fragments under `fixtures/io/cad/pmi/read/` (CC-BY-4.0); NIST MBE PMI models through the corpus (see `IO.CAD.PMI.CMD`).

## Build prompt

> In crate `fid-io-pmi`, implement `read_pmi(&P21File, &StepHeaderInfo) -> PmiGraph`. Extract semantic PMI (shape aspects and subtypes, geometric tolerances incl. complex forms with datum reference, modifiers and defined units, datum features, datums, datum targets, datum systems and compartments, dimensional size and location with plus-minus, value and limits-and-fits tolerances), resolve shape-aspect links to face and edge entity numbers (`link_unresolved` if dangling), and collect presentation items with their semantic associations. Classify every PMI entity exactly once as `semantic`, `graphic_only` or `unsupported` (with full entity-type list). Keep numeric values as authored lexemes plus unit context. No standards or recommended-practice text in code; entity names only. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-5.

## Open questions

- Exact supported-entity table to be built from CAx-IF recommended practices [unverified: edition and licence of those documents; cite by title only].
- Performance budget (10^4 PMI items < 1 s) is not yet measured on reference hardware; a smoke test reads 10^4 items within the default bounds.
- Presentation items are not yet resolved to their draughting model or annotation plane; `IO.CAD.PMI.CMD` may need that for its report.

## Children

_Leaf: none._
