---
id: OUTR.GDTC.RULES
title: Public rule pages and rule governance register
level: 3
parent: OUTR.GDTC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.GDTC-001, REQ-OUTR.GDTC-002, REQ-OUTR.GDTC-009]
covers: []
depends_on: [OUTR.GDTC.GUARD, OUTR.BRAND.LINT]
uses_artifacts: [GDT.RES.STATUS, GDT.RES.SCHEMA, GDT.RES.ROWFLOW.ROSTER, OUTR.GDTC.REVIEW]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# OUTR.GDTC.RULES: Public rule pages and rule governance register

## Purpose

Every interpretation rule (DMT row or interpretation profile) gets one public page, so anyone can see what the project currently does for a callout, how settled it is, who decides and by when, where to discuss it, and how it changed. Status and sign-offs come only from the interpretation status register (`GDT.RES.STATUS`), never from hand-written text, so a page cannot claim more than the engine stamps into results. The human-maintained part is a small governance register: decision owner, deadline and discussion link per rule.

## Scope

**In:**
- Governance register `gdt-rule-governance/v1` (TOML, in the repository): per rule ID the decision owner, decision deadline, the date that deadline was set (`deadline_set`), discussion link and dated change notes
- Validation of the governance register against the status register
- Page generator: one page per rule ID, an index by status and by deadline, stable URLs
- Change history per rule from the status register revisions plus the change notes
- Reviewer credit on each page from the reviewer register (`OUTR.GDTC.REVIEW`) and roster display names
- Running the standards-text lint (`OUTR.GDTC.GUARD`) and the claim lint (`OUTR.BRAND.LINT`) on the output

**Out:**
- Statuses and sign-offs (owned by `GDT.RES.STATUS`)
- Reviewer qualification and consent to credit (owned by `OUTR.GDTC.REVIEW`)
- Escalating overdue deadlines (owned by `OUTR.GDTC.GUARD`)
- Site build and hosting (owned by `OUTR.SITE.GEN`)

## Requirements

