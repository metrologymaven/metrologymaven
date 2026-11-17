---
id: UX.COMP.CONT
title: Container component specs (panels, tabs, wizard and rail, dialog, toast, status bar)
level: 3
parent: UX.COMP
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UX.COMP-001, REQ-UX.COMP-002, REQ-UX.COMP-003, REQ-UX.COMP-004, REQ-UX.COMP-006, REQ-UX.COMP-008]
covers: []
depends_on: [UX.COMP.SCHEMA]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.COMP.CONT: Container component specs

## Purpose

The specs of the components that hold and sequence others: dockable panels, tabs, the wizard and stepper (including the workflow rail that guides a job), modal and non-modal dialogs, toasts and notifications, and the status bar.

## Scope

**In:** `components/panel.md`, `components/tabs.md`, `components/stepper.md` (wizard and workflow rail), `components/dialog.md`, `components/toast.md`, `components/status-bar.md`.
**Out:** docking implementation (VIZ.SHELL); onboarding sequences and confirmation wording (`UX.PATTERN.GUIDE`, `UX.PATTERN.OPERATE`); progress and cancel behaviour (`UX.PATTERN.ERROR`).

## Requirements

- **REQ-UX.COMP.CONT-001** The dialog spec shall trap focus while modal, return focus to the invoking control on close, and close on Escape only when closing loses no entered data or asks first.
- **REQ-UX.COMP.CONT-002** The toast spec shall keep any toast that reports an error, a nonconformance or a required action until dismissed, and shall make every toast reachable later from a notification list.
- **REQ-UX.COMP.CONT-003** The stepper spec shall show for each step its number, title, state (pending, current, done, blocked) as glyph and text, and the reason for a blocked step as text.
- **REQ-UX.COMP.CONT-004** The panel and tabs specs shall define keyboard commands to move focus between panels (F6 and Shift+F6 or equivalents) and between tabs, and the status-bar spec shall expose its live items to assistive technology as polite announcements.
- **REQ-UX.COMP.CONT-005** Each spec shall pass `fid-design comp-check`.

## Acceptance criteria

- **AC-1** Given the six shipped specs, when `fid-design comp-check` runs, then it exits 0.
- **AC-2** Given `components/dialog.md` and `components/toast.md`, then the focus-return, Escape and persistent-toast rules are present (design-review checklist items).
- **AC-3** Given `components/stepper.md`, then the blocked-step reason is defined as visible text (verified by `comp-check` `disabled_reason`).

## Design

- **Deliverable:** six Markdown specs in `component-spec@1`.
- **Determinism:** D0.
- **Crate:** none (package content); checked by `fid-design comp-check`.

## Test fixtures

The shipped specs; CC-BY-4.0.

## Build prompt

> Write six component specs in `component-spec@1`: `panel.md`, `tabs.md`, `stepper.md` (wizard and workflow rail: step number, title, state glyph and text, blocked reason), `dialog.md` (modal focus trap, focus return, Escape rules), `toast.md` (errors, nonconformances and required actions persist until dismissed; notification list), `status-bar.md` (live items announced politely). Define F6/Shift+F6 panel cycling and tab navigation. Original wording and diagrams. Run `fid-design comp-check` until it exits 0, then open a design-review record.

## Open questions

- None.

## Children

_Leaf: none._
