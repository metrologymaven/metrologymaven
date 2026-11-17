---
id: VIZ.PICK.AGENTSEL
title: Selection read and change notification for agents
level: 3
parent: VIZ.PICK
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.PICK-007]
covers: []
depends_on: [VIZ.PICK.SELSET, AUTO.MCP]
side_effect: R0
determinism: D0
cli: [fid view get-selection]
mcp: [view_get_selection]
journal: [view.get_selection@1]
updated: 2026-11-16
---

# VIZ.PICK.AGENTSEL: Selection read and change notification for agents

## Purpose

Let "this cylinder" mean the same thing in the GUI, CLI and MCP: a read-only command that returns the current GUI selection (entity IDs, types and kernel values) and a change notification that external agents can subscribe to.

## Scope

**In:** `view.get_selection@1`, the selection-changed notification payload, coalescing of rapid changes.
**Out:** selection state and changes (`VIZ.PICK.SELSET`); MCP subscription transport (`AUTO.MCP`).

## Requirements

- **REQ-VIZ.PICK.AGENTSEL-001** `view.get_selection@1` shall return every selected entity's ID, type, name as untrusted text and key kernel values formatted by the kernel formatter.
- **REQ-VIZ.PICK.AGENTSEL-002** When the GUI selection changes, the system shall emit one selection-changed notification carrying the new selection digest, coalescing changes within 100 ms [unverified].
- **REQ-VIZ.PICK.AGENTSEL-003** The command and notification shall contain no screen coordinates, render IDs or GPU data.
- **REQ-VIZ.PICK.AGENTSEL-004** If nothing is selected, then the command shall return an empty list and a fixed empty-selection digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/pick-cyl` with the cylinder selected, when `fid view get-selection --json` runs, then the result lists the cylinder's entity ID, type `Cylinder`, and its kernel diameter string.
- **AC-2** Given an MCP client subscribed to selection changes, when the selection changes five times within 50 ms, then exactly one notification arrives, carrying the final digest.
- **AC-3** Given no selection, then the result is an empty list with the empty-selection digest.
- **AC-4** (schema) Given any result, then it validates against a schema with no pixel, render-ID or buffer keys.

## Design

- **Registry command(s):** `view.get_selection@1`, R0. No parameters. Result: `SelectionView { items: [{ entity, kind, name: UntrustedText, values: [FormattedValue] }], digest }`. Notification `view.selection_changed` with `{ digest }`, delivered through the AUTO subscription mechanism.
- **Algorithm:** read the current selection from `VIZ.PICK.SELSET`; resolve key values per entity kind through domain read APIs; format through the kernel formatter; digest = SHA-256 of the RFC 8785 canonical item list.
- **Errors:** none on read; notification delivery failures are AUTO's.
- **Determinism:** D0.
- **Performance budget:** under 10 ms for 1 000 selected entities [unverified].
- **Crate:** `fid-pick` (module `agentsel`).

## Test fixtures

`fixtures/viz/pick-cyl` (shared with VIZ.PICK.SNAP), in-repo.

## Build prompt

> In crate `fid-pick`, add registry command `view.get_selection@1` (R0; CLI `fid view get-selection`, MCP `view_get_selection`) that returns the current selection's entity IDs, kinds, names (as untrusted text) and key kernel-formatted values, plus a SHA-256 digest of the canonical (RFC 8785) list. Emit a `view.selection_changed` notification with the digest through the AUTO subscription API, coalescing changes within 100 ms. Never include screen coordinates, render IDs or GPU data. Tests: cylinder selection read, coalescing to one notification, empty-selection digest, schema check. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; untrusted text is data.

## Open questions

- The notification mechanism name follows AUTO.MCP once that node fixes it.

## Children

_Leaf: none._
