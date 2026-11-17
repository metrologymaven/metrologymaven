---
id: AUTO.CTRL.TRACE
title: UI action-trace record and playback
level: 3
parent: AUTO.CTRL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.CTRL-005, REQ-AUTO.CTRL-011]
covers: []
depends_on: [AUTO.CTRL.UIOPS, AUTO.SURF.ROOTS]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.CTRL.TRACE: UI action-trace record and playback

## Purpose

Records UI-level input (pointer, keys, text, widget targets, relative timing) as an action trace and plays it back, for GUI tests and demos. A trace is not a program: playback only re-sends UI input through `AUTO.CTRL.UIOPS`, so the document changes only through the registry commands that input triggers, and the semantic journal, not the trace, remains the record.

## Scope

**In:**
- Ops `trace.start`, `trace.stop` and `trace.play`
- Trace format `fid.ui-trace` version 1: widget-targeted events with relative timing
- Recording indicator, redaction of secret fields and a recording size cap
- Playback synchronisation (wait for widget enabled, per-event timeout) and divergence reporting
- Trace transfer inline in messages; reading a trace from a read root via a root handle

**Out:**
- Semantic recording into Command Programs (`AUTO.REC.MACRO`, `AUTO.REC.CONV`)
- Input injection, rate limits and principal binding (`AUTO.CTRL.UIOPS`)
- The visible recording indicator widget (`VIZ.SHELL`, through the `UiHost` trait)
- Root resolution (`AUTO.SURF.ROOTS`)

## Requirements

- **REQ-AUTO.CTRL.TRACE-001** A trace shall contain only a format header (`fid.ui-trace`, version, app version, manifest digest) and UI input events, each with a widget ID, event kind, payload and milliseconds since the previous event, and no document state.
- **REQ-AUTO.CTRL.TRACE-002** While a recording started by `trace.start` is active, the app shall show a visible recording indicator through the host, and `trace.stop` shall return the trace inline in its reply.
- **REQ-AUTO.CTRL.TRACE-003** When a recorded text event targets a widget the host marks secret, the trace shall store the event with its text replaced by a redaction marker.
- **REQ-AUTO.CTRL.TRACE-004** If a recording reaches 10 000 [unverified] events or 1 MiB encoded, then the server shall stop it, return the trace so far and mark it `truncated`.
- **REQ-AUTO.CTRL.TRACE-005** When `trace.play` runs, the server shall inject each event through the `AUTO.CTRL.UIOPS` input path under the session's principal, so the document changes only through registry commands the input triggers.
- **REQ-AUTO.CTRL.TRACE-006** Before injecting each event, playback shall wait until the target widget exists and is enabled, up to a per-event timeout of 5 s [unverified] by default.
- **REQ-AUTO.CTRL.TRACE-007** If a target widget is absent or disabled when the timeout expires, then playback shall stop at that event and report its index, widget ID and reason (`widget_absent` or `widget_disabled`), and shall inject no later event.
- **REQ-AUTO.CTRL.TRACE-008** The app shall read a trace only inline from the request or from a read-root handle resolved by `AUTO.SURF.ROOTS`, and shall never write a trace file itself.
- **REQ-AUTO.CTRL.TRACE-009** If a trace's version is unsupported or its manifest digest differs from the running app's, then `trace.play` shall refuse with `trace_version_unsupported`, or proceed only when the request sets `allow_manifest_mismatch` and report the mismatch in the reply.

## Acceptance criteria

