---
id: UX.SYSTEM.PRINC
title: Design principles and UI writing guide
level: 3
parent: UX.SYSTEM
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.SYSTEM-001, REQ-UX.SYSTEM-010]
covers: []
depends_on: [UX.SYSTEM.PKG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.SYSTEM.PRINC: Design principles and UI writing guide

## Purpose

The two short documents every design review cites: the design principles (with "do" and "don't" examples) and the UI writing guide (voice, terminology, reserved words, units and numbers in UI copy). It ships a machine-readable reserved-word list and a structural check so that review records and UI copy can be checked against them.

## Scope

**In:** `design/principles.md`; `design/writing-guide.md`; `design/reserved-words.json` (`reserved-words@1`); the `fid-design princ-check` structural check and the `fid-design words-check` copy check.
**Out:** the review record that cites principles (`UX.SYSTEM.REVIEW`); package format and versioning (`UX.SYSTEM.PKG`); message IDs and translations (I18N); trade-dress rules (`OUTR.BRAND.GUIDE.DRESS`, applied in `UX.SYSTEM.REVIEW`).

## Requirements

- **REQ-UX.SYSTEM.PRINC-001** The principles document shall define exactly these principles, each under its own heading with a stable ID: `P-DENSE` (dense but calm), `P-DATA` (data first), `P-EXPLAIN` (every number explainable), `P-KEYS` (keyboard-complete), `P-FAMILIAR` (familiar workflows without incumbent trade dress), `P-NOCOLOR` (status never by colour alone).
- **REQ-UX.SYSTEM.PRINC-002** Each principle section shall contain at least one "Do" example and one "Don't" example, written in the project's own words with no incumbent screenshot or text.
- **REQ-UX.SYSTEM.PRINC-003** The writing guide shall define voice, the preferred term for each metrology concept the UI names (with the competitor vocabulary it maps to, described in our own words), number and unit presentation rules (value and unit always together, formatter output shown verbatim) and capitalisation of command titles.
- **REQ-UX.SYSTEM.PRINC-004** The reserved-word list shall give each reserved word its only permitted meaning and the message-ID prefixes where it may appear, and shall include at least "approve" (human approval of proposals, programs and runs only), "release", "sign", "conforms" and "verified".
- **REQ-UX.SYSTEM.PRINC-005** When `fid-design words-check` runs over a UI-copy export, it shall report every string that uses a reserved word under a message ID outside that word's permitted prefixes.
- **REQ-UX.SYSTEM.PRINC-006** If a principle section lacks a stable ID, a "Do" or a "Don't" example, then `fid-design princ-check` shall fail and name the section.

## Acceptance criteria

- **AC-1** Given the shipped `design/principles.md`, when `fid-design princ-check --json` runs, then it exits 0 and lists the six principle IDs of REQ-001.
- **AC-2** (error path) Given fixture `fixtures/ux/system/princ/no-dont.md` (P-DATA has no "Don't"), when the check runs, then it exits 1 with `PRINC-EXAMPLE-MISSING P-DATA`.
- **AC-3** Given fixture `fixtures/ux/system/princ/copy-ok.jsonl` and the shipped `reserved-words.json`, when `fid-design words-check --copy copy-ok.jsonl --json` runs, then it exits 0.
- **AC-4** (error path) Given `copy-bad.jsonl` where message `agent.proposal.apply_button` reads "Approve", when the check runs, then it exits 1 with `WORD-RESERVED approve agent.proposal.apply_button`.

## Design

- **Commands:** `fid-design princ-check [--file <md>] [--json]`, `fid-design words-check --copy <jsonl> [--words <json>] [--json]`. Project tooling, not registry commands; nearest class R0.
- **Formats:** principle headings `## P-<ID>: <title>` with `### Do` and `### Don't` subsections. `reserved-words@1`: `[{ word, meaning, allowed_prefixes: [..], case_insensitive: true }]`. UI-copy export: JSON Lines `{ "id": "<message id>", "text": "<English source>" }`, which I18N or VIZ produce from their catalogue (this node defines the export shape only).
- **Matching:** whole-word, case-insensitive, also matching simple inflections listed per word (e.g. approve, approved, approval).
- **Errors:** `PRINC-ID-MISSING`, `PRINC-EXAMPLE-MISSING`, `PRINC-UNKNOWN`, `WORD-RESERVED`, `WORDS-SCHEMA`.
- **Determinism:** D0 (text parsing, sorted output).
- **Crate:** `fid-design` (Tooling layer), modules `princ` and `words`.

## Test fixtures

`fixtures/ux/system/princ/`: `no-dont.md`, `copy-ok.jsonl`, `copy-bad.jsonl`; all synthetic, CC-BY-4.0.

## Build prompt

> Write `design/principles.md` in your own words with six sections `## P-DENSE`, `## P-DATA`, `## P-EXPLAIN`, `## P-KEYS`, `## P-FAMILIAR`, `## P-NOCOLOR`, each with a short rationale, a `### Do` and a `### Don't` example (no competitor screenshots or text). Write `design/writing-guide.md`: voice (plain, exact, no blame), preferred terms with a mapping to common industry vocabulary in your own words, value-and-unit rules (show the formatter output verbatim, never re-round), command-title capitalisation. Write `design/reserved-words.json` (`reserved-words@1`) with at least approve (human approval only; prefixes `review.human.`, `program.release.`, `run.approve.`), release, sign, conforms, verified.
>
> In the Rust tooling crate `fid-design`, add `fid-design princ-check [--file] [--json]` (headings and Do/Don't presence; errors `PRINC-ID-MISSING|PRINC-EXAMPLE-MISSING|PRINC-UNKNOWN`) and `fid-design words-check --copy <jsonl> [--words <json>] [--json]` (whole-word, case-insensitive, inflection list; error `WORD-RESERVED <word> <id>`). Exit 0 clean, 1 findings, 2 unreadable input. Tests AC-1..AC-4 with fixtures under `fixtures/ux/system/princ/`.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded input (copy file at most 16 MiB); no network; sorted, deterministic output.

## Open questions

- The UI-copy export shape is defined here; I18N and VIZ.SHELL.THEME should produce it from their catalogue so the words check can run in their CI.

## Children

_Leaf: none._
