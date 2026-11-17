---
id: D-026
title: Python authoring view for Command Programs
status: open
raised_by: AUTO.PROG level-3 expansion
blocks: []
decide_by: before AUTO.PROG.EDIT is split
updated: 2026-11-16
---

# D-026: Python authoring view for Command Programs

## Question

Should users be able to author and edit Command Programs as Python source (an "authoring view"), with the canonical program format underneath as the authoritative form?

## Context

The AUTO domain node answered "yes, unless decided otherwise; confirm when PROG is expanded", following the Design Baseline. AUTO.PROG keeps the canonical format authoritative (REQ-AUTO.PROG-001, -014) and offers two views (command and summary). A Python view would be a third projection. Agents may not adopt their own proposals, so the confirmation needs a person.

A Python view is only safe if it is **lossless in both directions** for the subset it accepts: every canonical program prints to Python, and every Python file the view accepts parses back to exactly that canonical program. Arbitrary Python (imports, classes, comprehensions, I/O) cannot map to the format, so the view would be a restricted dialect, and users may expect full Python. Full Python already exists as scripts (`AUTO.PY`), which are not replayable programs.

Affects: `AUTO.PROG.EDIT` (third view), `AUTO.PROG.FMT` (round-trip tests), `AUTO.PY` (users may confuse the two).

## Options

| Option | For | Against |
|---|---|---|
| A. Yes in P1b: a restricted Python dialect, round-trip tested, as a third editor view | Familiar syntax; matches the baseline | Users will hit the restrictions; a second grammar to maintain |
| B. Not in P1b: command and summary views only; Python stays the scripting surface (`AUTO.PY`) with a "convert recorded program to script" export | Smaller, clearer boundary between replayable programs and scripts | Departs from the baseline; text-first users lose a text view |
| C. A non-Python text view (a small, purpose-built syntax) printed from and parsed to the canonical form | Lossless by design; no false promise of Python | Yet another language to learn |

## Recommendation

B for P1b, revisit A after field use of the editor views. The canonical JSON is already diffable; a script export covers users who want Python.

## Decision

_Open._ When decided: update `AUTO.PROG.EDIT` scope and the open question in `AUTO`.
