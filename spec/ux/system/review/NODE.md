---
id: UX.SYSTEM.REVIEW
title: Design review checklist, originality declaration and PR gate
level: 3
parent: UX.SYSTEM
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-design]
satisfies: [REQ-UX.SYSTEM-001, REQ-UX.SYSTEM-007, REQ-UX.SYSTEM-008, REQ-UX.SYSTEM-009, REQ-UX.SYSTEM-011]
covers: []
depends_on: [UX.SYSTEM.PKG, UX.SYSTEM.PRINC]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.SYSTEM.REVIEW: Design review checklist, originality declaration and PR gate

## Purpose

The design review every UI-changing PR passes. It defines the checklist, the originality declaration every design artefact carries, the review-record format (principles checked, trade-dress rules with results, approver), and a check that a PR touching design or UI paths has a valid, human-approved record before merge. It also guards the brand-neutral placeholder until D-002 names the product.

## Scope

**In:** `design/review/checklist.md`; `originality@1` declaration format; `design-review@1` record format; the trade-dress rule slot `design/review/dress-rules.json`; the path map of which files need review; `fid-design review-check`; the placeholder-name check.
**Out:** the content of the trade-dress rules (`OUTR.BRAND.GUIDE.DRESS`; copied into the slot by a reviewed PR); principles text (`UX.SYSTEM.PRINC`); the release brand review (`OUTR.BRAND.REVIEW`); CI wiring in consumer repositories (consuming domains).

## Requirements

