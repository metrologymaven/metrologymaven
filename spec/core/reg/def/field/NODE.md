---
id: CORE.REG.DEF.FIELD
title: Definition record, enums and policy fields
level: 4
parent: CORE.REG.DEF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-registry]
built_pr: 46
satisfies: [REQ-CORE.REG.DEF-001, REQ-CORE.REG.DEF-007, REQ-CORE.REG.DEF-008, REQ-CORE.REG.DEF-009]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.REG.DEF.FIELD: Definition record, enums and policy fields

## Purpose

The Rust types and the JSON shape of one command definition: every field a domain fills in, every closed enum (side-effect class, MCP projection, determinism, idempotency, `fs_effect`, undo, `network`, `web`) and the policy fields requested across the tree (`human_only` from D-031, `network`, `web`, `web_note`, `cli_bare`). The types make malformed single-field values unrepresentable; rules that span fields or declarations stay with `CORE.REG.VAL`.

## Scope

**In:** `CommandDef` and its enums; field defaults; serialisation to and from JSON with fixed snake_case names; the published `command-def.schema.json`; the per-field invariants that one value can check by itself (for example `web_note` non-empty when `web` is not `available`).
**Out:** schema keywords and the unit facade (`CORE.REG.DEF.SCHEMA`); precondition syntax (`CORE.REG.DEF.PRED`); macros (`CORE.REG.DEF.MACRO`); cross-field and cross-declaration rules (`CORE.REG.VAL`); lifecycle metadata semantics and migrator records (`CORE.REG.VER.LIFE`); canonical bytes and digests (`CORE.REG.MAN`).

## Requirements

