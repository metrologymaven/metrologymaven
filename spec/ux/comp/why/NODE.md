---
id: UX.COMP.WHY
title: The "Why?" panel spec
level: 3
parent: UX.COMP
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.COMP-001, REQ-UX.COMP-002, REQ-UX.COMP-003, REQ-UX.COMP-006, REQ-UX.COMP-007]
covers: []
depends_on: [UX.COMP.SCHEMA, UX.COMP.DATA]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.COMP.WHY: The "Why?" panel spec

## Purpose

The panel that explains any value of record: what the value is, which algorithm and version produced it, from which inputs (with digests), under which decision rule with which outcome, with which uncertainty class, and which journal entry recorded it, each linked to its source entity. It is the visible form of the principle "every number explainable".

## Scope

**In:** `components/why.md` in `component-spec@1`, including section order, empty and unavailable states, and copy-to-clipboard of IDs and digests.
**Out:** computing the explanation content (the owning domains and FLOW.DEADEND.WHY coverage); the fix suggestions shown on unresolved states (`UX.PATTERN.ERROR`).

## Requirements

- **REQ-UX.COMP.WHY-001** The spec shall define sections in this order: value (formatter string, unit, provenance badge), algorithm ID and version, inputs (each with entity link and digest), decision (rule ID, outcome glyph and text), uncertainty (UEC class 0 to 3, with UEC-0 shown as "not evaluated" and never as a number), journal entry (hash and link).
- **REQ-UX.COMP.WHY-002** Every section item that names an entity shall be a link that selects or opens the entity, reachable by keyboard.
- **REQ-UX.COMP.WHY-003** When a section's data is unavailable, the panel shall show the section with a text reason and, where the result offers one, a suggested-fix link, never omit the section silently.
- **REQ-UX.COMP.WHY-004** The spec shall pass `fid-design comp-check`.

## Acceptance criteria

- **AC-1** Given `components/why.md`, when `fid-design comp-check` runs, then it exits 0.
- **AC-2** Given the spec, then the six sections appear in the order of REQ-001 and the UEC-0 rule is stated (design-review checklist item).

## Design

- **Deliverable:** one Markdown spec in `component-spec@1`.
- **Determinism:** D0.
- **Crate:** none (package content).

## Test fixtures

The shipped spec; an example explanation with synthetic IDs and digests in the spec; CC-BY-4.0.

## Build prompt

> Write `components/why.md` in `component-spec@1`: sections in order value (formatter string, unit, provenance badge), algorithm ID and version, inputs with entity links and digests, decision (rule ID, outcome glyph and text), uncertainty (UEC-0..3; UEC-0 shown as "not evaluated", never a number), journal entry (hash, link). Every entity is a keyboard-reachable link; unavailable sections show a text reason and a suggested-fix link if offered; IDs and digests copyable. Reuse the value display from `components/value.md`. Run `fid-design comp-check` until it exits 0, then open a design-review record.

## Open questions

- None.

## Children

_Leaf: none._
