---
id: VIZ.SHELL.TREE.PROBLEMS
title: Problems panel with catalogued fix actions
level: 4
parent: VIZ.SHELL.TREE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.TREE-001, REQ-VIZ.SHELL.TREE-005]
covers: []
depends_on: [VIZ.SHELL.DOCK.HOST, VIZ.SHELL.A11Y.AXTREE, VIZ.RENDER.SCENE.STALE]
uses_artifacts: [FLOW.DEADEND.CATALOG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.TREE.PROBLEMS: Problems panel with catalogued fix actions

## Purpose

One list of everything that needs attention: command errors, warnings, stale results and NOT_DECIDABLE or blocked states, each with its reason code and the fix actions from the shipped fix catalogue, so every catalogued state has a fix one activation away. Fix actions submit registry commands (usually by opening their palette form); the panel never fixes anything itself.

## Scope

**In:** problems panel `viz.problems`; problem sources (bus error envelopes, diagnostics, stale count, catalogued states); fix catalogue loader for `fix-catalogue.json` (`fix-catalogue@1`); fix actions with `command` attributes; a `FixService` used by RESULTS and ELTREE to attach the same actions.
**Out:** the catalogue content and its validation rules (`FLOW.DEADEND.CATALOG`); stale computation (`VIZ.RENDER.SCENE.STALE`).

## Requirements

- **REQ-VIZ.SHELL.TREE.PROBLEMS-001** For each displayed state with a catalogued reason code, the panel shall offer each fix of that code as an action that opens or submits the fix command, with role `catalogued_state`, `reason_code` and `command` attributes.
- **REQ-VIZ.SHELL.TREE.PROBLEMS-002** If the fix catalogue file fails to load, then the panel shall still list problems, show "no fix available", and record a diagnostic.
- **REQ-VIZ.SHELL.TREE.PROBLEMS-003** Problem messages shall come from catalog message IDs, and any content strings inside them shall be rendered as untrusted plain text.
- **REQ-VIZ.SHELL.TREE.PROBLEMS-004** Problems shall be ordered by severity, then entity ID, then reason code.

## Acceptance criteria

- **AC-1** Given fixture catalogue `fixtures/flow/fixes/catalogue-valid.json` and a NOT_DECIDABLE result with `decide.rule_missing`, then the problems row shows its fix actions, each with `command` set, reachable in one activation.
- **AC-2** (error path) Given a corrupted catalogue, then problems still list and a diagnostic names the load error.
- **AC-3** Given a stale result and a failed command, then both appear, ordered by severity.

## Design

- **Registry command(s):** none of its own.
- **Data structures:** `Problem { severity, entity: Option<EntityId>, reason_code, message: MessageId, args }`; `FixService { fixes(reason_code) -> &[FixAction] }`.
- **Errors:** catalogue load errors as diagnostics.
- **Determinism:** D0.
- **Performance budget:** 10^4 problems under 5 ms per frame (virtualised).
- **Crate:** `fid-shell` (module `tree::problems`).

## Test fixtures

`fixtures/flow/fixes/catalogue-valid.json` (published by FLOW.DEADEND.CATALOG).

## Build prompt

> In crate `fid-shell`, add `tree::problems`: a problems panel (`viz.problems`) collecting bus errors, diagnostics, stale results and catalogued states (NOT_DECIDABLE, blocked), ordered by severity, entity ID, reason code; load the shipped `fix-catalogue.json` (`fix-catalogue@1`) read-only and attach each reason's fixes as actions (role `catalogued_state`, `reason_code`, `command` attributes) that open or submit the fix command; expose a `FixService` for other panels. Catalogue load failures degrade to "no fix available" with a diagnostic. Messages from catalog IDs; content strings untrusted. Tests AC-1..AC-3. Rules: CONSTITUTION §8; C3, C6.

## Open questions

- None.

## Children

_Leaf: none._
