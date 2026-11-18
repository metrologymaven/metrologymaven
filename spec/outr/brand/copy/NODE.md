---
id: OUTR.BRAND.COPY
title: Copied-text policy guard (D-013 option C)
level: 3
parent: OUTR.BRAND
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-outreach]
built_pr: 102
satisfies: [REQ-OUTR.BRAND-005]
covers: []
depends_on: []
uses_artifacts: [OUTR.BRAND.REVIEW]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BRAND.COPY: Copied-text policy guard (D-013 option C)

## Purpose

Protection against copying incumbents' public text into outreach material. D-013 (accepted) chose option C: the project keeps no reference set of incumbent text and does no automatic copy matching; it relies on the clean-room rule (CONSTITUTION §8) and the human "copied text" item of the trade-dress review (OUTR.BRAND.REVIEW), and revisits hashed-shingle matching (option B) after a counsel review if copied text is ever found. This node keeps that arrangement from eroding: a policy file, and a small guard that fails when the human control is removed, when a found copy has not triggered the revisit, or when the policy is changed without the counsel review D-013 requires.

## Scope

**In:** the policy file `brand/copy-policy.toml`; the `copy-guard` check over the policy file, the trade-dress checklist and the review records; fixtures written by the project.
**Out:** the review checklist, records and gate (`OUTR.BRAND.REVIEW`); the claim lint engine and rules (`OUTR.BRAND.LINT`, with ECO.RULES.VOCAB as the shared engine); any reference set of incumbent text (not kept, per D-013); a future matcher under option B (needs a new decision after counsel review).

## Requirements

- **REQ-OUTR.BRAND.COPY-001** The policy file shall state `mode = "review_only"` and `decision = "D-013"`, and if `mode` has any other value without a `counsel_review` reference (date and reviewer reference), then the guard shall fail with `counsel_review_missing`.
- **REQ-OUTR.BRAND.COPY-002** If the trade-dress checklist has no item with ID `copied_text`, then the guard shall fail with `copied_text_item_missing`.
- **REQ-OUTR.BRAND.COPY-003** If any review record answers `copied_text` with `fail`, and the policy file has no `[[revisit]]` entry naming that record's release, then the guard shall fail with `d013_revisit_due` and name the release.
- **REQ-OUTR.BRAND.COPY-004** While `mode` is `review_only`, the guard shall fail with `reference_set_present` if any file in the scanned repository declares itself a reference set of third-party text (a `refset` file extension or a `kind = "incumbent_text"` key in TOML or JSON).
- **REQ-OUTR.BRAND.COPY-005** The guard's fixtures shall consist of project-authored text only, and none shall contain text from an incumbent's material.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/brand/copy/ok/` (policy `review_only`, a checklist with `copied_text`, and two review records answering pass), when `fid-outreach copy-guard --policy copy-policy.toml --checklist checklist.toml --records records/ --root . --json` runs, then it exits 0.
- **AC-2** (error path) Given `no-item/` (a checklist without `copied_text`), when the guard runs, then it exits 1 with `copied_text_item_missing`.
- **AC-3** (error path) Given `found-copy/` (one record for release `site-2027.03` answering `copied_text = fail`, no revisit entry), when the guard runs, then it exits 1 with `d013_revisit_due site-2027.03`; after adding `[[revisit]] release = "site-2027.03"` with a note, it exits 0.
- **AC-4** (error path) Given `mode-changed/` (`mode = "hashed_shingles"` with no `counsel_review`), when the guard runs, then it exits 1 with `counsel_review_missing`.
- **AC-5** (error path) Given `refset/` (a file `brand/incumbents.refset.json`), when the guard runs, then it exits 1 with `reference_set_present`.
- **AC-6** (error path) Given a missing policy file, when the guard runs, then it exits 2 and names the path.

## Design

- **Command:** `fid-outreach copy-guard --policy <file> --checklist <file> --records <dir> --root <dir> [--json]`. Project tooling; nearest class R0. Adding a `[[revisit]]` or `counsel_review` entry is a human act through a reviewed pull request.
- **Policy file:** `mode = "review_only"`, `decision = "D-013"`, optional `counsel_review = { date, reviewer_ref }`, `[[revisit]] release, date, note`.
- **Inputs:** the checklist and record TOML formats of OUTR.BRAND.REVIEW (`items = [{ id, question }]`; `answers.<item> = { result, note }`).
- **Errors:** `counsel_review_missing`, `copied_text_item_missing`, `d013_revisit_due`, `reference_set_present`, `policy_parse`; exit 0, 1 findings, 2 unreadable input.
- **Determinism:** D0; sorted walk.
- **Crate:** `fid-outreach`, module `copy_guard`.

## Test fixtures

Hand-written policy, checklist and record files under `fixtures/outr/brand/copy/` (`ok`, `no-item`, `found-copy`, `mode-changed`, `refset`), CC-BY-4.0, with project-authored text only.

## Build prompt

> In the Rust crate `fid-outreach`, add module `copy_guard` and subcommand `fid-outreach copy-guard --policy <file> --checklist <file> --records <dir> --root <dir> [--json]`, and write `brand/copy-policy.toml` with `mode = "review_only"` and `decision = "D-013"`.
>
> Background (project decision D-013): the project keeps no copy of competitors' text and runs no automatic copy matcher; a human reviewer answers a "copied text" checklist item for every outreach release, and the project revisits automatic matching (after a counsel review) if copied text is ever found.
>
> Checks: `counsel_review_missing` when `mode` is not `review_only` and there is no `counsel_review = { date, reviewer_ref }`; `copied_text_item_missing` when the trade-dress checklist TOML (`items = [{ id, question }]`) has no `copied_text` item; `d013_revisit_due <release>` when a review record (`release`, `answers.copied_text.result = "fail"`) has no matching `[[revisit]] release = ...` entry in the policy; `reference_set_present` when, in `review_only` mode, any file under `--root` has a `.refset.` name part or is TOML or JSON with top-level `kind = "incumbent_text"`. Exit 0 clean, 1 findings, 2 unreadable input; sorted output; RFC 8785 JSON with `--json`.
>
> Tests with fixtures under `fixtures/outr/brand/copy/`: ok, missing checklist item, found copy without and with revisit entry, mode changed without counsel review, reference-set file present, missing policy (exit 2).
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads; no network; never add competitor text to fixtures or code.

## Open questions

- **D-013 condition:** the counsel review must happen before this node's output is published or released; under option C no third-party text is handled, and REQ-001 makes any move to option B wait for that review. A move to option B also needs a new decision.
- REQ-OUTR.BRAND-005 (in the parent) still asks for automatic detection of 15 or more copied words. D-013 option C replaces it with the review-based control specified here; the parent node said the steering group would change REQ-OUTR.BRAND-005 in that case. Until it does, this leaf satisfies it only in substitute form. Cross-node issue for OUTR.BRAND.
- The rule ID CL003 proposed earlier is not used; whether OUTR.BRAND.LINT's rule file should reserve it is for that node.

## Children

_Leaf: none._
