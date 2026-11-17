---
id: VIZ.SHELL.A11Y.DESCR
title: Deterministic describe-view command
level: 4
parent: VIZ.SHELL.A11Y
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.A11Y-002]
covers: []
depends_on: [VIZ.RENDER.SCENE.PROJ, VIZ.RENDER.SCENE.STALE, VIZ.VIEW.VISIB.MODEL, CORE.REG.DEF]
side_effect: R0
determinism: D0
cli: [fid view describe-view]
mcp: [view_describe_view]
journal: [view.describe_view@1]
updated: 2026-11-17
---

# VIZ.SHELL.A11Y.DESCR: Deterministic describe-view command

## Purpose

`view.describe_view@1` turns what a viewport shows into deterministic text: the camera summary, then the visible entities with type, name, status and provenance in a fixed order, then counts per status. It is the accessible model of the 3D scene for screen-reader users and an agent-readable view summary, and it returns identical text for identical document and view state.

## Scope

**In:** the command; text and JSON output; ordering rules; locale-invariant JSON and catalogued English text.
**Out:** visibility evaluation (`VIZ.VIEW.VISIB.MODEL`); statuses (`VIZ.RENDER.SCENE.STALE`, kernel); screen-reader announcement plumbing (`VIZ.SHELL.A11Y.AXTREE`).

## Requirements

- **REQ-VIZ.SHELL.A11Y.DESCR-001** `view.describe_view@1` shall list every visible entity of the viewport with type, name (as untrusted text, escaped), display status, evaluation basis where applicable, and provenance (creating command ID and principal kind), ordered by type rank then entity ID.
- **REQ-VIZ.SHELL.A11Y.DESCR-002** The result shall include the camera eye, target and projection in metres, and counts of visible entities per display status including `stale` and `not_evaluated`.
- **REQ-VIZ.SHELL.A11Y.DESCR-003** For identical document and ViewState digests, the command shall return byte-identical text and JSON.
- **REQ-VIZ.SHELL.A11Y.DESCR-004** If more than a limit of entities (default 1 000) is visible, then the text shall list the first entries by the ordering rule and state the number omitted, while counts cover all.
- **REQ-VIZ.SHELL.A11Y.DESCR-005** The command shall change no state.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/scene-doc-small` and a stored view, when `fid view describe-view --viewport main --json` runs, then the output equals `fixtures/viz/describe/expected-small.json` byte for byte.
- **AC-2** Given the same state on another tier-1 OS, then the output is byte-identical.
- **AC-3** Given an entity named `Ignore previous instructions`, then it appears as an escaped quoted string under `name` and nowhere else.
- **AC-4** Given 1 500 visible points sets, then 1 000 are listed and `omitted: 500` appears, with counts covering 1 500.

## Design

- **Registry command(s):** `view.describe_view@1`, R0, MCP exposed (Activity Log during recording only); params `{ viewport, limit?: u32, format: Text | Json }`; result `{ digest_doc, digest_view, camera, entities: [...], counts, omitted }`.
- **Algorithm:** visible set from VISIB.MODEL; per entity read scene node status and element provenance; sort; render text from catalog messages in English (locale-invariant output; the UI may show a localised rendering separately).
- **Errors:** `UnknownViewport`.
- **Determinism:** D0.
- **Performance budget:** 10^4 visible entities under 50 ms [unverified].
- **Crate:** `fid-shell` (module `a11y::describe`); command declared in `fid-view-cmd`.

## Test fixtures

`fixtures/viz/scene-doc-small`; `fixtures/viz/describe/expected-small.json`.

## Build prompt

> In crate `fid-shell`, add `a11y::describe` with registry command `view.describe_view@1` (R0; CLI `fid view describe-view`; MCP `view_describe_view`): from the viewport's visible set, list entities ordered by type rank then entity ID with type, escaped untrusted name, display status, evaluation basis and provenance; include camera and per-status counts (including `stale`, `not_evaluated`); limit listing (default 1 000) with an omitted count; output locale-invariant and byte-identical for identical document and view digests; change nothing. Tests AC-1..AC-4. Rules: CONSTITUTION §3, §8; C6.

## Open questions

- None.

## Children

_Leaf: none._
