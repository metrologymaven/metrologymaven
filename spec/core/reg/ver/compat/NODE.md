---
id: CORE.REG.VER.COMPAT
title: Compatibility classifier and manifest diff engine
level: 4
parent: CORE.REG.VER
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-registry]
satisfies: [REQ-CORE.REG.VER-004, REQ-CORE.REG.VER-008]
covers: []
depends_on: [CORE.REG.DEF.FIELD, CORE.REG.VER.LIFE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.REG.VER.COMPAT: Compatibility classifier and manifest diff engine

## Purpose

Decides, field by field, whether a change to a command definition is additive or breaking, and compares two whole manifests into a sorted list of added, removed, lifecycle-changed and changed definitions. It is the one implementation behind the build gate (`CORE.REG.VER.GATE`), the `registry.diff_manifests@1` command (`CORE.REG.QRY`) and the server's version rules (`ENT.EVENTS`).

## Scope

**In:** the per-field rule table, schema-level comparison of parameter and result schemas, the diff engine over two manifests, deterministic output.
**Out:** failing the build (`CORE.REG.VER.GATE`); the command surface (`CORE.REG.QRY`); proving that a new default preserves behaviour (replay CI through `CORE.RPL.EXEC`).

## Requirements

- **REQ-CORE.REG.VER.COMPAT-001** The classifier shall mark a change additive only when it adds an optional parameter with a declared default, adds a result field, or changes `title`, `description`, `x-ui` hints, `web`, `web_note` or `cli_bare`.
- **REQ-CORE.REG.VER.COMPAT-002** The classifier shall mark every other change breaking, including any removed or renamed parameter or result field, a changed JSON type, `x-quantity`, `x-unit` or `x-ref`, a narrowed enum or range, a new required parameter, a changed precondition, a changed `side_effect`, `mcp_projection`, `determinism`, `idempotency`, `fs_effect`, `undo` or `network`, and a change of `human_only`.
- **REQ-CORE.REG.VER.COMPAT-003** Each change record shall name the command ID and major, the JSON pointer of the changed field, the old and new values' digests, and the classification.
- **REQ-CORE.REG.VER.COMPAT-004** When two manifests are compared, the diff engine shall list added, removed, lifecycle-changed and changed definitions keyed by (ID, major), sorted by ID then major then pointer.
- **REQ-CORE.REG.VER.COMPAT-005** If the classifier meets a schema construct it has no rule for, then it shall classify the change breaking with reason `unclassified_construct`.
- **REQ-CORE.REG.VER.COMPAT-006** Comparing a manifest with itself shall produce an empty diff.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/reg/ver/compat/pairs/` (at least 40 old/new definition pairs, one per rule, each with an `expect` list of change records), when classified, then each output equals its `expect` list.
- **AC-2** Given `fixtures/core/reg/manifest/v1.json` and `v2.json` (the five-change pair used by `CORE.REG.QRY` AC-3), when diffed, then the result has exactly five entries, the parameter-type change is breaking and the new optional parameter additive.
- **AC-3** Given any fixture manifest diffed with itself, then the diff is empty.
- **AC-4** Given a schema using `if`/`then`/`else`, when changed, then the change is breaking with `unclassified_construct`.
- **AC-5** Given AC-2 run twice, then the outputs are byte-identical.

## Design

- **Registry command(s):** none; library.
- **Data structures:** `Change { command: CommandRef, pointer: JsonPointer, old_digest, new_digest, kind: Additive | Breaking, reason: Reason }`; `ManifestDiff { added, removed, lifecycle_changed, changed: Vec<Change> }`.
- **Algorithm:** match definitions by (ID, major) through ordered maps; compare top-level fields by rule table; compare schemas structurally (properties by name, `required` as sets, enums as sets, numeric bounds by direction). Default values are trusted as declared; replay CI is the check that a default preserves prior behaviour.
- **Errors:** `manifest_invalid` passed through from the parser.
- **Determinism:** D0; ordered maps only.
- **Performance budget:** diff two 5,000-command manifests in < 1 s [unverified until measured].
- **Crate:** `fid-registry` (module `compat`).

## Test fixtures

`fixtures/core/reg/ver/compat/pairs/` and the shared manifests in `fixtures/core/reg/manifest/`; hand-written, CC-BY-4.0.

## Build prompt

> In crate `fid-registry`, add module `compat` with `classify(old: &CommandDef, new: &CommandDef) -> Vec<Change>` and `diff(a: &Manifest, b: &Manifest) -> ManifestDiff`. Additive only: new optional parameter with default, new result field, changed title/description/x-ui/web/web_note/cli_bare. Everything else is breaking (removed or renamed fields, type, x-quantity, x-unit, x-ref, narrowed enum or range, new required parameter, precondition, side_effect, mcp_projection, determinism, idempotency, fs_effect, undo, network, human_only); unknown schema constructs are breaking with `unclassified_construct`. Change records carry command, JSON pointer, old/new value digests and reason; output sorted by ID, major, pointer. Self-diff is empty. Tests AC-1..AC-5. Constraints: CONSTITUTION §3 (breaking change needs a new major), §8, BTreeMap only.

## Open questions

- Widening an enum in a *result* is additive for readers that ignore unknown values but breaking for strict ones; this leaf classifies it breaking until `AUTO` states its readers' policy.

## Children

_Leaf: none._
