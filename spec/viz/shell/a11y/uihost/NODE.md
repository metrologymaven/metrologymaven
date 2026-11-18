---
id: VIZ.SHELL.A11Y.UIHOST
title: Control-channel UI host, input injection and origin binding
level: 4
parent: VIZ.SHELL.A11Y
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.A11Y-003, REQ-VIZ.SHELL.A11Y-004]
covers: []
depends_on: [VIZ.SHELL.A11Y.AXTREE, AUTO.CTRL.UIOPS, AUTO.CTRL.TRACE, CORE.BUS.PRIN]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.A11Y.UIHOST: Control-channel UI host, input injection and origin binding

## Purpose

The shell's implementation of the `UiHost` trait defined by `AUTO.CTRL.UIOPS` (and extended by `AUTO.CTRL.TRACE`): serve the inspection projection of the widget tree, inject pointer, key and text events into the app's own windows, and make sure any registry command an injected event triggers runs under the control-channel session's principal, never as `human`. It also provides the input tap, the secret-widget flag and the recording-indicator hook that traces need.

## Scope

**In:** `UiHost` implementation (`tree`, `inject`, `owns_window`); injected-event queue processed at the start of a frame; `InputOrigin` propagation from event to `ShellBus` submissions; keyboard-reachability guard; input tap for trace recording; secret flag from AXTREE attributes; indicator hook.
**Out:** the trait, protocol, rate limits and auth (`AUTO.CTRL.UIOPS`, `AUTO.CTRL.TRACE`, `AUTO.CTRL.AUTH`); principal kinds (`CORE.BUS.PRIN`).

## Requirements

- **REQ-VIZ.SHELL.A11Y.UIHOST-001** `UiHost::tree` shall return the inspection projection of the current frame's `AxTree`, or the subtree under a given widget ID, with ID, role, name, enabled flag, bounds in window pixels and all attributes.
- **REQ-VIZ.SHELL.A11Y.UIHOST-002** Injected events shall be delivered at the start of the next frame, in order, only to widgets that exist, are enabled and belong to the app's own windows.
- **REQ-VIZ.SHELL.A11Y.UIHOST-003** Every registry command submitted while handling an injected event shall be submitted with that event's `InputOrigin`, so the bus assigns the channel session's principal and never `human`.
- **REQ-VIZ.SHELL.A11Y.UIHOST-004** Injected events shall reach only actions that are also reachable by keyboard (widgets with a focusable node or a key binding), and an injected event targeting any other action shall be rejected with `input_target_invalid`.
- **REQ-VIZ.SHELL.A11Y.UIHOST-005** The host shall offer an input tap that reports every human and injected UI input event with widget ID and kind, redacting text for widgets whose `secret` attribute is set.
- **REQ-VIZ.SHELL.A11Y.UIHOST-006** The host shall show a visible indicator while a control-channel session is injecting input or recording a trace.

## Acceptance criteria

- **AC-1** Given the test app with `fixtures/projects/min.fid`, when the `AUTO.CTRL.UIOPS` conformance tests run against the real host instead of the fake, then AC-1, AC-4, AC-5 and AC-6 of that node pass.
- **AC-2** Given an agent channel session clicking a button bound to a `human_only` adoption command, then the bus refuses it with `policy_refused` and the journal is unchanged.
- **AC-3** Given a W2 command triggered by an injected click, then its journal entry's principal is the channel session's principal.
- **AC-4** (error path) Given an injected click on a pointer-only widget without a focusable node or key binding, then `input_target_invalid`.
- **AC-5** Given a text entry into a widget marked `secret`, then the tap reports the event with redacted text.

## Design

- **Registry command(s):** none.
- **Data structures:** `ShellUiHost` implementing `UiHost`; `PendingInput { event, origin: InputOrigin }` queue; `ShellBus::submit_with_origin(cmd, Option<InputOrigin>)`.
- **Algorithm:** frame start → drain queue → validate target (exists, enabled, own window, keyboard-reachable) → translate to toolkit input → widget handlers call `ShellBus` with the active origin (scoped origin stack per event) → tap reports.
- **Errors:** `widget_not_found`, `input_target_invalid`.
- **Determinism:** N.
- **Performance budget:** inspection of 10^4 widgets under 50 ms (shared budget with `AUTO.CTRL.UIOPS`) [unverified].
- **Crate:** `fid-shell` (module `a11y::host`).

## Test fixtures

`fixtures/projects/min.fid`; `fixtures/ctrl/ui/tree-small.json` scenarios replayed against the real host.

## Build prompt

> In crate `fid-shell`, add `a11y::host` implementing the `UiHost` trait from `fid-ctrl` (`tree`, `inject`, `owns_window`, plus the trace extensions: input tap, secret flag, indicator hook). `tree` serves the inspection projection of the frame's `AxTree`. Injected events are queued, validated (exists, enabled, own window, keyboard-reachable; else `input_target_invalid`) and delivered at the next frame start; every command submitted while handling one goes through `ShellBus::submit_with_origin` carrying the event's `InputOrigin`, so the bus assigns the channel principal, never `human`. Show an indicator while a session injects or records. Tests AC-1..AC-5. Rules: CONSTITUTION §3 (human_only commands refused for non-human principals), §8; C3, C6.

## Open questions

- **Revise for D-035 (accepted 2026-11-17):** pass an origin token for every injected input event, so commands triggered by automation carry the control channel's principal, never `human`.
- Cross-domain (unresolved): `CORE.BUS.PRIN` REQ-005 assigns `human` to any GUI-widget call in an interactive session. This leaf needs the bus to accept an injected-input origin token from the control channel and map it to the channel's principal; `CORE.BUS.PRIN` must add that before this leaf can pass AC-2 and AC-3.

## Children

_Leaf: none._
