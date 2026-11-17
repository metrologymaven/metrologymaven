---
id: VIZ.SHELL.A11Y.SPKIME
title: Toolkit spike - CJK IME composition and font fallback
level: 4
parent: VIZ.SHELL.A11Y
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.A11Y-001]
covers: []
depends_on: []
uses_artifacts: [I18N.SCRIPT]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.A11Y.SPKIME: Toolkit spike - CJK IME composition and font fallback

## Purpose

The fourth D-005 spike, raised by I18N: before egui + winit + wgpu is confirmed, prove that text fields compose Japanese, Chinese (Pinyin) and Korean input correctly through the OS input methods, place the candidate window at the caret, expose composition state to the accessibility API, and render Han characters from the right per-language font in the fallback chain. Its output is a measured report for the maintainers; it adopts nothing itself.

## Scope

**In:** a prototype with single-line and multi-line text fields, a table cell editor and a tree rename field; scripted and manual IME sessions for Japanese (Microsoft IME, macOS Kotoeri), Chinese Pinyin and Korean on Windows, macOS and one Linux desktop with IBus or Fcitx; font fallback with per-language Han variants; screen-reader check of preedit and committed text; results report.
**Out:** the production IME and font contract (`I18N.SCRIPT`); the other spikes (`VIZ.SHELL.SPKTREE`, `VIZ.SHELL.SPKTABLE`, `VIZ.SHELL.SPKRPT`); the decision (maintainers, D-005).

## Requirements

- **REQ-VIZ.SHELL.A11Y.SPKIME-001** The spike shall show preedit text inline during composition and shall place the IME candidate window within 2 text lines of the caret in every tested field, on each tested OS.
- **REQ-VIZ.SHELL.A11Y.SPKIME-002** The spike shall verify that composition keystrokes (including Enter, Space and arrows during composition) are not treated as shortcuts, and that a committed composition leaves exactly the committed text in the field.
- **REQ-VIZ.SHELL.A11Y.SPKIME-003** The spike shall verify that, with the UI language set to Japanese, Simplified Chinese, Traditional Chinese or Korean, a test string of shared Han characters renders from that language's glyph-variant font.
- **REQ-VIZ.SHELL.A11Y.SPKIME-004** The spike shall verify that a screen reader on Windows announces committed text in a text field and its role and name.
- **REQ-VIZ.SHELL.A11Y.SPKIME-005** The spike shall publish a results report with pass or fail per criterion, OS and input method, toolkit versions, and observed limitations, and shall pass only if REQ-001 to REQ-003 pass on Windows and macOS.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/ime-cases` (20 composition scripts: 8 Japanese, 6 Pinyin, 6 Korean), when run on Windows with the OS IMEs, then each field's final text equals the expected committed string and the report records candidate-window offset from the caret.
- **AC-2** Given the same cases on macOS, then results are recorded against the same criteria.
- **AC-3** Given a field focused with a shortcut bound to Enter, when Enter commits a composition, then the shortcut does not fire.
- **AC-4** Given the Han test string under each of the four CJK UI languages, then golden screenshots per language match and differ from each other where glyph variants differ.
- **AC-5** Given the report, then it contains a pass or fail for every criterion on every OS and input method tested.

## Design

- **Registry command(s):** none; throwaway prototype plus harness.
- **Algorithm:** winit IME events (preedit, commit) wired to egui text edit; caret rectangle reported for candidate placement; font fallback chain from the I18N.SCRIPT font data where available, else open-licensed Noto CJK fonts (OFL-1.1) [unverified: final font choice]; scripted IME driving through OS automation where available, otherwise a documented manual run.
- **Errors:** n/a (report records failures).
- **Determinism:** N.
- **Performance budget:** none beyond 33 ms frame time while composing on H1 [unverified].
- **Crate:** spike repository folder `spikes/ime` (not shipped).

## Test fixtures

`fixtures/viz/ime-cases` (project-authored composition scripts and expected strings); open-licensed CJK fonts listed in ATTRIBUTION.md.

## Build prompt

> Build a throwaway spike in `spikes/ime` on egui + winit + wgpu: single-line and multi-line text fields, a table cell editor and a tree rename field. Wire winit IME preedit and commit events, report the caret rectangle for candidate placement, suppress shortcuts during composition, and set per-language font fallback chains (Japanese, Simplified Chinese, Traditional Chinese, Korean Han variants) with open-licensed fonts. Run the 20 cases of `fixtures/viz/ime-cases` on Windows and macOS (and one Linux IBus/Fcitx desktop if possible), check screen-reader announcement on Windows, capture golden screenshots of a shared Han string per language, and publish a report with pass/fail per criterion, OS and input method, toolkit versions and limitations. Pass requires preedit, candidate placement, shortcut suppression, exact commits and correct glyph variants on Windows and macOS. Rules: CONSTITUTION §8 (even in spikes, no `unsafe`); D-005 confirmation is a human decision.

## Open questions

- IME support in the provisional stack is reported as incomplete on some platforms [unverified]; a failure here reopens D-005 for text-entry screens.

## Children

_Leaf: none._
