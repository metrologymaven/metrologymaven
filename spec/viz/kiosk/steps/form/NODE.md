---
id: VIZ.KIOSK.STEPS.FORM
title: Operator prompt and field renderers
level: 4
parent: VIZ.KIOSK.STEPS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.KIOSK.STEPS-002, REQ-VIZ.KIOSK.STEPS-003, REQ-VIZ.KIOSK.STEPS-004]
covers: []
depends_on: [AUTO.PROG.OPER, AUTO.RUN.PLAY.ANSWER, VIZ.SHELL.THEME]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.KIOSK.STEPS.FORM: Operator prompt and field renderers

## Purpose

Renders an `operator-v1` descriptor's prompt, optional image and fields of type `ack`, `bool`, `integer`, `decimal`, `quantity`, `text` and `choice` as large, glove-friendly controls in the shop-floor theme, and submits the answer with `program.answer_step@1`. Numbers are typed on an on-screen keypad and passed as the exact decimal strings the operator entered, with the declared unit; field errors returned by the command are shown next to their fields. Comment steps (no fields) show an acknowledge button only.

## Scope

**In:** prompt rendering as plain text; image rendering from the descriptor's root handle (decoded with a bounded decoder, at most 4096x4096 px); one renderer per field type; on-screen numeric keypad; display of declared unit, min and max; local pre-checks that mirror the descriptor rules; submission and field-error display.
**Out:** the descriptor schema and validation (`AUTO.PROG.OPER`); the answer command (`AUTO.RUN.PLAY.ANSWER`); `element` fields (`VIZ.KIOSK.STEPS.PICK`); hosting and sequencing (`VIZ.KIOSK.STEPS.LOCK`).

## Requirements

- **REQ-VIZ.KIOSK.STEPS.FORM-001** Numeric and quantity fields shall pass the operator's entered characters unchanged as a decimal string with the declared unit symbol, and shall never convert them to binary floating point.
- **REQ-VIZ.KIOSK.STEPS.FORM-002** Quantity fields shall show the declared unit and offer no unit choice, and bounds shall be shown as the descriptor's authored strings.
- **REQ-VIZ.KIOSK.STEPS.FORM-003** Prompt text, labels, choices and images from the program shall be rendered as untrusted content: text as plain text with no markup or links, images only through the bounded decoder.
- **REQ-VIZ.KIOSK.STEPS.FORM-004** When `program.answer_step@1` returns field errors, every error shall be shown at its field and the entered values kept.
- **REQ-VIZ.KIOSK.STEPS.FORM-005** Every control shall be at least 15 mm in both dimensions on the reference kiosk display [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/kiosk-forms.json` (one descriptor per field type, including a quantity in mm with min `10.0` and max `10.5`), when the harness types `10.10` and submits, then the command receives the string `10.10` with unit `mm`.
- **AC-2** Given a prompt containing `<b>` and a Markdown link, then the UI snapshot shows the literal characters and the accessibility tree has no link.
- **AC-3** (error path) Given an answer above max, then the command's `above_max` error is shown at the field and the typed value remains.
- **AC-4** Given a 10,000 x 10,000 px image handle, then the decoder refuses it and the prompt shows "image too large" without crashing.
- **AC-5** Given the target-size audit, then every control is ≥ 15 mm at the reference DPI.

## Design

- **Registry command(s):** none owned; submits `program.answer_step@1`.
- **Data structures:** `FieldWidget` per `ResponseType`; `EnteredValue { text, unit }`.
- **Algorithm:** build widgets from descriptor fields in order; keypad emits characters only from `0-9 . -`; pre-checks reuse the descriptor's exact-decimal comparator through `AUTO.PROG.OPER::validate` before submission (advisory; the command is authoritative).
- **Errors:** `image_too_large`, `image_unreadable` (shown in place of the image).
- **Determinism:** D0.
- **Performance budget:** screen ready < 100 ms for 10 fields [unverified].
- **Crate:** `fid-kiosk` (module `steps::form`).

## Test fixtures

`fixtures/viz/kiosk-forms.json`, hand-written descriptors; CC-BY-4.0.

## Build prompt

> In crate `fid-kiosk`, add module `steps::form`: renderers for `operator-v1` prompts (plain text, optional image through a bounded decoder ≤ 4096×4096) and fields `ack`, `bool`, `integer`, `decimal`, `quantity` (fixed declared unit), `text`, `choice`, with an on-screen keypad, controls ≥ 15 mm from physical DPI in the shop-floor theme. Submit with `program.answer_step@1` passing entered decimal strings and units unchanged (no f64), show every returned field error at its field and keep the input. All program text is untrusted (C6). Tests AC-1..AC-5. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- None blocking.

## Children

_Leaf: none._
