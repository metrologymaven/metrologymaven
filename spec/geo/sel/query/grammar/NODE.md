---
id: GEO.SEL.QUERY.GRAMMAR
title: JSON AST grammar, bounded parser, canonical form and digest
level: 4
parent: GEO.SEL.QUERY
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.SEL.QUERY-001, REQ-GEO.SEL.QUERY-002]
covers: []
depends_on: [CORE.JRN.CANON, GEO.FRAME.UNITS, GEO.SEL.MODEL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.SEL.QUERY.GRAMMAR: JSON AST grammar, bounded parser, canonical form and digest

## Purpose

The semantic selection language is a closed JSON abstract syntax tree and nothing else: no text syntax, no expressions, no code, no free-text fields that are ever interpreted (C6). This leaf defines the schema (version 1), a bounded fail-closed parser into a typed `SelectionQuery`, a canonical form in which equivalent spellings coincide, and the digest used by journals and staleness checks.

## Scope

**In:**
- Top level: `{ "v": 1, "domain": "points" | "triangles" | "faces" | "edges", "where": Expr }`
- `Expr`: `{ "and": [Expr, ...] }`, `{ "or": [Expr, ...] }`, `{ "not": Expr }`, or exactly one predicate object `{ "<pred>": {params} }`
- Predicate set v1 (closed):
  - `all {}`
  - `ids { "ids": [u64] }` (element indexes)
  - `attr_range { "channel": "intensity" | "scanner_id" | "timestamp_ns", "min"?: number, "max"?: number }`
  - `quality_flags { "all_of"?: u16, "none_of"?: u16 }`
  - `normal_cone { "axis": [3 numbers], "half_angle": Angle }`
  - `in_box { "min": [Length; 3], "max": [Length; 3] }`, `in_sphere { "centre": [Length; 3], "radius": Length }`
  - `near { "of": Ref, "distance": Length }` where `Ref` is `{ "element": uuid }`, `{ "selection": uuid }` or `{ "face": { "body": uuid, "id": TopologyId } }`
  - `face_ids { "body": uuid, "ids": [TopologyId] }`, `in_group { "group": uuid }`, `in_layer { "layer": uuid }`, `in_body { "body": uuid }`
  - `tangent_grow { "seed": Expr, "angle": Angle }`
- `Length` and `Angle` are authored quantities `{ "text": "2", "unit": "mm" }` parsed by GEO.FRAME.UNITS
- Limits: 64 KiB input, depth 32, 1 024 AST nodes, 10 000 IDs per list
- Canonical form: flatten nested `and`/`or`, remove double `not`, sort and deduplicate operands by their canonical bytes, sort ID lists; RFC 8785 bytes through CORE.JRN.CANON; digest `sha256:<hex>`

**Out** (and which node owns it instead):
- Evaluation (owned by `GEO.SEL.QUERY.EVAL`, `GEO.SEL.QUERY.NEAR` and `GEO.SEL.QUERY.CAD`)
- Commands (owned by `GEO.SEL.QUERY.CMD`)
- Index-set storage (owned by `GEO.SEL.MODEL`, which holds the query opaquely)

## Requirements

- **REQ-GEO.SEL.QUERY.GRAMMAR-001** The parser shall accept only JSON that matches schema v1 exactly; any unknown key, unknown predicate, wrong type, missing field, extra predicate key in one object or unknown version shall fail with `bad_query` and a JSON pointer to the offending location.
- **REQ-GEO.SEL.QUERY.GRAMMAR-002** No string in a query shall be evaluated, templated or executed: strings are only enum tags, UUIDs (validated), topology labels (≤ 256 bytes, compared bytewise) or authored quantity text passed to GEO.FRAME.UNITS.
- **REQ-GEO.SEL.QUERY.GRAMMAR-003** If the input exceeds 64 KiB, depth 32, 1 024 AST nodes or 10 000 IDs in one list, then the parser shall fail closed with `query_limit`, without recursion on untrusted depth.
- **REQ-GEO.SEL.QUERY.GRAMMAR-004** Two queries that differ only in operand order of `and`/`or`, duplicate operands, nested same-operator groups, double negation, ID-list order or JSON whitespace and key order shall have identical canonical bytes and digest.
- **REQ-GEO.SEL.QUERY.GRAMMAR-005** The canonical form shall store each authored quantity with its text, unit and converted SI value bits, so the digest changes when the authored text changes even if the SI value does not.
- **REQ-GEO.SEL.QUERY.GRAMMAR-006** Parsing and canonicalisation shall be deterministic class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/sel/query/valid_v1.json` (60 valid queries covering every predicate and combinator), when parsed and canonicalised, then each canonical byte string and digest equals the committed vector.
- **AC-2** Given fixture `fixtures/geo/sel/query/equivalent_pairs.json` (40 pairs differing as in REQ-004), then each pair has equal digests; and 20 pairs that differ semantically have different digests.
- **AC-3** (error path) Given fixture `fixtures/geo/sel/query/invalid_v1.json` (80 malformed queries: unknown keys, two predicates in one object, strings where numbers belong, `"v": 2`, prompt-like text in a label field used as a key), then each fails with `bad_query` and the expected JSON pointer.
- **AC-4** (error path) Given a 10 000-deep nested `not` and a 65 KiB input, then `query_limit` without stack overflow.
- **AC-5** Given a fuzz run of 10⁶ mutated inputs (seed 37), then no panic occurs and every result is a typed query or a typed error.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `SelectionQuery { domain: Domain, root: Expr }`; `enum Expr { And(Vec<Expr>), Or(Vec<Expr>), Not(Box<Expr>), Pred(Predicate) }`; `enum Predicate { All, Ids(Vec<u64>), AttrRange{..}, QualityFlags{..}, NormalCone{..}, InBox{..}, InSphere{..}, Near{..}, FaceIds{..}, InGroup{..}, InLayer{..}, InBody{..}, TangentGrow{..} }`; `QueryDigest`.
- **Algorithm:** parse with the CORE.JRN.CANON bounded iterative parser into a value tree, then a schema walk with an explicit stack building the typed AST; canonicalise bottom-up; serialise to the CORE.JRN.CANON value model and digest.
- **Errors:** `bad_query { pointer, reason }`, `query_limit`, `bad_quantity` (from GEO.FRAME.UNITS).
- **Determinism:** D0.
- **Performance budget:** parse and canonicalise a 64 KiB query under 5 ms [unverified].
- **Crate:** `fid-geo-sel` (L0, KNP-1 applies; depends on `fid-canon` and `fid-geo-frame`).

## Test fixtures

- `valid_v1.json`, `equivalent_pairs.json`, `invalid_v1.json`: hand-written JSON with committed canonical vectors; fuzz seed 37. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-sel` (L0, no I/O, KNP-1), add module `query::ast` and `query::parse`. Define the closed schema v1: `{ v: 1, domain, where: Expr }`, `Expr = and[] | or[] | not | exactly one predicate`, predicates `all`, `ids`, `attr_range`, `quality_flags`, `normal_cone`, `in_box`, `in_sphere`, `near`, `face_ids`, `in_group`, `in_layer`, `in_body`, `tangent_grow`, with lengths and angles as `{ text, unit }` parsed by `fid-geo-frame` units. Parse with `fid-canon`'s bounded parser (64 KiB, depth 32) then an explicit-stack schema walk: any unknown key, type error, missing field, multiple predicates in one object or version ≠ 1 gives `bad_query { pointer, reason }`; > 1 024 nodes or > 10 000 IDs per list gives `query_limit`. Strings are never interpreted (C6). Canonicalise (flatten and/or, drop double not, sort and dedupe operands by canonical bytes, sort ID lists, keep quantity text, unit and SI bits) and digest with `fid-canon`. Acceptance: AC-1 vectors; AC-2 equivalence and non-equivalence; AC-3 80 invalid inputs with pointers; AC-4 limits without stack overflow; AC-5 10⁶-input fuzz without panic. Constraints: CONSTITUTION C6, §8 (no panic, no `unsafe`, no recursion on untrusted depth). Files: `crates/fid-geo-sel/src/query/ast.rs`, `src/query/parse.rs`, `src/query/canon.rs`, `tests/query_grammar.rs`, `fixtures/geo/sel/query/`.

## Open questions

- A compact text syntax for the CLI is deliberately not part of v1 (the parent's open question chose the JSON AST for C6). A CLI convenience could read the JSON from a file handle in an approved import root; a text form would need its own decision and bounded parser.

## Children

_Leaf: none._