- **REQ-UX.SYSTEM.REVIEW-001** Every design artefact in the package shall have an `originality@1` declaration holding author, date, references consulted (described in words, no incumbent images) and the statement that no incumbent code, text, icon, screenshot or layout was copied.
- **REQ-UX.SYSTEM.REVIEW-002** A `design-review@1` record shall hold the PR reference, the artefacts reviewed, the principle IDs checked, one result (`pass`, `fail` or `n/a` with reason) per rule in `dress-rules.json`, and the approver, who shall be a person and not an agent.
- **REQ-UX.SYSTEM.REVIEW-003** When `fid-design review-check` runs over a PR's changed-path list, it shall fail if any changed path matches the review path map and no valid record lists that path with an approval.
- **REQ-UX.SYSTEM.REVIEW-004** If a record approves an artefact that lacks an originality declaration, or omits a result for any dress rule, or cites an unknown principle ID, then `review-check` shall fail and name the defect.
- **REQ-UX.SYSTEM.REVIEW-005** If the product-name deny list (D-002 candidate names, held privately by maintainers and supplied at run time) is given, then `review-check` shall report every package text file that contains a listed name, and the shipped package shall name the product only as `{{FAMILY_NAME}}`.
- **REQ-UX.SYSTEM.REVIEW-006** The checklist shall include, as separate items, the principles, the originality declaration, each dress rule, colour-alone status cues, keyboard path and contrast evidence from the token check.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/ux/system/review/ok/` (changed paths, two artefacts with declarations, one human-approved record covering both), when `fid-design review-check --changed ok/changed.txt --root ok/design --json` runs, then it exits 0.
- **AC-2** (error path) Given `missing-record/` (a changed `tokens/` file with no record), when the check runs, then it exits 1 with `REVIEW-RECORD-MISSING tokens/color.tokens.json`.
- **AC-3** (error path) Given `agent-approver/` (approver principal `agent`), when the check runs, then it exits 1 with `REVIEW-APPROVER-NOT-HUMAN`.
- **AC-4** (error path) Given `dress-gap/` (dress-rules.json has 5 rules, the record has 4 results), when the check runs, then it exits 1 with `REVIEW-DRESS-RESULT-MISSING <rule id>`.
- **AC-5** (error path) Given `no-decl/` (approved icon without originality declaration), when the check runs, then it exits 1 with `REVIEW-ORIGINALITY-MISSING`.
- **AC-6** (error path) Given `name-leak/` and a deny-list fixture containing `Examplar`, when the check runs with `--deny-names`, then it exits 1 with `REVIEW-NAME-LEAK` naming the file.

## Design

- **Command:** `fid-design review-check --changed <file> --root <design dir> [--deny-names <file>] [--json]`. Tooling, nearest class R0. The deny list is passed at run time and never committed, since D-002 says publish nothing before the knockout search.
- **Path map `review/paths.toml`:** globs that need review (`design/**`, consumer globs such as UI source folders, declared by consumers in their own copy).
- **Formats:** `originality@1` `{ artefact, author, date, references: [text], statement_version }`, stored beside the artefact as `<file>.origin.json` or in `review/origin/`. `design-review@1` `{ pr, artefacts: [path], principles: [P-*], dress: [{rule, result, reason?}], checklist: [{item, result}], approver: {name, principal: human}, date }`.
- **Dress-rule slot:** `dress-rules.json` holds `[{ id, text_ref }]` copied by a human PR from the brand guidelines; this node defines the slot and the per-rule result, not the rules.
- **Errors:** `REVIEW-RECORD-MISSING`, `REVIEW-APPROVER-NOT-HUMAN`, `REVIEW-DRESS-RESULT-MISSING`, `REVIEW-PRINCIPLE-UNKNOWN`, `REVIEW-ORIGINALITY-MISSING`, `REVIEW-NAME-LEAK`, `REVIEW-SCHEMA`; added when built: `REVIEW-CHECKLIST-MISSING <item>` (a checklist item without a result) and `REVIEW-RESULT-FAIL <rule or item>` (a record with a `fail` result does not approve).
- **As built:** all paths (changed list, path-map globs, record artefacts) are relative to `--root`; a record counts only when its own path is in the changed list; `review/paths.toml` and `review/dress-rules.json` always need a review; untrusted text that is obfuscated, carries bidi controls or is not NFC is refused; an empty changed list, map, rule slot or deny list is a finding, never a pass; findings never quote a deny-list entry.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `review`.

## Test fixtures

`fixtures/ux/system/review/`: `ok/`, `missing-record/`, `agent-approver/`, `dress-gap/`, `no-decl/`, `name-leak/` plus `deny-names.txt` with an invented name; synthetic, CC-BY-4.0.

## Build prompt

> In the Rust tooling crate `fid-design`, add `fid-design review-check --changed <file> --root <design dir> [--deny-names <file>] [--json]`. Read `review/paths.toml` (globs needing review), `review/dress-rules.json` (`[{id, text_ref}]`), `principles.md` IDs (via the princ module), records `review/records/*.json` (`design-review@1`: pr, artefacts, principles, dress results pass|fail|n/a with reason, checklist, approver {name, principal}, date) and originality declarations (`originality@1`, `<file>.origin.json` or `review/origin/`). Fail with `REVIEW-RECORD-MISSING`, `REVIEW-APPROVER-NOT-HUMAN` (principal not `human`), `REVIEW-DRESS-RESULT-MISSING`, `REVIEW-PRINCIPLE-UNKNOWN`, `REVIEW-ORIGINALITY-MISSING`, `REVIEW-NAME-LEAK` (deny-listed names found in package text), `REVIEW-SCHEMA`. Write `design/review/checklist.md` (principles, originality, each dress rule, no colour-only status, keyboard path, contrast evidence) and a PR template section for design review. Exit 0, 1 findings, 2 unreadable. Tests AC-1..AC-6 of spec node UX.SYSTEM.REVIEW.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing; the deny list is read only from the given path and never written anywhere; no network.

## Open questions

- The dress rules come from `OUTR.BRAND.GUIDE.DRESS`; UX may not depend on OUTR, so a human PR copies the rule IDs into the slot. OUTR could instead publish the rule list as an artifact UX reads; that would need maintainers to accept a UX `uses_artifacts` edge to OUTR.
- Who may approve (any maintainer, or a design-review group) is a governance matter for GOVERNANCE.md, not set here.

## Children

_Leaf: none._
