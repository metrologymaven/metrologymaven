---
id: AUTO.CTRL.UIOPS
title: UI inspection, input driving and screenshots
level: 3
parent: AUTO.CTRL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.CTRL-005, REQ-AUTO.CTRL-009]
covers: []
depends_on: [AUTO.CTRL.EXEC]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: [capture.take_screenshot@1]
updated: 2026-11-17
---

# AUTO.CTRL.UIOPS: UI inspection, input driving and screenshots

## Purpose

The UI-level operations of the channel: read the UI tree (widgets, roles, labels, enabled state, bounds), drive pointer and keyboard input, and take screenshots. This node owns the protocol side and the `UiHost` trait that the app shell implements; it never touches widgets itself. Screenshots are the VIZ capture registry command (`capture.take_screenshot@1`) sent through the bus, so they are journaled like any other; injected input only causes what the same keystrokes by a person would cause, and every state change still goes through registry commands under the channel client's principal, never the human's.

## Scope

**In:**
- Ops `ui.inspect`, `ui.input` (pointer, key, text) and `ui.screenshot`, with their request and reply schemas
- The `UiHost` trait (inspect, inject, window ownership) that the shell implements; widget IDs are opaque strings supplied by the host
- Mapping `ui.screenshot` to `capture.take_screenshot@1` through the `AUTO.CTRL.EXEC` execute path
- Input rate limit and the rule that input goes only to the app's own windows
- Marking text entered by injected input as untrusted and binding triggered commands to the client's principal

**Out:**
- The shell's widget tree, widget-ID convention, accessibility mapping and input injection (`VIZ.SHELL.A11Y`, which implements `UiHost`)
- The screenshot command itself (`VIZ.CAPTURE.SHOT`)
- Action-trace record and playback (`AUTO.CTRL.TRACE`)
- Framing, auth and command execution (`AUTO.CTRL.WIRE`, `AUTO.CTRL.AUTH`, `AUTO.CTRL.EXEC`)

## Requirements

- **REQ-AUTO.CTRL.UIOPS-001** When `ui.inspect` is received, the server shall return the host's UI tree with, for every widget, its host-supplied ID, role, label, enabled flag and bounds in window pixels, and shall change no UI or document state.
- **REQ-AUTO.CTRL.UIOPS-002** When `ui.inspect` names a subtree root ID that the host does not know, the server shall reply `widget_not_found` with that ID.
- **REQ-AUTO.CTRL.UIOPS-003** When `ui.screenshot` is received, the server shall execute `capture.take_screenshot@1` through the same bus path and principal as `command.execute`, and the reply shall carry that command's envelope including the journal entry hash and image digest.
- **REQ-AUTO.CTRL.UIOPS-004** When `ui.input` is received, the server shall pass each event to the host tagged with the session's principal, and the host contract shall require that any registry command the event triggers runs under that principal.
- **REQ-AUTO.CTRL.UIOPS-005** If a `ui.input` event targets a window that does not belong to the app, a disabled widget or an unknown widget ID, then the server shall reject the request with `input_target_invalid` and inject none of its events.
- **REQ-AUTO.CTRL.UIOPS-006** If a session sends more than 100 [unverified] input events per second, then the server shall reject the excess requests with `rate_limited` and inject none of their events.
- **REQ-AUTO.CTRL.UIOPS-007** Text injected with `ui.input` shall reach any command parameter as `Untrusted<String>` (C6).
- **REQ-AUTO.CTRL.UIOPS-008** If injected input triggers a command declared `human_only`, then the command shall be refused with `policy_refused` because the triggering principal is not `human`.

## Acceptance criteria

- **AC-1** Given the fake `UiHost` loaded from `fixtures/ctrl/ui/tree-small.json` (40 widgets), when a client sends `ui.inspect`, then the reply lists 40 widgets with the fixture's IDs, roles, labels, enabled flags and bounds, and the fake host records no mutation.
- **AC-2** (error path) Given `ui.inspect { root: "no-such-id" }`, then the reply is `widget_not_found` naming `no-such-id`.
- **AC-3** Given the test app with fixture project `fixtures/projects/min.fid`, when `ui.screenshot { view: "main", width: 800, height: 600 }` runs, then the reply envelope carries an image digest and journal entry hash, and the agent branch holds one `capture.take_screenshot@1` entry.
- **AC-4** Given the fake host where clicking widget `btn-offset` triggers `demo.set_offset@1` (W2), when a client sends a `ui.input` click on `btn-offset`, then the fake bus receives `demo.set_offset@1` with the session's principal kind, never `human`.
- **AC-5** (error path) Given the fake host where clicking `btn-adopt` triggers `demo.adopt_thing@1` (`human_only: true`), when clicked via `ui.input`, then the command is refused with `policy_refused` and the journal is unchanged.
- **AC-6** (error path) Given a request whose second event clicks a disabled widget or a foreign window handle, then the reply is `input_target_invalid` and the fake host received no event from that request.
- **AC-7** Given 150 single-event `ui.input` requests sent within one second, then at least 50 replies are `rate_limited` and the fake host received exactly the accepted events.
- **AC-8** Given text `"; drop"` typed into a field bound to a string parameter, then the bus receives it wrapped as untrusted.

