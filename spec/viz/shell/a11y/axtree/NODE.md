---
id: VIZ.SHELL.A11Y.AXTREE
title: Widget tree, stable widget IDs, roles and platform exposure
level: 4
parent: VIZ.SHELL.A11Y
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.A11Y-001, REQ-VIZ.SHELL.A11Y-003]
covers: []
depends_on: [VIZ.SHELL.DOCK.HOST]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.A11Y.AXTREE: Widget tree, stable widget IDs, roles and platform exposure

## Purpose

One semantic widget tree per frame, built by the shell from what every panel declares, with two projections: the platform accessibility API (through AccessKit under the provisional toolkit) and the control channel's UI inspection data. It owns the stable widget-ID convention (needed because egui IDs are per-frame hashes), the role and attribute vocabulary that acceptance checks and automation rely on, and the per-frame uniqueness check.

## Scope

**In:**
- `WidgetId` convention: `<panel id>/<stable key>/...`, with keys supplied by widget code from entity IDs, command IDs or fixed names, never from positions or frame counters
- `AxNode { id, role, name, state, value, bounds, actions, attrs, children }`
- Metrology roles: `value_of_record`, `not_decidable`, `catalogued_state`, `explanation`, `status_chip`, plus standard roles (button, tree item, table cell, text field, panel)
- Attributes: `command` (registry command ID of an action), `reason_code`, `standards_ref` (designator, edition, clause; never text), `basis` (ResultRecord `evaluation_basis` of a displayed result), `entity_id`, `message_id` (catalog message ID of displayed text), `secret` (text redacted in traces)
- AccessKit mapping on Windows and macOS; attributes not representable in the platform API go to the inspection projection only
- Per-frame duplicate-ID detection

**Out:** input injection and the `UiHost` implementation (`VIZ.SHELL.A11Y.UIHOST`); describe-view text (`VIZ.SHELL.A11Y.DESCR`); which panels set which roles (each panel leaf, e.g. `VIZ.SHELL.TREE.RESULTS`); contrast and scaling (`VIZ.SHELL.THEME`); web viewer accessibility (`VIZ.WEB`).

## Requirements

- **REQ-VIZ.SHELL.A11Y.AXTREE-001** Every panel, control and tree item shall appear in the widget tree with a role, a name and its state (enabled, focused, selected, expanded, checked as applicable), and shall be exposed to the platform accessibility API on Windows and macOS [unverified].
- **REQ-VIZ.SHELL.A11Y.AXTREE-002** Widget IDs shall be built only from the panel ID and stable keys, so that the same widget for the same entity or command has the same ID across frames, sessions and machines.
- **REQ-VIZ.SHELL.A11Y.AXTREE-003** If two widgets in one frame share an ID, then the tree builder shall keep the first, suffix the second with `#dup<n>`, and record a `DuplicateWidgetId` diagnostic that fails CI tests.
- **REQ-VIZ.SHELL.A11Y.AXTREE-004** Every actionable widget that triggers a registry command shall carry the `command` attribute with that command ID.
- **REQ-VIZ.SHELL.A11Y.AXTREE-005** Widgets displaying a value of record, a NOT_DECIDABLE outcome or a state with a catalogued reason shall carry the roles `value_of_record`, `not_decidable` or `catalogued_state`, and `reason_code`, `basis` and `entity_id` attributes where applicable.
- **REQ-VIZ.SHELL.A11Y.AXTREE-006** Explanation widgets shall carry `standards_ref` as structured designator, edition and clause, and every widget showing catalog text shall carry the `message_id` attribute.
- **REQ-VIZ.SHELL.A11Y.AXTREE-007** The control-channel inspection projection and the platform projection shall be produced from the same `AxNode` tree in the same frame.

## Acceptance criteria

- **AC-1** Given the test app with fixture `fixtures/projects/min.fid` and the demo panels, when the inspection projection is taken twice in separate sessions, then all widget IDs are identical and match `fixtures/viz/a11y/ids-min.json`.
- **AC-2** Given a results row for a NOT_DECIDABLE characteristic with reason `decide.rule_missing`, then its node has role `not_decidable`, `reason_code = decide.rule_missing`, `basis`, `entity_id`, and its "Why?" action carries `command`.
- **AC-3** (error path) Given a test panel emitting two widgets with key `ok`, then the second is `.../ok#dup1` and a `DuplicateWidgetId` diagnostic is recorded.
- **AC-4** Given Windows UI Automation (or macOS NSAccessibility) queried in CI on the test app, then every node of the platform projection has a role and a non-empty name [unverified: CI runner accessibility support].
- **AC-5** Given the same frame, then the node sets of both projections are equal by ID.

## Design

- **Registry command(s):** none.
- **Data structures:** `WidgetId(String)`; `AxNode`; `AxRole` (standard + metrology roles); `AxAttrs { command: Option<CommandId>, reason_code, standards_ref: Option<{ designator, edition, clause }>, basis: Option<EvaluationBasis>, entity_id, message_id, secret: bool }`; `AxTree` built per frame; `PanelUi` helpers `ax_value_of_record(...)`, `ax_action(command, ...)` so panels cannot forget attributes.
- **Algorithm:** panels declare nodes during layout; the builder assembles the tree in panel then declaration order; maps to AccessKit `Node`s (role mapping table; metrology roles map to the nearest platform role plus a description); inspection projection serialises all attributes.
- **Errors:** `DuplicateWidgetId` (diagnostic).
- **Determinism:** D1 (tree order depends on layout of the platform toolkit; IDs are D0).
- **Performance budget:** build tree of 10^4 nodes under 3 ms on H1 [unverified].
- **Crate:** `fid-shell` (module `a11y::tree`).

## Test fixtures

`fixtures/projects/min.fid`; `fixtures/viz/a11y/ids-min.json` (expected IDs), project-authored.

## Build prompt

> In crate `fid-shell`, add `a11y::tree`: a per-frame semantic `AxTree` of `AxNode { id, role, name, state, value, bounds, actions, attrs, children }` built from panel declarations through `PanelUi` helpers. Widget IDs are `<panel id>/<stable key>/...` from entity IDs, command IDs or fixed names only; duplicates get `#dup<n>` and a `DuplicateWidgetId` diagnostic. Roles include the standard set and metrology roles `value_of_record`, `not_decidable`, `catalogued_state`, `explanation`, `status_chip`; attributes `command`, `reason_code`, `standards_ref` (designator/edition/clause only), `basis`, `entity_id`, `message_id`, `secret`. Project the same tree to AccessKit (Windows, macOS) and to the control-channel inspection format. Tests AC-1..AC-5. Rules: CONSTITUTION §5 (cite standards by designator only), §8; never put standards text in attributes.

## Open questions

- AccessKit coverage on Linux is weaker [unverified]; PLAT decides Linux support tier for accessibility.

## Children

_Leaf: none._
