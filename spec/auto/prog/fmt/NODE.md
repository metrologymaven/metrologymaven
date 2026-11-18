---
id: AUTO.PROG.FMT
title: Program document format, canonical serialisation and step IDs
level: 3
parent: AUTO.PROG
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-program]
built_pr: 61
satisfies: [REQ-AUTO.PROG-001, REQ-AUTO.PROG-002, REQ-AUTO.PROG-003]
covers: [SA.MP.01]
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-18
---

# AUTO.PROG.FMT: Program document format, canonical serialisation and step IDs

## Purpose

Defines what a Command Program is on disk and in memory: a versioned JSON document in RFC 8785 canonical form with a published JSON Schema, a stable UUIDv7 per step, and a closed set of step kinds in which the only state-changing kind is an invocation of a registry command by ID and major. Expressions are stored as source text and parsed by `AUTO.PROG.EXPR`; operator descriptors and parameter declarations are embedded objects validated by their own modules.

## Scope

**In:**
- Document schema `program-v1`: header, parameter declarations, variables, subroutines, main body, step list
- Step kinds: `invoke`, `assign`, `if`, `loop`, `call`, `return`, `label`, `jump`, `operator`, `comment`
- Canonical serialisation, parsing, schema validation, program digest
- Step ID rules: UUIDv7, unique within a program, never reused
- Format version field and refusal of unknown versions

**Out:**
- Expression grammar and typing (`AUTO.PROG.EXPR`)
- Control-flow semantics (`AUTO.PROG.FLOW`)
- Operator descriptor schema (`AUTO.PROG.OPER`), parameter declaration schema (`AUTO.PROG.FAMILY`)
- Cross-checks against the manifest (`AUTO.PROG.CHECK`)

## Requirements

- **REQ-AUTO.PROG.FMT-001** A program shall be a JSON document with `format: "fid-program"` and an integer `format_version`, and if the version is unknown, then parsing shall fail with `format_version_unsupported`.
- **REQ-AUTO.PROG.FMT-002** The stored form of a program shall be RFC 8785 canonical JSON; parsing shall reject non-canonical bytes with `non_canonical`, and serialising a parsed program shall reproduce the input bytes exactly.
- **REQ-AUTO.PROG.FMT-003** Every step shall carry a `step_id` that is a UUIDv7, and if two steps share an ID, then parsing shall fail with `duplicate_step_id`.
- **REQ-AUTO.PROG.FMT-004** The only step kind that names a state-changing action shall be `invoke`, holding a command ID, a major version, arguments (each either a canonical JSON literal or an expression source string) and an optional result variable; the schema shall admit no other field through which a step can change state.
- **REQ-AUTO.PROG.FMT-005** The program digest shall be the SHA-256 of the canonical bytes.
- **REQ-AUTO.PROG.FMT-006** Parsing shall be bounded (16 MiB, nesting depth 64, 100,000 steps, 250,000 JSON objects and 2,000,000 JSON structural tokens [unverified]) and fail closed with `program_too_large` beyond any of those bounds. The object and token bounds keep the in-memory tree under 256 MiB (AC-6); every step, argument wrapper and opaque object counts toward the object bound, so an invoke-heavy program reaches it first (an `invoke` with two arguments costs about 4 objects, which allows about 60,000 such steps), while the 100,000-step bound is reachable only by programs whose steps carry few objects (for example comments, assignments and labels).
- **REQ-AUTO.PROG.FMT-007** The JSON Schema `schema/program-v1.json` shall be generated from the Rust types, and a test shall fail if the checked-in schema differs.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/program/valid/minimal.json` and `fixtures/program/valid/all-kinds.json` (every step kind once), when parsed and serialised, then the output bytes equal the input and the digest equals the fixture's recorded digest.
- **AC-2** (error path) Given `fixtures/program/invalid/whitespace.json` (same content, pretty-printed), then parsing fails with `non_canonical`.
- **AC-3** (error path) Given `fixtures/program/invalid/dup-step.json`, then `duplicate_step_id` naming the ID.
- **AC-4** (error path) Given `fixtures/program/invalid/extra-field.json` where an `assign` step carries a `command` field, then schema validation fails with `unknown_field`.
- **AC-5** (error path) Given `format_version: 2`, then `format_version_unsupported`.
- **AC-6** (fuzz) Given the `cargo fuzz` target `program_parse` for 10 minutes on the corpus `fixtures/program/`, then no crash and no allocation above 256 MiB.

## Design

- **Registry command(s):** none; library `Program::parse(&[u8])`, `Program::to_canonical()`, `Program::digest()`.
- **Data structures:** `Program { format_version, program_id: Uuid, meta, params: Vec<ParamDecl>, variables: Vec<VarDecl>, subroutines: Vec<Subroutine>, main: Vec<Step> }`; `Step { step_id: Uuid, kind: StepKind }`; `StepKind::Invoke { command: String, major: u32, args: BTreeMap<String, Arg>, bind: Option<VarName> }` with `Arg = Literal(CanonicalJson) | Expr(ExprSrc)` (literals let `AUTO.REC.CONV` store recorded arguments without depending on the expression grammar), `Assign { var, expr }`, `If { branches: Vec<(ExprSrc, Vec<Step>)>, else_body }`, `Loop { kind: Counted{var, from, to, step} | While{cond}, max_iterations: u32, body }`, `Call { subroutine, args, bind }`, `Return { expr }`, `Label { name }`, `Jump { label, cond }`, `Operator { descriptor: OperatorDescriptor }`, `Comment { text }`. `ParamDecl` and `OperatorDescriptor` are opaque JSON here, validated by `AUTO.PROG.FAMILY` and `AUTO.PROG.OPER`.
- **Algorithm:** bounded JSON parse → canonical re-serialise → byte compare; serde with `deny_unknown_fields`.
- **Errors:** `format_version_unsupported`, `non_canonical`, `duplicate_step_id`, `unknown_field`, `schema_invalid`, `program_too_large`.
- **Determinism:** D0.
- **Performance budget:** parse and validate a 10,000-step program in < 100 ms [unverified until measured].
- **Crate:** `fid-program`

## Test fixtures

Hand-written programs under `fixtures/program/valid/` and `fixtures/program/invalid/` (project-authored, CC-BY-4.0), with recorded digests.

## Build prompt

> Create crate `fid-program` with the Command Program document model. Types: `Program` (format "fid-program", `format_version` 1, program_id UUIDv7, meta, params, variables, subroutines, main) and `Step { step_id: UUIDv7, kind }` with kinds invoke (command ID, major, args each either `{"literal": <canonical JSON>}` or `{"expr": "<source>"}`, optional bind variable), assign, if, loop (counted or while, each with `max_iterations`), call, return, label, jump, operator (opaque descriptor object), comment. Stored form is RFC 8785 canonical JSON: reject non-canonical input (`non_canonical`), and serialising a parsed program must reproduce the input bytes. Reject unknown fields, unknown format versions, duplicate step IDs. Digest = SHA-256 of canonical bytes. Bounded parsing: 16 MiB, depth 64, 100,000 steps. Generate `schema/program-v1.json` from the types with a test that fails on drift. Add fuzz target `program_parse`. Acceptance: the six cases in the spec. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; `BTreeMap` only; all input hostile.

## Open questions

- Whether a Python authoring view is added on top of this format is D-026; this format stays authoritative either way.

## Children

_Leaf: none._
