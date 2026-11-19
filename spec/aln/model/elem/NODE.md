---
id: ALN.MODEL.ELEM
title: Alignment element type and dependency-graph registration
level: 3
parent: ALN.MODEL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-align]
built_pr: 230
satisfies: [REQ-ALN.MODEL-001, REQ-ALN.MODEL-002]
covers: []
depends_on: [CORE.PRJ, CORE.DEP, GEO.FRAME]
side_effect: R0
determinism: D0
cli: [fid align list-alignments, fid align get-alignment]
mcp: [align_list_alignments, align_get_alignment]
journal: [align.list_alignments@1, align.get_alignment@1]
updated: 2026-11-19
---

# ALN.MODEL.ELEM: Alignment element type and dependency-graph registration

## Purpose

Defines the `Alignment` element that every alignment solver produces, the solver-facing contract that turns a solver output into an element, the element's content digest, and its registration in CORE's dependency graph so that it recomputes when its inputs change. It also offers the two read commands that list and fetch alignments. Hierarchy, activation, library, and adoption build on this type in sibling leaves.

## Scope

**In:**
- The `Alignment` element type and its serialisation (RFC 8785 canonical JSON)
- The `AlignmentSolver` trait (solver-facing contract) and the wrapper that creates or updates an element from a solver output
- Content digest of an alignment
- Dependency edges from inputs, parameters and parent alignment, and the recompute callback CORE.DEP invokes
- `align.list_alignments@1` and `align.get_alignment@1` (R0)

**Out** (and which node owns it instead):
- Parent/child chaining, composition, groups, frames, delete (`ALN.MODEL.HIER`)
- Active alignment and result stamping (`ALN.MODEL.ACTIVE`)
- Save and recall definitions (`ALN.MODEL.LIB`)
- Provenance state transitions and adoption (`ALN.MODEL.ADOPT`); this leaf only stores the provenance fields
- The residual report content (`ALN.QA.REPORT`); this leaf stores only its ID and digest
- The graph algorithm, recompute ordering and recompute journaling (`CORE.DEP`)

## Requirements