- **REQ-CORE.REG.DEF.FIELD-001** The `CommandDef` type shall hold the fields `id`, `major`, `title`, `description`, `params_schema`, `result_schema`, `side_effect`, `mcp_projection`, `determinism`, `idempotency`, `fs_effect`, `undo`, `preconditions` (each an object `{expr, bound, message_key}`: an FPL expression, a step bound 1..=1,000,000 and a message key), `effects`, `algorithms`, `exposure`, `lifecycle`, `human_only`, `network`, `web`, `web_note` and `cli_bare`.
- **REQ-CORE.REG.DEF.FIELD-002** `side_effect` shall take exactly one of R0, R1, W1, W2, W3, X1, I1, D1, M1, S1 or A0; `mcp_projection` one of `exposed`, `request_only` or `never`; `determinism` one of D0, D1, D2, D3 or N.
- **REQ-CORE.REG.DEF.FIELD-003** `fs_effect` shall take exactly one of `none`, `read_root` or `write_root`, and `undo` exactly one of `head_move` or `not_undoable`.
- **REQ-CORE.REG.DEF.FIELD-004** `idempotency` shall take exactly one of `natural` (repeating the call has no further effect), `keyed` (the caller supplies an idempotency key) or `none`.
- **REQ-CORE.REG.DEF.FIELD-005** `human_only` shall be a boolean defaulting to `false`, and when it is `true` the definition shall report its effective MCP projection as the stricter of its declared projection and `request_only`.
- **REQ-CORE.REG.DEF.FIELD-006** `network` shall take exactly one of `none`, `egress` (data leaves the machine through `egress.send@1`) or `device` (talks to an instrument or driver host), defaulting to `none`.
- **REQ-CORE.REG.DEF.FIELD-007** `web` shall be required and take exactly one of `available`, `partial` or `unavailable`, and if `web` is `partial` or `unavailable` with an empty or missing `web_note`, then constructing the definition shall fail with `web_note_missing`.
- **REQ-CORE.REG.DEF.FIELD-008** `cli_bare` shall be a boolean defaulting to `false`; when `true` it marks the command that the CLI generator also emits as the bare `fid <domain>` form (CONSTITUTION §3.1).
- **REQ-CORE.REG.DEF.FIELD-009** The definition type shall have no licence-tier, entitlement or edition field, and JSON input carrying any unknown top-level field shall be rejected with `unknown_field`.
- **REQ-CORE.REG.DEF.FIELD-010** Serialising a `CommandDef` to JSON and parsing it back shall give an equal value, and the JSON shall validate against the published `command-def.schema.json`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/reg/def/field/defs_valid.json` (one definition per side-effect class, each enum value used at least once), when each is parsed and re-serialised, then the value round-trips equal and validates against `command-def.schema.json`.
- **AC-2** (error path) Given `fixtures/core/reg/def/field/defs_invalid/` (one file per case: `bad_side_effect`, `bad_fs_effect`, `bad_undo`, `bad_network`, `web_missing`, `web_note_missing`, `entitlement_field`, `unknown_field`), when each is parsed, then it fails with the error code named in its `expect` field.
- **AC-3** Given a W2 definition with `human_only: true` and `mcp_projection: exposed`, when its effective projection is queried, then it is `request_only`; with `mcp_projection: never` it stays `never`.
- **AC-4** Given a definition with no `network`, `human_only` or `cli_bare` field, when parsed, then they default to `none`, `false` and `false`.
- **AC-5** Given the type definitions, when `command-def.schema.json` is regenerated in CI, then it is byte-identical to the committed file.

## Design

- **Registry command(s):** none; library types.
- **Data structures:** `CommandDef { id: CommandId, major: u32, title: String, description: String, params_schema: SchemaDoc, result_schema: SchemaDoc, side_effect: SideEffect, mcp_projection: Projection, determinism: Determinism, idempotency: Idempotency, fs_effect: FsEffect, undo: Undo, preconditions: Vec<Precondition>, effects: Vec<EventType>, algorithms: Vec<AlgorithmId>, exposure: BTreeSet<Surface>, lifecycle: LifecycleRef, human_only: bool, network: Network, web: Web, web_note: Option<String>, cli_bare: bool }`. `Surface { Cli, Mcp, Python, Gui, Control }`. `Precondition { expr: String, bound: u32, message_key: String }` (JSON `{"expr": ..., "bound": N, "message_key": "..."}`, all required; bound 1..=1,000,000; message key shaped like a command ID; a malformed entry is `bad_precondition`). `SchemaDoc` and the precondition expression are opaque here and checked by SCHEMA and PRED. `LifecycleRef` holds state and metadata whose rules live in `CORE.REG.VER.LIFE`.
- **Algorithm:** serde-style derive with `deny_unknown_fields`; enums serialise as fixed strings; `effective_projection()` = min(declared, `request_only`) when `human_only`, ordering `exposed` > `request_only` > `never`.
- **Errors:** `unknown_field`, `bad_enum{field,value}`, `bad_precondition`, `web_missing`, `web_note_missing`.
- **Determinism:** D0; `BTreeSet`/`Vec` only, no hash maps.
- **Performance budget:** parse 5,000 definitions in < 200 ms [unverified until measured].
- **Crate:** `fid-registry` (module `def`).

## Test fixtures

Hand-written JSON under `fixtures/core/reg/def/field/`, CC-BY-4.0 with the spec; each invalid file carries an `expect` field.

## Build prompt

> In Rust crate `fid-registry`, add module `def` with `CommandDef` and its enums exactly as listed: fields id, major, title, description, params_schema, result_schema, side_effect (R0 R1 W1 W2 W3 X1 I1 D1 M1 S1 A0), mcp_projection (exposed, request_only, never), determinism (D0 D1 D2 D3 N), idempotency (natural, keyed, none), fs_effect (none, read_root, write_root), undo (head_move, not_undoable), preconditions, effects, algorithms, exposure (set of cli, mcp, python, gui, control), lifecycle, human_only (bool, default false), network (none, egress, device; default none), web (available, partial, unavailable; required), web_note (required non-empty unless web is available), cli_bare (bool, default false). Reject unknown top-level fields; there is no entitlement field. Add `effective_projection()` that tightens to request_only when human_only (CONSTITUTION §3, D-031). Generate `schemas/command-def.schema.json` from the types and check it in CI (byte-identical). Tests: AC-1..AC-5 with fixtures in `fixtures/core/reg/def/field/`. Constraints: CONSTITUTION §8 (no panic, no `unsafe`, deny unwrap/expect), BTreeMap/BTreeSet only, clean-room.

## Open questions

- Cross-field rules (human_only with a looser projection, one `cli_bare` per domain, `network: egress` only on X1, `network: device` only on I1, D1 or M1) belong to `CORE.REG.VAL`; that leaf needs new rule codes.
- `AUTO.CLI` must learn to emit the bare form for `cli_bare` commands (raised by `PLAT.DOCTOR`).

## Children

_Leaf: none._
