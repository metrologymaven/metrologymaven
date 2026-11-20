---
id: OUTR.GDTC.REVIEW
title: Public reviewer role, qualification and conflicts of interest
level: 3
parent: OUTR.GDTC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.GDTC-006]
covers: []
depends_on: [OUTR.GDTC.GUARD]
uses_artifacts: [GDT.RES.ROWFLOW.ROSTER, GDT.RES.STATUS, OUTR.GDTC.PROFILE]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# OUTR.GDTC.REVIEW: Public reviewer role, qualification and conflicts of interest

## Purpose

Signing off an interpretation rule is gate H4 and needs two credentialed reviewers (D-009). This leaf defines the **public reviewer role** that community experts can hold: who qualifies, what they must declare, when they must step aside, and how they are credited. It owns the public reviewer register (the public counterpart of the DMT reviewer roster, `GDT.RES.ROWFLOW.ROSTER`, which stays the source of the role for the workflow), a CI check of conflicts against actual sign-offs, and the reviewer page. It answers the open question on `GDT.RES.ROWFLOW` that no node owned the community reviewer process.

## Scope

**In:**
- Qualification criteria, in general terms (below)
- Public reviewer register `gdt-reviewer-register/v1`: per roster reviewer ID the display name, credit consent, qualification category and summary, organisation, declared interests, recusals, last confirmation date
- Conflict-of-interest rules and a check against the sign-offs in the status register
- The public reviewer page and the joining and leaving procedure

**Out:**
- The roster file and role lookup used by the workflow (owned by `GDT.RES.ROWFLOW.ROSTER`)
- The review command (owned by `GDT.RES.ROWFLOW.REVIEW`)
- Paid author contracts and partner agreements (outside this tree)
- Rendering credit on rule pages (owned by `OUTR.GDTC.RULES`, from this register)

## Qualification criteria (general terms)

A public reviewer holds a lawful copy of each standard edition they review against (D-016) and meets at least one of:
- recognised professional certification in GD&T or GPS at an advanced level;
- several years of documented practice interpreting GD&T or GPS specifications in design, inspection or metrology;
- teaching or research in GD&T or GPS at a university or training institution;
- membership of a standards committee or working group for the dialect concerned.

Maintainers add a reviewer to the roster for the dialects they qualify for, after a public nomination under the D-003 comment period. The register records the category and a short public summary, not documents or contact data.

## Conflict-of-interest rules (general terms)

- A reviewer declares, publicly, the organisation they work for or represent and any commercial interest in the outcome of an interpretation (for example a product, a consulting engagement or a profile sponsorship).
- A reviewer does not sign off a rule they authored (already enforced by `GDT.RES.ROWFLOW.STATE`), a profile their organisation sponsors, or a rule they have listed as a recusal.
- The two sign-offs of one revision come from reviewers of different organisations.
- Declarations are re-confirmed at least every 12 months; a reviewer whose confirmation has lapsed is not credited until they confirm.

## Requirements

