---
id: VIZ.SHELL.DOCK.RAIL
title: Workflow rail driven by the dependency graph
level: 4
parent: VIZ.SHELL.DOCK
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.DOCK-004]
covers: []
depends_on: [VIZ.SHELL.DOCK.HOST, CORE.PRJ.TREE, CORE.DEP.PROP, GEO.SEL.QUERY]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.DOCK.RAIL: Workflow rail driven by the dependency graph

## Purpose

A non-modal stepper (import, align, inspect, report, for example) whose step completion is read from the project's element tree and dependency-graph statuses, never from UI-local flags, so it is correct after reopening, undo or replay. Each step links to the registry command that starts it; the rail never runs anything itself.

## Scope

**In:** `RailSpec` data (steps with title message ID, completion query, start command ID) supplied by the app; evaluation of step state (`todo`, `done`, `outdated`, `blocked`); rendering; activating a step opens the palette form of its start command.
**Out:** the step lists per app (`ECO.APPS.*`, supplied as data); command forms (`VIZ.SHELL.PALETTE.FORMS`).

## Requirements

- **REQ-VIZ.SHELL.DOCK.RAIL-001** Step state shall be computed from the element tree and dependency statuses through the step's semantic completion query, and the rail shall hold no completion flag of its own.
- **REQ-VIZ.SHELL.DOCK.RAIL-002** A step whose matching elements exist but are outdated shall show `outdated`, never `done`.
- **REQ-VIZ.SHELL.DOCK.RAIL-003** Activating a step shall open the argument form of its start command and shall not invoke any command directly.
- **REQ-VIZ.SHELL.DOCK.RAIL-004** Evaluating the same `RailSpec` on the same document state shall give the same step states.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/rail-4step` and project `scene-doc-small` with an alignment, then steps 1–2 are `done` and 3–4 `todo`.
- **AC-2** Given the import changed so the alignment is outdated, then step 2 shows `outdated`.
- **AC-3** Given an undo of the alignment, then step 2 returns to `todo` with no UI action.
- **AC-4** Given activation of step 3, then the palette form of its start command opens and the journal is unchanged.

## Design

- **Registry command(s):** none.
- **Data structures:** `RailSpec { steps: [{ id, title: MessageId, done_when: SemanticQuery, start: CommandId }] }`; `StepState`.
- **Algorithm:** on each change batch, resolve each query through `GEO.SEL.QUERY` on the element tree; any match outdated → `outdated`; none → `todo`; a step whose predecessors are `todo` → `blocked` (advisory, still clickable).
- **Errors:** `QueryInvalid` shows the step as unavailable with a diagnostic.
- **Determinism:** D0.
- **Performance budget:** evaluate 10 steps under 1 ms on 10^4 elements [unverified].
- **Crate:** `fid-shell` (module `dock::rail`).

## Test fixtures

`fixtures/viz/rail-4step` (RailSpec JSON), `fixtures/viz/scene-doc-small`.

## Build prompt

> In crate `fid-shell`, add `dock::rail`: render a non-modal workflow rail from a data `RailSpec` (steps with title message ID, completion semantic query, start command ID). Step state (`todo`, `done`, `outdated`, `blocked`) is computed only from the element tree and dependency statuses on every change batch; activating a step opens the start command's palette form and never runs a command. Tests AC-1..AC-4. Rules: CONSTITUTION §8; C3.

## Open questions

- App manifests (`ECO.APPS.MANIFEST.SCHEMA`) depend on `VIZ.SHELL.DOCK`, so the rail receives its `RailSpec` as data rather than through a code edge to ECO.

## Children

_Leaf: none._
