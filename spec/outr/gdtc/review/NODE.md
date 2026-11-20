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
- Public reviewer register `gdt-reviewer-register/v1`: per roster reviewer ID a pseudonymous public ID, a pseudonymous organisation ID, consent flags, the display name and organisation name only where consent is recorded, qualification category and summary, declared interests, recusals, last confirmation date
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

- A reviewer declares to the maintainers the organisation they work for or represent, which the register records publicly as a pseudonymous organisation ID (and by name only with consent), and declares publicly any commercial interest in the outcome of an interpretation (for example a product, a consulting engagement or a profile sponsorship).
- A reviewer does not sign off a rule they authored (already enforced by `GDT.RES.ROWFLOW.STATE`), a profile their organisation sponsors, or a rule they have listed as a recusal.
- The two sign-offs of one revision come from reviewers with different organisation IDs.
- No reviewer name or organisation name appears in any public file without the reviewer's recorded consent; otherwise the pseudonymous IDs stand in for them.
- Declarations are re-confirmed at least every 12 months; a reviewer whose confirmation has lapsed is not credited until they confirm.

## Requirements

- **REQ-OUTR.GDTC.REVIEW-001** The register shall hold one entry per reviewer ID in the roster, with pseudonymous public ID, pseudonymous organisation ID, consent flags `credit_name` and `credit_org`, qualification category from the four listed, qualification summary (at most 280 characters), declared interests, recusal list (rule and profile IDs) and last confirmation date; it shall hold a display name only when `credit_name` is true and an organisation name only when `credit_org` is true, and the check shall fail for a name present without its consent flag.
- **REQ-OUTR.GDTC.REVIEW-002** The check shall fail for every roster reviewer without a register entry, and for every register entry whose reviewer ID is not in the roster.
- **REQ-OUTR.GDTC.REVIEW-003** For every `signed_off` revision in the status register, the check shall fail when the two approving reviewers have the same organisation ID, when an approver's organisation ID equals the sponsoring organisation ID of the profile being signed off, or when an approver lists the rule or profile among their recusals.
- **REQ-OUTR.GDTC.REVIEW-004** The check shall report, without failing, every reviewer whose last confirmation is more than 12 months before `--as-of`, and the rule pages shall not credit that reviewer by name until they confirm.
- **REQ-OUTR.GDTC.REVIEW-005** The reviewer page shall publish the qualification criteria, the conflict-of-interest rules, the joining and leaving procedure, and for each reviewer the pseudonymous ID, dialects, qualification category and summary and declared interests, plus the display name and organisation name only where consent is recorded.
- **REQ-OUTR.GDTC.REVIEW-006** The contact-data check shall scan only the register's declared free-text fields (`summary`, `interests`, `display_name`) and shall fail with `contact_data` naming the field.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/gdtc/review/ok/` (roster of three reviewers, matching register entries, a status register with one `signed_off` revision approved by reviewers of two organisations), when `fid-outreach gdt-reviewers --roster roster.json --register reviewers.toml --status status.json --profiles profiles.toml --as-of 2027-01-15 --out out/ --json` runs, then it exits 0 and writes the reviewer page.
- **AC-2** (error path) Given `missing_entry/` and `unknown_entry/`, then exit 1 naming the reviewer ID.
- **AC-3** (error path) Given `same_org/`, `sponsor_conflict/` and `recused/`, then exit 1 naming the revision and the rule broken.
- **AC-4** Given `lapsed/` (one reviewer last confirmed 2025-11-01), then exit 0 with a lapse report naming that reviewer, and the register output marks them not creditable.
- **AC-5** (error path) Given `contact/` (an email address in a summary), then exit 1 with `contact_data` naming `summary`; given an email-like string in `id`, which is not a free-text field, then it is not scanned (the ID grammar rejects it instead).
- **AC-6** (determinism) Running AC-1 twice gives byte-identical output.
- **AC-7** (consent, error path) Given `name_no_consent/` (a display name with `credit_name = false`), then exit 1; given `ok/` with one reviewer without consent, then neither that reviewer's name nor organisation appears in `out/`, only `rv-…` and `org-…` IDs.

## Design

- **Command:** `fid-outreach gdt-reviewers --roster <file> --register <file> --status <file> --profiles <file> --as-of <date> --out <dir> [--json]`. Project tooling; nearest class W1.
- **Register (`gdt-reviewer-register/v1`, TOML):** `[[reviewer]] id` (roster reviewer ID), `public_id` (`rv-` plus 4 to 8 hex digits, assigned by maintainers, not derived from the name), `org_id` (`org-` plus digits, assigned by maintainers), `credit_name` (bool), `display_name` (only with `credit_name`), `credit_org` (bool), `organisation` (only with `credit_org`), `category` (`certification` | `practice` | `academic` | `committee`), `summary`, `interests` (list of strings, each at most 280 characters), `recusals` (rule and profile IDs), `confirmed` (date). Duplicate IDs and keys refused. Conflict rules compare `org_id` only.
- **Algorithm:** join roster, register, status register approvals and profile sponsors; apply the rules; scan the declared free-text fields with the contact-data patterns shared with `OUTR.GDTC.CASE`; render the page; run the GDTC standards-text lint on it.
- **Errors:** `register_missing{id}`, `register_unknown{id}`, `same_organisation{revision}`, `sponsor_conflict{revision}`, `recused_approval{revision}`, `contact_data`; exit 0, 1, 2.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `gdtc::reviewers`.

## Test fixtures

Synthetic rosters, registers and status registers under `fixtures/outr/gdtc/review/` (invented reviewers and organisations; CC-BY-4.0).

## Build prompt

> In crate `fid-outreach`, add module `gdtc::reviewers` and subcommand `fid-outreach gdt-reviewers --roster <file> --register <file> --status <file> --profiles <file> --as-of <date> --out <dir> [--json]`. The register `gdt-reviewer-register/v1` (TOML; duplicates refused) holds per roster reviewer ID: `public_id` (`rv-…`), `org_id` (`org-…`), consent flags `credit_name` and `credit_org`, display name (≤ 80 chars, only with `credit_name`), organisation name (only with `credit_org`), category (`certification` | `practice` | `academic` | `committee`), summary (≤ 280 chars), interests, recusals (rule and profile IDs), last confirmation date; fail on a name without its consent flag; scan only the free-text fields `summary`, `interests` and `display_name` for contact data (reuse the scan of `gdtc::cases`). Fail (exit 1) when a roster reviewer has no entry or an entry is not in the roster; and for each `signed_off` revision of the status register (`gdt-rule-status/v1`) when both approvers have the same `org_id`, an approver's `org_id` equals the profile's `sponsor_org` (`gdt-profiles/v1`), or an approver lists the rule or profile as a recusal. Report, without failing, reviewers last confirmed more than 12 months before `--as-of` and mark them not creditable. Write the reviewer page with the qualification criteria, conflict rules, joining and leaving procedure and each reviewer's public fields (names only with consent), and run the GDTC standards-text lint on it. Tests per AC-1..AC-7 with fixtures under `fixtures/outr/gdtc/review/`. Constraints: no panics, no `unsafe`; deny `unwrap`/`expect`/`panic`; no clock reads; BTreeMap only.

## Open questions

- `GDT.RES.STATUS` already refuses to count a conflicted approval, reading this register as an artifact; this check is the public, after-the-fact report of the same rules. Refusing a conflicted review at review time would need the register inside `GDT.RES.ROWFLOW.REVIEW`; left for later.
- How many years count as "several" for the practice category is left to the nominating maintainers, case by case, and recorded in the summary.

## Children

_Leaf: none._
