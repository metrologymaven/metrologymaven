---
id: OUTR.GDTC.CASE
title: Case pipeline and permanent regression fixtures
level: 3
parent: OUTR.GDTC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.GDTC-004, REQ-OUTR.GDTC-005]
covers: []
depends_on: [OUTR.GDTC.GUARD]
uses_artifacts: [UNC.ORS.TRACE, UNC.RDL.GEN, GDT.RES.STATUS]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# OUTR.GDTC.CASE: Case pipeline and permanent regression fixtures

## Purpose

The "case of the week": anyone can submit a hard GD&T case (a part, a callout and data whose true answer is known because the part and data come from the project's generators and the expected value from its independent oracle). The project publishes one featured case a week, discusses it in public, resolves it against the interpretation rules, and then keeps it for ever as a regression fixture credited to its submitter. Cases turn interpretation arguments into executable checks, and each resolved case enlarges the open reference suites (`UNC.ORS`). Because the data is generated, no customer data and no personal contact data ever enter the repository.

## Scope

**In:**
- Case file format `gdt-case/v1` and its validator
- States `submitted`, `published`, `in_discussion`, `resolved`, `fixture`, and `rejected` with a reason
- Weekly featuring: one case per ISO week from the published queue
- Resolution record: rules affected and outcome (rule confirmed, rule revised, new rule needed, out of scope)
- Conversion of a resolved case into an ORS case under `fixtures/gdt/cases/<case-id>/`, permanent and credited
- Privacy check: no contact data in any case or fixture file

**Out:**
- Generators and the oracle cross-check (owned by `UNC.RDL.GEN`)
- The ORS case format, citation checks and suite versioning (owned by `UNC.ORS.TRACE`)
- Changing a rule after a resolution (owned by `GDT.RES.ROWFLOW`, through its normal review)
- Discussion channels and moderation (owned by `OUTR.COMM`)
- Routing customer-part questions (owned by `OUTR.GDTC.GUARD`)

## Requirements

- **REQ-OUTR.GDTC.CASE-001** A case file shall hold only these fields: case ID, title, credit account, optional credit display name, dialect and edition, callout (as a characteristic document), generator ID with parameters and seed, oracle cross-check reference, expected values with tolerances, question (at most 280 characters), state, discussion link, resolution and history; any other field shall be refused.
- **REQ-OUTR.GDTC.CASE-002** If a case's data does not come from a registered generator with a recorded oracle cross-check that agrees within its stated bound, then the validator shall reject it with `not_generated`.
- **REQ-OUTR.GDTC.CASE-003** A case shall enter `submitted` only through a pull request whose author's forge account equals the case's credit account (compared ASCII-lowercased) and whose commits carry a DCO sign-off by that author; the optional credit display name is chosen by that account's holder and is shown only next to the account.
- **REQ-OUTR.GDTC.CASE-004** If any declared free-text field of a case or fixture file (title, question, credit display name, resolution note, history event text) contains an email address, a telephone number or a URL with an email, phone or messaging scheme, then the validator shall reject it with `contact_data` naming the file and field; structured fields (IDs, digests, numbers, generator parameters) shall not be scanned, because their grammars already exclude contact data.
- **REQ-OUTR.GDTC.CASE-005** State changes shall follow `submitted` → `published` → `in_discussion` → `resolved` → `fixture`, or any state before `resolved` → `rejected` with a reason from `customer_part`, `standards_text`, `contact_data`, `not_generated` or `duplicate`; the validator shall refuse any other transition.
- **REQ-OUTR.GDTC.CASE-006** When the published queue is non-empty, the featuring step shall select exactly one case per ISO week, the oldest published case not yet featured, and record the week in its history.
- **REQ-OUTR.GDTC.CASE-007** A resolved case shall name every rule ID it concerns with the rule's revision and content digest at resolution and an outcome from `rule_confirmed`, `rule_revised`, `new_rule_needed` or `out_of_scope`.
- **REQ-OUTR.GDTC.CASE-008** When a case reaches `fixture`, the pipeline shall write an ORS case under `fixtures/gdt/cases/<case-id>/` in the `UNC.ORS.TRACE` format with its expected values, tolerances, DMT row links, credit account and optional credit display name.
- **REQ-OUTR.GDTC.CASE-009** If a change removes a case fixture, or changes its expected values without a new suite version recording the reason, then the check shall fail with `fixture_not_permanent`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/gdtc/case/ok/submitted.json` (generator `gen:plane-pattern` with seed, an agreeing cross-check, a flatness callout), when `fid-outreach gdt-case check --cases cases/ --generators gens.json --xcheck xcheck.json --json` runs, then it exits 0.
- **AC-2** (error path) Given `extra_field.json` (a `submitter_email` field), then rejected for an unknown field; given `email_in_question.json` and `phone_in_title.json`, then `contact_data` naming file and field; given `digits_in_params.json` (a ten-digit generator seed), then it is accepted, because generator parameters are not free text.
- **AC-3** (error path) Given `uploaded_data.json` (point data with no generator) and `xcheck_disagrees.json`, then `not_generated`.
- **AC-4** (error path) Given `skip_state/` (`published` → `resolved`), then the transition is refused.
- **AC-5** Given `queue/` (three published cases, two already featured in weeks 2027-W01 and 2027-W02), when `fid-outreach gdt-case feature --week 2027-W03 --json` runs, then exactly the oldest unfeatured case is featured and its history records 2027-W03; running it again for 2027-W03 changes nothing.
- **AC-6** Given `resolved/` (outcome `rule_confirmed` naming one rule with revision and digest), when `fid-outreach gdt-case fixture --case C-0007 --json` runs, then `fixtures/gdt/cases/C-0007/` holds an ORS case that the `UNC.ORS.TRACE` validator accepts, with the credit account and DMT row link.
- **AC-7** (error path) Given a base/head pair where head deletes `fixtures/gdt/cases/C-0007/`, or changes its expected value with no new suite version, then `fixture_not_permanent`.
- **AC-8** (determinism) Running AC-6 twice gives byte-identical files.
- **AC-9** (credit, error path) Given a submission whose credit account is `alice-example` from a pull request authored by `bob-example`, when `fid-outreach gdt-case check --pr-author bob-example` runs, then it is rejected with `credit_not_author`; with `--pr-author Alice-Example`, it is accepted.

## Design

- **Command:** `fid-outreach gdt-case <check | feature | fixture | permanence>`; `check --cases <dir> --generators <file> --xcheck <file> [--pr-author <login>]` (CI passes the pull request author), `feature --week <YYYY-Www>`, `fixture --case <id>`, `permanence --base <dir> --head <dir>`; all `--json`. Project tooling; nearest class W1. The week is an input, never read from the clock.
- **Case file (`gdt-case/v1`, JSON, duplicate keys refused):** fields as REQ-001; `callout` is a `fid.gdt-resolve-char/1` document; `generator { id, params, seed }`; `xcheck` is the cross-check record ID from `UNC.RDL.GEN`; `resolution { rules: [{ rule, revision, content_digest, outcome }], date, note (≤ 280 chars) }`; `history: [{ date or week, event }]`.
- **Contact-data scan:** only the declared free-text fields of REQ-004. **Patterns:** RFC 5322-style address local part plus `@` plus a dotted domain; seven or more digits with optional separators and a leading `+`; URL schemes `mailto:`, `tel:`, `sms:` and messaging-app schemes listed in the tool's pattern file. False positives are fixed by rewording, never by suppression.
- **Algorithm:** validate fields and state; check the generator registry and cross-check record; scan the declared free-text fields for contact data; compare the credit account with `--pr-author`; for `fixture`, map to the ORS case format and write; for `permanence`, compare fixture trees and suite versions between base and head.
- **Errors:** `credit_not_author`, `unknown_field`, `not_generated`, `contact_data{file, field}`, `bad_transition{from, to}`, `fixture_not_permanent{case}`, `field_too_long`; exit 0, 1, 2.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `gdtc::cases`.

## Test fixtures

Hand-written case files, a stub generator registry and stub cross-check records under `fixtures/outr/gdtc/case/` (invented values, no standards text, no real names or contact data; CC-BY-4.0).

## Build prompt

> In crate `fid-outreach`, add module `gdtc::cases` and subcommand `fid-outreach gdt-case` with `check`, `feature --week <YYYY-Www>`, `fixture --case <id>` and `permanence --base <dir> --head <dir>` (all `--json`; exit 0, 1 findings, 2 unreadable). Case files `gdt-case/v1` (JSON, unknown fields and duplicate keys refused) hold only: case ID, title, credit account (forge login), optional credit display name, dialect and edition, callout (`fid.gdt-resolve-char/1`), generator ID/params/seed, oracle cross-check reference, expected values with tolerances, question (≤ 280 chars), state, discussion link, resolution (rules with revision, content digest and outcome `rule_confirmed` | `rule_revised` | `new_rule_needed` | `out_of_scope`) and history. Reject data not from a registered generator with an agreeing cross-check (`not_generated`); reject any email address, phone number or `mailto:`/`tel:`/`sms:`/messaging URL in the declared free-text fields only (`contact_data{file, field}`); with `--pr-author`, reject a case whose credit account differs from the PR author, compared ASCII-lowercased (`credit_not_author`); allow only the transitions submitted → published → in_discussion → resolved → fixture, or → rejected (reasons `customer_part`, `standards_text`, `contact_data`, `not_generated`, `duplicate`) before resolved. `feature` picks the oldest published, unfeatured case for the given week, idempotently. `fixture` writes an ORS case under `fixtures/gdt/cases/<case-id>/` in the `UNC.ORS.TRACE` format with expected values, tolerances, DMT row links, credit account and optional display name. `permanence` fails (`fixture_not_permanent`) when a case fixture is removed or its expected values change without a new suite version. Tests per AC-1..AC-9 with fixtures under `fixtures/outr/gdtc/case/`. Constraints: no panics, no `unsafe`; deny `unwrap`/`expect`/`panic`; no clock reads; BTreeMap only; never include standards text.

## Open questions

- Who resolves a case: the decision owner of the main rule concerned (`OUTR.GDTC.RULES`) records the resolution; a `rule_revised` outcome only opens a row revision, which then goes through review like any other.
- The `fixture` command and the `permanence` check need `UNC.ORS.TRACE` to accept a `credit` field; if it cannot, the credit stays in a sidecar file next to the case.

## Children

_Leaf: none._