## Design

- **Registry command(s):** none of its own; `ui.screenshot` invokes `capture.take_screenshot@1` (W2, owned by `VIZ.CAPTURE.SHOT`).
- **Data structures:** `trait UiHost { fn tree(&self, root: Option<&WidgetId>) -> Result<UiNode, UiError>; fn inject(&self, ev: InputEvent, origin: InputOrigin) -> Result<(), UiError>; fn owns_window(&self, w: WindowId) -> bool; }`; `UiNode { id: WidgetId, role, label: String, enabled: bool, bounds: Rect, children: Vec<UiNode> }`; `InputEvent = Pointer { target, kind, pos } | Key { target, key, mods } | Text { target, text }`; `InputOrigin { principal: Principal, session: SessionId }`.
- **Algorithm:** inspect → host tree → serialize in host order; input → validate every event (target known, enabled, own window) before injecting any → per-session token bucket → inject in order with `InputOrigin`; screenshot → build an execute request and reuse the `AUTO.CTRL.EXEC` path.
- **Errors:** `widget_not_found`, `input_target_invalid`, `rate_limited`, plus envelope errors passed through from the bus.
- **Determinism:** N (live UI state and timing).
- **Performance budget:** `ui.inspect` of a 10 k-widget tree serialized in < 50 ms on reference hardware [unverified until measured].
- **Crate:** `fid-ctrl` (module `uiops`).

## Test fixtures

`fixtures/ctrl/ui/tree-small.json` (project-authored fake widget tree, CC-BY-4.0); a fake `UiHost` that maps widget clicks to fixture commands (`demo.set_offset@1`, `demo.adopt_thing@1` with `human_only: true`); the `AUTO.CTRL` test app; `fixtures/projects/min.fid`.

## Build prompt

> In crate `fid-ctrl`, add module `uiops` implementing three control-channel ops on top of the existing execute path. Define `trait UiHost { tree(root); inject(event, origin); owns_window(w) }` with `UiNode { id, role, label, enabled, bounds, children }`, `InputEvent` (pointer, key, text) and `InputOrigin { principal, session }`; the shell implements this trait later, so test with a fake host. `ui.inspect` returns the host tree (or a subtree; `widget_not_found` if unknown) and mutates nothing. `ui.input` validates all events first (known, enabled, own window; else `input_target_invalid` and inject nothing), applies a per-session token bucket of 100 events/s (`rate_limited`), then injects each event with `InputOrigin` carrying the session principal; the host contract is that triggered commands run under that principal, never `human`, so a `human_only` command triggered this way is refused. Text input reaches commands as `Untrusted<String>`. `ui.screenshot` builds a `capture.take_screenshot@1` execute request and reuses the execute path, replying with its envelope. Acceptance: AC-1..AC-8 with `fixtures/ctrl/ui/tree-small.json`, the fake host and the test app. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; messages within the wire's 1 MiB / 16 MiB bounds; CONSTITUTION C3 (state changes only via registry commands) and C6 (untrusted text).

## Open questions

- **Revise for D-035 (accepted 2026-11-17):** injected UI input must carry the control channel's principal via the UI host's origin token; it can never satisfy a human-only gate.
- Cross-domain: `VIZ.SHELL.A11Y` implements `UiHost` and must guarantee REQ-004 (commands triggered by injected input carry the channel principal, not `human` as `CORE.BUS.PRIN` REQ-005 would assign to a GUI widget). Without that, an agent could click an adoption button and act as a human. `CORE.BUS.PRIN` needs an injected-input origin; neither node states it yet.
- Stable widget IDs under egui's immediate mode (D-005) are the host's convention (`VIZ.SHELL.A11Y`); this node treats them as opaque strings.
- `VIZ.SHELL.A11Y` declares `depends_on: [AUTO.CTRL]`, so this node defines the `UiHost` trait instead of depending on VIZ (avoids a dependency cycle).

## Children

_Leaf: none._