- **AC-1** Given the fake `UiHost` from `fixtures/ctrl/ui/tree-small.json`, when a client sends `trace.start`, the host receives three scripted clicks and one text entry, and the client sends `trace.stop`, then the returned trace validates against `schemas/ui-trace-1.json`, holds four events with the right widget IDs and no document fields, and the host saw the indicator turn on and off.
- **AC-2** Given a text entry into widget `pwd` marked secret, then the recorded event's text is the redaction marker.
- **AC-3** Given fixture trace `fixtures/ctrl/trace/set-offset.json` and the fake host, when `trace.play` runs, then the fake bus receives exactly `demo.set_offset@1` once, under the session principal, and nothing else reaches the document.
- **AC-4** (error path) Given fixture trace `fixtures/ctrl/trace/missing-widget.json` whose third event targets `btn-gone`, when played with a 200 ms timeout, then playback stops with index 2, `btn-gone` and `widget_absent`, and events 3 and later are not injected.
- **AC-5** (error path) Given a trace with version 99, then `trace.play` replies `trace_version_unsupported` and injects nothing; given a manifest-digest mismatch without `allow_manifest_mismatch`, the same.
- **AC-6** Given 10 001 recorded events, then the recording stops at 10 000, the trace is marked `truncated` and the indicator turns off.
- **AC-7** (error path) Given `trace.play { source: "root:in/../x.json" }`, then the reply is `bad_handle` and nothing is read.

## Design

- **Registry command(s):** none; protocol ops only. Traces are test artefacts, not records (C4 is met by the semantic journal of the commands that playback triggers).
- **Data structures:** `UiTrace { format: "fid.ui-trace", version: 1, app_version, manifest_digest, truncated: bool, events: Vec<TraceEvent> }`; `TraceEvent { widget: WidgetId, kind, payload, dt_ms: u32 }`; `PlaybackReport { completed: u32, stopped_at: Option<{ index, widget, reason }>, manifest_mismatch: bool }`.
- **Algorithm:** recording taps the host's input stream (human or injected) for the session's app instance and appends events with saturating `dt_ms`; playback validates header, then for each event polls the host tree until target is present and enabled or timeout, sleeps `dt_ms` scaled by an optional speed factor (0.1 to 10), and injects via UIOPS.
- **Errors:** `trace_version_unsupported`, `trace_malformed`, `widget_absent`, `widget_disabled`, `bad_handle`, `root_escape`, `recording_active` (second `trace.start`).
- **Determinism:** N (UI timing).
- **Performance budget:** recording overhead < 0.1 ms per event [unverified until measured].
- **Crate:** `fid-ctrl` (module `trace`).

## Test fixtures

`fixtures/ctrl/ui/tree-small.json`; `fixtures/ctrl/trace/set-offset.json` and `fixtures/ctrl/trace/missing-widget.json` (project-authored, CC-BY-4.0); `schemas/ui-trace-1.json` published by this node; fake `UiHost` with an input tap and indicator hook.

## Build prompt

> In crate `fid-ctrl`, add module `trace` with control-channel ops `trace.start`, `trace.stop` and `trace.play`. Format `fid.ui-trace` v1: header (format, version, app version, manifest digest, truncated) plus events `{ widget, kind, payload, dt_ms }`; publish its JSON Schema as `schemas/ui-trace-1.json`. Recording: one active recording per session (`recording_active` otherwise), turn the host's recording indicator on, tap the host input stream, replace text for secret widgets with a redaction marker, stop at 10 000 events or 1 MiB and mark `truncated`; `trace.stop` returns the trace inline. Playback: accept a trace inline or as a read-root handle (resolve with `fid_surface::Roots`; never write files), refuse unsupported versions and manifest mismatches unless `allow_manifest_mismatch`; for each event wait until the target widget is present and enabled (default 5 s timeout), else stop and report index, widget and `widget_absent`/`widget_disabled`; inject through the `uiops` input path with the session principal. Acceptance: AC-1..AC-7 with the fixtures above. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; bounded parsing (fail closed); playback changes the document only through registry commands (CONSTITUTION C3).

## Open questions

- The target's question "should traces be journaled like screenshots" is answered no here: traces are test artefacts returned inline, and the app never writes them, so no W1 export command is needed. If users want traces stored in a project, a later `capture`-style command would be needed (owner `VIZ.CAPTURE`).
- `UiHost` needs an input tap, a "secret widget" flag and an indicator hook; these extend the trait defined in `AUTO.CTRL.UIOPS` and must be implemented by `VIZ.SHELL.A11Y`.

## Children

_Leaf: none._