- **REQ-ALN.MODEL.ELEM-001** The `Alignment` element shall store: algorithm ID with semver, input references (element ID plus content digest each), parameters (authored value and unit plus SI value), resulting transform (rigid or similarity, GEO.FRAME type), optional parent alignment ID, residual-report ID and digest, provenance (principal ID, principal kind `human` or `agent`, provenance state), and status (`ok`, `not_converged`, `ambiguous`, `failed`, `outdated`).
- **REQ-ALN.MODEL.ELEM-002** When a solver output is wrapped into a new alignment, the system shall assign a UUIDv7 stable ID; when an existing alignment is recomputed, the system shall keep its ID (CONSTITUTION §5).
- **REQ-ALN.MODEL.ELEM-003** The alignment content digest shall be SHA-256 over the RFC 8785 canonical JSON of algorithm ID, input digests, SI parameters, transform, parent ID and digest, and residual-report digest, and shall exclude timestamps and provenance.
- **REQ-ALN.MODEL.ELEM-004** When an alignment is created, the system shall register in CORE.DEP one dependency edge from each input element, each parameter and the parent alignment to the alignment.
- **REQ-ALN.MODEL.ELEM-005** When CORE.DEP recomputes an alignment, the system shall re-invoke the solver named by the stored algorithm ID with the stored inputs and parameters and replace the transform, residual-report reference, status and digest in place.
- **REQ-ALN.MODEL.ELEM-006** If a solver returns a typed error on first creation, then the system shall create no element and journal nothing; if it returns an error during recompute, then the alignment status shall become `failed` and CORE.DEP shall handle dependents.
- **REQ-ALN.MODEL.ELEM-007** If the stored algorithm ID and version is not registered at recompute time, then the system shall set status `failed` with error `method_unavailable` and shall not substitute another version.
- **REQ-ALN.MODEL.ELEM-008** `fid align list-alignments --json` and `fid align get-alignment <id> --json` shall return the same content as the MCP tools `align_list_alignments` and `align_get_alignment`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/model/chain3` (a synthetic project with a mesh, three fitted planes and three alignments, generated with seed 1), when `fid align get-alignment <A2> --json` runs, then the output contains every field of REQ-001 and a digest equal to the fixture's recorded digest.
- **AC-2** Given `chain3`, when the input plane of alignment A1 is replaced with a shifted copy, then CORE.DEP recomputes A1 and A2 in one recompute entry, both keep their IDs, and both digests change.
- **AC-3** (error path) Given a stub solver that returns `degenerate_input`, when the wrapper is called to create an alignment, then no element exists afterwards and the Record Journal length is unchanged.
- **AC-4** (error path) Given `chain3` with A1's algorithm version removed from the test registry, when a recompute runs, then A1 has status `failed` with `method_unavailable` and its dependents are `outdated`.
- **AC-5** Given `chain3`, when the CLI and MCP list commands run, then their JSON payloads are byte-identical after canonicalisation.
- **AC-6** (determinism) Given `chain3` serialised on two platforms, then every alignment digest is identical.

## Design

- **Registry commands:** `align.list_alignments@1` (R0, params: optional `parent`, optional `status` filter; result `[AlignmentSummary]`), `align.get_alignment@1` (R0, param `id`; result `Alignment`). R0 calls go to the Activity Log, not the Record Journal (REQ-CORE.JRN-016).
- **Data structures:** `Alignment { id, algorithm_id, inputs: Vec<InputRef>, params: ParamSet, transform: geo::Transform, parent: Option<AlignmentId>, residual_report: ReportRef { id, digest }, provenance: Provenance { principal, kind, state }, status: AlignmentStatus, digest }`. `trait AlignmentSolver { fn algorithm_id(&self) -> AlgorithmId; fn solve(&self, inputs, params, parent_pose) -> Result<SolverOutput, AlignError>; }` with `SolverOutput { transform, report_ref, status }`. Inputs are ordered by their order in the command call; params are a sorted map.
- **Algorithm:** creation = solve, wrap, digest, register edges, hand to CORE.PRJ for insertion. Recompute = registered callback looked up by algorithm ID in a deterministic (sorted) registry table.
- **Errors:** `method_unavailable`, `input_missing`, `not_found`, solver errors passed through unchanged.
- **Determinism:** D0; serialisation and digest are canonical, no floating-point work beyond copying values.
- **Performance budget:** list of 10,000 alignments in under 50 ms on reference hardware [unverified until measured].
- **Crate:** `fid-align` (document-model layer above the kernel; crate name provisional).
- **Pinned in implementation (`fid-align`, layer `engine` because it uses `fid-dep`):**
  - *Types reused:* transform = `fid_geo_frame::Transform` (validated rigid or similarity); parameters = `fid_geo_frame::AuthoredQuantity` (authored text, unit symbol, SI value from one rounding; length and angle units only, an unknown unit symbol is `unknown_kind`); algorithm ID = `fid_kci::AlgorithmId` (`module.feature.method@major.minor.patch`); digests = `fid_types::Digest`; IDs = `fid_uuid::Uuid` drawn as UUIDv7 through `fid_project::IdService`; edges = `fid_dep::EdgeIndex` / `EdgesSet`; command records = `fid_registry::CommandDef`.
  - *Element form* (RFC 8785, every key always present): `algorithm`, `digest`, `failure` (null, or the error code exactly when `status` is `failed`), `id`, `inputs` (`[{digest, id}]` in command order), `params` (`{name: {si, text, unit}}`, names `^[a-z][a-z0-9_]*$` at most 64 bytes, at most 64 params), `parent` (null or `{digest, id}`), `provenance` (`{kind: human|agent, principal, state: human|proposed|adopted}`), `report` (`{digest, id}`), `status`, `transform` (`{kind: rigid, r: 3x3 row-major, t: [3]}` or `{kind: similarity, r, s, t}`). Every float is `"0x"` + 16 lowercase hex digits of its IEEE 754 bits. Decoding is strict: exact key set, canonical lowercase IDs (nil and max refused), known enumerations (`unknown_kind` otherwise), finite floats (`non_finite`), transform re-validated by its GEO.FRAME constructor (`degenerate_transform`), param text re-converted to the stored SI bits, digest recomputed (`digest_mismatch`), at most 1 MiB and depth 8, duplicate keys refused.
  - *Content digest* (REQ-003): SHA-256 of canonical `{"algorithm", "inputs": [input digests in command order], "params": {name: SI bits}, "parent": null | {digest, id}, "report": report digest, "transform"}`. Status, failure, provenance, the element ID and input IDs are not hashed.
  - *Parent reference:* the parent is stored as `{id, digest}` so its digest can be hashed (REQ-003); `list_alignments` summaries show only the parent ID.
  - *Provenance on creation:* a human principal gives state `human`, an agent gives `proposed` (ALN.MODEL.ADOPT REQ-001/002); an agent can never hold state `human`. Principal IDs: 1..=128 printable ASCII, no space, no upper case.
  - *Creation order* (all or nothing): budget (65,536 alignments) -> provenance -> inputs (at most 1024; each known to the project and the edge index, else `input_missing`; no repeats, `duplicate_input`) -> solver for exactly the ID and version (`method_unavailable`) -> parent (an alignment, else `not_found`; status `ok`, else `parent_unusable`) -> solve (solver error returned unchanged) -> draw the UUIDv7 (`id_collision` if taken) -> digest -> one `EdgesSet` from the inputs and parent -> insert. Nothing before the solve draws an ID observation. The caller journals the returned element and `EdgesSet`; on error there is nothing to journal.
  - *Solver contract:* `solve(inputs: &[InputRef], params: &ParamSet, parent_pose: Option<&Transform>) -> Result<SolverOutput, SolverError>`; `SolverOutput.status` is `ok`, `not_converged` or `ambiguous` (only the system sets `failed` and `outdated`); solver error codes match `^[a-z][a-z0-9_]*$`, at most 64 bytes; a nil or max report ID is refused.
  - *Recompute:* nodes = the changed elements plus their transitive dependents (at most 65,536); visited in topological order, ties by ascending ID; per alignment: an upstream node failed or outdated in this pass, or the parent is not `ok` -> `outdated`; the parent gone or an input digest unreadable -> `failed`/`input_missing`; no solver for exactly the stored version -> `failed`/`method_unavailable`; solver error -> `failed`/its code; else inputs (fresh digests), parent digest, transform, report, status and digest are replaced in place, ID, algorithm, params and provenance kept. A `failed` or `outdated` alignment keeps its previous content and digest. The pass is one `RecomputeEntry {changed, items: [{id, status, failure, digest_before, digest_after}]}`, committed only when it completes.
  - *Commands:* list result sorted by alignment ID; `parent` and `status` filters both apply; params at most 64 KiB and depth 8, unknown keys `bad_params`, unknown status `unknown_kind`. CLI `fid align list-alignments [--parent <id>] [--status <s>] [--json]` and `get-alignment <id> [--json]` and MCP `align_list_alignments` / `align_get_alignment` build the same typed params and call one handler that returns canonical bytes; each call is appended to the caller's Activity Log, never the Record Journal.
  - *Error codes:* `method_unavailable`, `input_missing`, `not_found`, solver codes unchanged, plus `duplicate_input`, `parent_unusable`, `id_collision`, `bad_params`, `bad_element`, `unknown_kind`, `non_finite`, `degenerate_transform`, `bad_quantity`, `digest_mismatch`, `duplicate_algorithm`, `input_limit`, edge and ID errors passed through, `internal`.
  - *Parameters and edges (REQ-004):* parameters are values stored inside the alignment, not separate entities, so there is no entity to draw a parameter edge from; a parameter change is an update of the alignment itself, whose dependents CORE.DEP propagates. Edges are drawn from each input and the parent.

## Test fixtures

`fixtures/aln/model/chain3`: produced by a seeded generator in the test suite (seed 1); one 10,000-triangle synthetic block mesh, three LSQ planes, three alignments chained A0 to A1 to A2 using a stub solver that returns a fixed transform. Project-generated, CC-BY-4.0.

As built: `fixtures/aln/model/make_fixtures.py` (stdlib Python, `--check`) is the independent reference. Seed 1 drives SplitMix64 for the UUIDv7 observations. The mesh is a stand-in element with a content digest, not 10,000 triangles, and the three planes are given by origin and normal (no LSQ fit), because the stub solver reads only input digests and parameters. The stub `aln.stub.fixed` returns R = [[0,-1,0],[1,0,0],[0,0,1]] and t = (tx, ty, tz); version 1.0.0 is rigid (A0, A2), 1.1.0 a similarity with s = 1.5 (A1). The script writes `project.txt`, `expected.txt` (IDs, digests, digests after the 1 mm shift of P1), `get-a2.json` and `list.json` (expected output bytes).

## Build prompt

> In crate `fid-align`, implement the `Alignment` element type, the `AlignmentSolver` trait, the wrapper that turns a solver output into an element, and the registry commands `align.list_alignments@1` and `align.get_alignment@1` (both side-effect class R0; CLI `fid align list-alignments|get-alignment` with `--json`; MCP `align_list_alignments`, `align_get_alignment`).
> Requirements: the element stores algorithm ID with semver, input references with digests, parameters (authored value and unit plus SI value), rigid or similarity transform (use the GEO frame transform type), optional parent ID, residual-report ID and digest, provenance (principal, kind human or agent, state) and status (ok, not_converged, ambiguous, failed, outdated). New elements get UUIDv7 IDs; recompute keeps the ID. The content digest is SHA-256 over RFC 8785 canonical JSON of algorithm ID, input digests, SI params, transform, parent ID and digest, report digest; no timestamps or provenance. On creation, register one CORE dependency edge per input, parameter and parent, and register a recompute callback keyed by algorithm ID. A solver error on creation creates nothing and journals nothing; on recompute it sets status failed. An unregistered algorithm version at recompute gives `method_unavailable`, never a substitute.
> Acceptance: fixture `fixtures/aln/model/chain3` (seeded generator, seed 1, stub solver). Tests: full field round trip and recorded digest; replacing A1's input recomputes A1 and A2 in one recompute entry keeping IDs; stub solver error leaves no element and an unchanged Record Journal; missing algorithm version gives failed plus outdated dependents; CLI and MCP list payloads identical; digests identical across platforms.
> Constraints: no panic, no unwrap/expect, no unsafe; no std HashMap/HashSet (use ordered maps); R0 calls are logged to the Activity Log only; stable IDs per CONSTITUTION §5. Do not implement hierarchy, activation, library or adoption here.

## Open questions

- None blocking. The `ambiguous` status is used by `ALN.PRE.AMBIG`.

## Children

_Leaf: none._