- **REQ-OUTR.GDTC.RULES-001** For every rule ID in the status register, the generator shall write one page at the stable path `gdt/rules/<rule-slug>/` showing rule ID, kind, latest revision and content digest, status, decision owner, decision deadline, discussion link, the clause references of the row (designator, edition and clause number only) and the change history.
- **REQ-OUTR.GDTC.RULES-002** The status, revision, digest and sign-offs on a page shall be copied from the status register without change, and a page shall show the status register digest it was built from; if `verify_register` (`GDT.RES.STATUS`) does not accept the register, then the generator shall exit 1 and write no pages.
- **REQ-OUTR.GDTC.RULES-003** The change history shall list every revision in the status register with its revision, revision timestamp, content digest and status (including `superseded`), merged in date order with the governance register's change notes for that rule.
- **REQ-OUTR.GDTC.RULES-004** If a rule in the status register has no governance entry, or an entry lacks an owner, a deadline, a `deadline_set` date or a discussion link, then the generator shall exit 1 naming the rule and write no pages.
- **REQ-OUTR.GDTC.RULES-005** If the governance register names a rule that the status register does not hold, or a discussion link whose host is not an official channel in the `OUTR.COMM` channel directory, then the generator shall exit 1 naming the entry.
- **REQ-OUTR.GDTC.RULES-006** Every free-text field of the governance register (change notes, question summary) shall be at most 280 characters, and the generator shall refuse a longer one.
- **REQ-OUTR.GDTC.RULES-007** A page for a `signed_off` rule shall credit each reviewer whose approval counts towards that sign-off: by public display name only when the reviewer register records consent to credit, and otherwise by the reviewer's pseudonymous ID; no page shall show a reviewer name or organisation without recorded consent.
- **REQ-OUTR.GDTC.RULES-008** After writing pages, the generator shall run the standards-text lint and the claim lint over them, and if either reports an error, then it shall exit 1 and leave no output.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/gdtc/rules/ok/` (a status register with three rules, one per status, a complete governance register, a reviewer register and a channel directory), when `fid-outreach gdt-rule-pages --status status.json --governance governance.toml --dmt table.json --reviewers reviewers.toml --channels channels.toml --out out/ --json` runs, then it exits 0 and writes three pages and an index, each page showing every field of REQ-001 and the register digest.
- **AC-2** Given `ok/`, then the `signed_off` page credits both reviewers by display name and the `provisional` page credits nobody; with one reviewer's consent removed, that reviewer shows only as their pseudonymous ID (for example `rv-7f3a`) and neither the name nor the organisation appears anywhere in `out/`.
- **AC-3** Given `history/` (a rule with two revisions, the first `superseded`, and two change notes), then the history lists four items in date order.
- **AC-4** (error path) Given `no_owner/`, `no_deadline/`, `unknown_rule/` and `bad_link_host/`, then each exits 1 naming the rule or entry and `out/` is empty.
- **AC-5** (error path) Given `long_note/` (a 281-character change note), then exit 1 with `field_too_long`.
- **AC-6** (error path) Given `quoted_text/` (a change note that the standards-text lint flags), then exit 1 and `out/` is empty.
- **AC-7** (determinism) Running AC-1 twice gives byte-identical output folders.

## Design

- **Command:** `fid-outreach gdt-rule-pages --status <file> --governance <file> --dmt <file> --reviewers <file> --channels <file> --out <dir> [--json]`. Project tooling; nearest class W1 (writes only inside `--out`).
- **Governance register (`gdt-rule-governance/v1`):** `[[rule]] id`, `owner` (a maintainer's forge login, compared ASCII-lowercased), `deadline` (ISO date), `deadline_set` (ISO date, not after `deadline`), `discussion` (URL), `question` (optional, at most 280 characters), `[[rule.note]] date`, `text` (at most 280 characters). Duplicate rule IDs, duplicate keys and logins that differ only in case are refused.
- **Output:** Markdown with front matter for `OUTR.SITE.GEN`: `gdt/rules/index.md`, `gdt/rules/<rule-slug>/index.md`, `gdt/rules/manifest.json` with each page's source digests. The slug is the rule ID with `:`, `/` and `@` mapped to `-`, stable across versions.
- **Algorithm:** load and validate all inputs; join by rule ID; build history; render; write to a temporary folder; run the GUARD standards-text lint and the claim lint in-process; move into place on success.
- **Errors:** `governance_missing{rule}`, `governance_incomplete{rule, field}`, `unknown_rule`, `link_host_not_official`, `field_too_long`, `lint_failed`; exit 0, 1 findings, 2 unreadable input.
- **Determinism:** D0; dates come from inputs, never from the clock.
- **Crate:** `fid-outreach`, module `gdtc::rules`.

## Test fixtures

Hand-written synthetic registers under `fixtures/outr/gdtc/rules/` (invented rule IDs and clause numbers, invented display names, no standards text; CC-BY-4.0).

## Build prompt

> In crate `fid-outreach`, add module `gdtc::rules` and subcommand `fid-outreach gdt-rule-pages --status <file> --governance <file> --dmt <file> --reviewers <file> --channels <file> --out <dir> [--json]`. Inputs: the interpretation status register (`gdt-rule-status/v1` JSON), the governance register (`gdt-rule-governance/v1` TOML: per rule `id`, `owner` login, `deadline` date, `deadline_set` date, `discussion` URL, optional `question` ≤ 280 chars, dated `note`s ≤ 280 chars; refuse duplicate IDs, duplicate keys and logins differing only in case), the DMT table (for clause references only), the reviewer register (`gdt-reviewer-register/v1`, display names and credit consent) and the channel directory. Write one page per rule ID at `gdt/rules/<slug>/index.md` with rule ID, kind, latest revision and digest, status, owner, deadline, discussion link, clause references (designator, edition, clause number), history (every register revision with revision number, `revised_at`, digest, status including `superseded`, merged by date with the notes) and the register digest; credit reviewers of the current sign-off by display name only with recorded consent, otherwise by pseudonymous ID, and never print a name or organisation without consent. Copy status data from the register unchanged. Fail (exit 1, no output) when a register rule has no complete governance entry, a governance entry names an unknown rule, a discussion host is not an official channel, or a free-text field exceeds 280 characters. Write to a temp folder, run the GDTC standards-text lint and the existing claim lint, and move into place only if both pass. Tests per AC-1..AC-7 with fixtures under `fixtures/outr/gdtc/rules/`. Constraints: no panics, no `unsafe`; deny `unwrap`/`expect`/`panic`; no clock reads; BTreeMap only; never include standards text.

## Open questions

- Whether `draft` rules get public pages or only an index entry: this leaf gives them pages, because a public draft is how a community member finds a rule to discuss.

## Children

_Leaf: none._
