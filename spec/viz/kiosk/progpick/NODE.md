---
id: VIZ.KIOSK.PROGPICK
title: Released-program chooser and serial entry
level: 3
parent: VIZ.KIOSK
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.KIOSK-001]
covers: []
depends_on: [VIZ.KIOSK.SESSION, AUTO.PROG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.KIOSK.PROGPICK: Released-program chooser and serial entry

## Purpose

The operator's first screen in a cycle: pick a program from the released programs assigned to this station, then scan or type the part serial. Draft, in-review or retired programs never appear. Barcode scanners act as keyboard input, so serial entry is bounded and validated against the program's declared serial pattern.

## Scope

**In:** the chooser screen, filtering by lifecycle state and station assignment, serial entry with bounded validation, hand-off to the program runner.
**Out:** program lifecycle and release (`AUTO.PROG`); starting the run (`AUTO.RUN`); operator steps (`VIZ.KIOSK.STEPS`).

## Requirements

- **REQ-VIZ.KIOSK.PROGPICK-001** The chooser shall list only programs whose lifecycle state is released and that are assigned to the current station.
- **REQ-VIZ.KIOSK.PROGPICK-002** If a program's state changes away from released while it is listed, then it shall disappear from the list before the next selection is accepted.
- **REQ-VIZ.KIOSK.PROGPICK-003** Serial entry shall accept at most 64 characters, shall validate against the program's declared pattern with a bounded matcher, and shall treat the serial as untrusted text.
- **REQ-VIZ.KIOSK.PROGPICK-004** Every chooser target shall be at least 15 mm on the reference kiosk display [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/kiosk-programs` (one released, one draft, one retired, one released but assigned to another station), when the chooser loads on station S1, then it lists exactly the released program assigned to S1.
- **AC-2** Given the listed program retired during display, when the operator selects it, then selection is refused with `ProgramNotReleased` and the list refreshes.
- **AC-3** Given serial pattern `^[A-Z]{2}[0-9]{6}$`, when "AB123456" is entered, then it is accepted; "AB12" is rejected with `SerialInvalid`; a 65-character input is rejected with `SerialTooLong`.
- **AC-4** Given the shop-floor theme on the reference kiosk display profile, then every chooser button measures at least 15 mm (layout test using the profile's DPI).

## Design

- **Registry command(s):** none new; reads programs through AUTO.PROG R0 queries and hands off to AUTO.RUN's start command.
- **Data structures:** `ProgramChoice { program_id, revision, title: UntrustedText }`; `SerialPattern` (bounded regex subset without backreferences).
- **Algorithm:** query released programs for the station; subscribe to lifecycle changes; re-check state at selection time; validate serial with a linear-time matcher.
- **Errors:** `ProgramNotReleased`, `SerialInvalid`, `SerialTooLong`.
- **Determinism:** D0.
- **Performance budget:** chooser shows within 500 ms for 200 programs [unverified].
- **Crate:** `fid-kiosk` (module `progpick`).

## Test fixtures

`fixtures/viz/kiosk-programs`: generated project with four programs in different states; in-repo.

## Build prompt

> In crate `fid-kiosk`, add module `progpick`: a chooser listing only released programs assigned to the current station (query AUTO.PROG; re-check the state at selection and refuse with `ProgramNotReleased` if it changed), then serial entry of at most 64 characters validated by a linear-time matcher against the program's declared pattern (`SerialInvalid`, `SerialTooLong`), treating the serial as untrusted text, then hand-off to the program runner's start command. Targets at least 15 mm on the reference kiosk display. Tests: state and station filtering; retired-during-display refusal; serial accept and reject cases; target-size layout test. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; bounded, fail-closed input handling.

## Open questions

- Station assignment of programs may belong to AUTO.PROG metadata; confirm when AUTO.PROG is split.

## Children

_Leaf: none._