- **REQ-OUTR.GDTC.REVIEW-001** The register shall hold one entry per reviewer ID in the roster, with display name (at most 80 characters), credit consent, qualification category from the four listed, qualification summary (at most 280 characters), organisation, declared interests, recusal list (rule and profile IDs) and last confirmation date, and no contact data.
- **REQ-OUTR.GDTC.REVIEW-002** The check shall fail for every roster reviewer without a register entry, and for every register entry whose reviewer ID is not in the roster.
- **REQ-OUTR.GDTC.REVIEW-003** For every `signed_off` revision in the status register, the check shall fail when the two approving reviewers declare the same organisation, when an approver's organisation sponsors the profile being signed off, or when an approver lists the rule or profile among their recusals.
- **REQ-OUTR.GDTC.REVIEW-004** The check shall report, without failing, every reviewer whose last confirmation is more than 12 months before `--as-of`, and the rule pages shall not credit that reviewer by name until they confirm.
- **REQ-OUTR.GDTC.REVIEW-005** The reviewer page shall publish the qualification criteria, the conflict-of-interest rules, the joining and leaving procedure, and for each consenting reviewer the display name, dialects, qualification category and summary, organisation and declared interests.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/gdtc/review/ok/` (roster of three reviewers, matching register entries, a status register with one `signed_off` revision approved by reviewers of two organisations), when `fid-outreach gdt-reviewers --roster roster.json --register reviewers.toml --status status.json --profiles profiles.toml --as-of 2027-01-15 --out out/ --json` runs, then it exits 0 and writes the reviewer page.
- **AC-2** (error path) Given `missing_entry/` and `unknown_entry/`, then exit 1 naming the reviewer ID.
- **AC-3** (error path) Given `same_org/`, `sponsor_conflict/` and `recused/`, then exit 1 naming the revision and the rule broken.
- **AC-4** Given `lapsed/` (one reviewer last confirmed 2025-11-01), then exit 0 with a lapse report naming that reviewer, and the register output marks them not creditable.
- **AC-5** (error path) Given `contact/` (an email address in a summary), then exit 1 with `contact_data`.
- **AC-6** (determinism) Running AC-1 twice gives byte-identical output.

## Design

- **Command:** `fid-outreach gdt-reviewers --roster <file> --register <file> --status <file> --profiles <file> --as-of <date> --out <dir> [--json]`. Project tooling; nearest class W1.
- **Register (`gdt-reviewer-register/v1`, TOML):** `[[reviewer]] id`, `display_name`, `credit` (bool), `category` (`certification` | `practice` | `academic` | `committee`), `summary`, `organisation`, `interests` (list of strings, each at most 280 characters), `recusals` (rule and profile IDs), `confirmed` (date). Duplicate IDs and keys refused. Organisation names are compared after Unicode NFC, case folding and whitespace collapsing.
- **Algorithm:** join roster, register, status register approvals and profile sponsors; apply the rules; scan text fields with the contact-data patterns shared with `OUTR.GDTC.CASE`; render the page; run the GDTC standards-text lint on it.
- **Errors:** `register_missing{id}`, `register_unknown{id}`, `same_organisation{revision}`, `sponsor_conflict{revision}`, `recused_approval{revision}`, `contact_data`; exit 0, 1, 2.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `gdtc::reviewers`.

## Test fixtures

Synthetic rosters, registers and status registers under `fixtures/outr/gdtc/review/` (invented reviewers and organisations; CC-BY-4.0).

## Build prompt

> In crate `fid-outreach`, add module `gdtc::reviewers` and subcommand `fid-outreach gdt-reviewers --roster <file> --register <file> --status <file> --profiles <file> --as-of <date> --out <dir> [--json]`. The register `gdt-reviewer-register/v1` (TOML; duplicates refused) holds per roster reviewer ID: display name (≤ 80 chars), credit consent, category (`certification` | `practice` | `academic` | `committee`), summary (≤ 280 chars), organisation, interests, recusals (rule and profile IDs), last confirmation date; no contact data (reuse the contact-data scan of `gdtc::cases`). Fail (exit 1) when a roster reviewer has no entry or an entry is not in the roster; and for each `signed_off` revision of the status register (`gdt-rule-status/v1`) when both approvers declare the same organisation (compare after NFC, case folding and whitespace collapsing), an approver's organisation sponsors the profile being signed off (`gdt-profiles/v1`), or an approver lists the rule or profile as a recusal. Report, without failing, reviewers last confirmed more than 12 months before `--as-of` and mark them not creditable. Write the reviewer page with the qualification criteria, conflict rules, joining and leaving procedure and each consenting reviewer's public fields, and run the GDTC standards-text lint on it. Tests per AC-1..AC-6 with fixtures under `fixtures/outr/gdtc/review/`. Constraints: no panics, no `unsafe`; deny `unwrap`/`expect`/`panic`; no clock reads; BTreeMap only.

## Open questions

- The check runs after the fact, in CI over the exported status register. Refusing a conflicted approval at review time would need the register inside `GDT.RES.ROWFLOW.REVIEW`; left for when the roster gains an organisation field.
- How many years count as "several" for the practice category is left to the nominating maintainers, case by case, and recorded in the summary.

## Children

_Leaf: none._
