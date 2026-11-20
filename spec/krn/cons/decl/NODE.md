---
id: KRN.CONS.DECL
title: Construction contract and dependency declarations
level: 3
parent: KRN.CONS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-construct]
built_pr: 291
satisfies: [REQ-KRN.CONS-002, REQ-KRN.CONS-003, REQ-KRN.CONS-004, REQ-KRN.CONS-009, REQ-KRN.CONS-010]
covers: []
depends_on: [KRN.LSQ.TRAIT, KRN.NUM.ERR, KRN.MAN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.CONS.DECL: Construction contract and dependency declarations

## Purpose

The contract every construction implements. A construction is a pure function of declared input elements and parameters; it publishes those inputs as a dependency declaration in plain data so that `CORE.DEP` can recompute it when an input changes, without the kernel depending on `CORE`. This leaf also fixes the typed error model for undefined and ill-conditioned constructions, the algorithm ID scheme and the shape of the journal event.

## Scope

**In:** the `Construction` trait; `ElementRef` (opaque 128-bit element ID plus role); `DependencyDecl` (algorithm ID, ordered input refs, parameter digest) and its canonical encoding; `ConstructParams` with explicit angular and distance thresholds; `ConstructedElement` (a `FittedFeature` geometry or a scalar relationship value); `ConstructError`; the purity and recompute-equivalence test harness; the data-row format for `construct.*` commands.
**Out:** the constructions themselves (`KRN.CONS.OPS`, `KRN.CONS.ELEM`, `KRN.CONS.DERIV`, `KRN.CONS.AUTO`, `KRN.CONS.REL`); graph scheduling and recompute (`CORE.DEP`); digests and the kernel call (`KRN.MAN`).

## Requirements

- **REQ-KRN.CONS.DECL-001** The `Construction` trait shall expose `declare(&self, params) -> DependencyDecl` and `evaluate(&self, inputs: &[ElementGeom], params) -> Result<ConstructedElement, ConstructError>`, where `inputs` are given in the declared order.
- **REQ-KRN.CONS.DECL-002** `DependencyDecl` shall contain the algorithm ID `construct.<op>.<variant>@<semver>`, the ordered input element IDs with roles, and the parameter digest, and shall have one canonical byte encoding.
- **REQ-KRN.CONS.DECL-003** `ConstructError` shall include `Undefined{reason}`, `IllConditioned{measure, value, threshold}`, `WrongInputKind{index, expected, got}` and `InputCount{expected, got}`, and no construction shall return a partial element with an error.
- **REQ-KRN.CONS.DECL-004** Every angular and distance threshold a construction uses shall be an explicit field of `ConstructParams` and part of the parameter digest.
- **REQ-KRN.CONS.DECL-005** The test harness shall check, for any construction, that evaluating twice gives bit-identical output and that evaluating after replacing an input equals a fresh evaluation on the new inputs bit for bit.
- **REQ-KRN.CONS.DECL-006** The module shall define the data-row format for `construct.<verb_object>@1` commands (W2 with R1 dry-run) and the journal fields: algorithm ID, input element IDs, parameter digest and output digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/decl-toy-v1` (a test-only midpoint construction and 100 seeded point pairs), when the harness runs, then repeated evaluation and replace-then-evaluate are bit-identical to fresh evaluation.
- **AC-2** Given the same construction declared twice with the same parameters, then the `DependencyDecl` encodings are byte-identical on x86-64, ARM64 and wasm32; changing any threshold changes the parameter digest.
- **AC-3** (error path) Given a test-only line-plane intersection with a line 1e-12 rad from parallel and a threshold of 1e-9 rad, then evaluation returns `IllConditioned{measure: "angle", value, threshold}` and no element.
- **AC-4** Given a construction called with 1 input when 2 are declared, then it returns `InputCount{expected: 2, got: 1}`.
- **AC-5** Given the command row schema, when sample rows are validated, then every numeric field declares a quantity kind and unit.

## Design

- **Registry command(s):** none directly; defines the row format for `construct.*` commands.
- **Data structures:** `struct ElementRef { id: [u8; 16], role: Role }`; `enum ElementGeom { Feature(FittedFeature), Points(PointsView), Scalar{value, kind} }`; `struct DependencyDecl { algorithm: AlgorithmId, inputs: Vec<ElementRef>, params_digest: [u8; 32] }`; `enum ConstructedElement { Feature(FittedFeature), Value{value, kind, unit} }`.
- **Algorithm:** canonical encoding as in `KRN.LSQ.TRAIT` (fixed field order, f64 as IEEE-754 hex); parameter digest computed by `KRN.MAN`'s hashing over that encoding.
- **Errors:** as in REQ-KRN.CONS.DECL-003, plus wrapped `NumError`.
- **Determinism:** D0.
- **Performance budget:** declaration and encoding < 5 microseconds per construction [unverified until measured].
- **Crate:** `fid-construct` (module `contract`).

## Test fixtures

`fixtures/cons/decl-toy-v1`: seeded point pairs and a near-parallel line-plane case, CC-BY-4.0.

## Build prompt

> Create crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-fit` only; no dependency on CORE or GEO crates) with module `contract`. Define `ElementRef { id: [u8;16], role }`, `ElementGeom` (fitted feature, point view or scalar), `DependencyDecl { algorithm: AlgorithmId /* construct.<op>.<variant>@<semver> */, inputs: Vec<ElementRef>, params_digest }` with a canonical byte encoding (fixed field order, f64 as IEEE-754 hex), `ConstructParams` (every angular and distance threshold explicit), `ConstructedElement` (feature or value with quantity kind and unit), `ConstructError` (Undefined, IllConditioned{measure,value,threshold}, WrongInputKind, InputCount, Num), and `trait Construction { fn declare(&self, &ConstructParams) -> DependencyDecl; fn evaluate(&self, &[ElementGeom], &ConstructParams) -> Result<ConstructedElement, ConstructError>; }`. Provide a generic test harness checking repeat and replace-then-evaluate bit identity. Define the data-row schema for `construct.<verb_object>@1` registry commands (W2, dry-run R1; params with quantity kind and unit; journal fields algorithm ID, input element IDs, parameter digest, output digest). Tests: `fixtures/cons/decl-toy-v1` with a test-only midpoint construction, cross-target encoding identity (x86-64/ARM64/wasm32), near-parallel IllConditioned case, InputCount error, row-schema validation. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.CONS.DECL.

## Open questions

- `CORE.DEP` must accept `DependencyDecl` as data; confirm the field names when `CORE.DEP` is expanded.
- Undeclared reads (#291 review): a construction cannot read undeclared supplied inputs; geometry captured in the construction value at `make()` is a contract violation the harness cannot detect. Closing this needs construction values that hold no geometry by type (for example only `ElementRef`s and plain options), not yet enforced.

## Children

_Leaf: none._
