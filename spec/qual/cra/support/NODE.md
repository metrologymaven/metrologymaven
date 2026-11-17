---
id: QUAL.CRA.SUPPORT
title: Support-period statement and security-update policy
level: 3
parent: QUAL.CRA
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.CRA-001]
covers: []
depends_on: []
uses_artifacts: [ECO.REL.LTS, ECO.REL.PROC.CHANNEL, ECO.APPS]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.CRA.SUPPORT: Support-period statement and security-update policy

## Purpose

Publishes, per product line (each focused app and the `fid` CLI and verifier), how many months it receives security updates, which versions get fixes and how fixes are delivered, with a counsel review status. The statement is data so the release gate can read its review status and a check can keep it consistent with the LTS lines the release process actually maintains.

## Scope

**In:**
- `compliance/cra/support.toml` schema `qual-cra-support/1`: per product line, support period in months, versions receiving fixes (rule such as "latest stable minor and every supported LTS line"), delivery (new patch release through the normal channels; no in-product download per `ECO.REL.UPD`), review status `draft|counsel_reviewed` with reviewer reference
- Rendered page `compliance/cra/SUPPORT.md`
- Consistency check: every product line in the app list has an entry; the support period is at least the LTS minimum window; every LTS line marked supported is within a stated period

**Out** (and which node owns it instead):
- LTS line dates and backport policy (owned by `ECO.REL.LTS`)
- The distribution gate status file (owned by `QUAL.CRA.CLASS`)
- Legal sufficiency of the period (counsel)

## Requirements

- **REQ-QUAL.CRA.SUPPORT-001** The support statement shall give, for every product line in the app list plus `fid` and `fid-verify`, a support period in whole months, the rule naming which versions receive security fixes, the delivery method and a review status of `draft` or `counsel_reviewed`.
- **REQ-QUAL.CRA.SUPPORT-002** If a product line in the app list has no entry, or an entry's period is shorter than the minimum LTS window in `lts.toml`, then the check shall fail and name the line.
- **REQ-QUAL.CRA.SUPPORT-003** If an entry has review status `counsel_reviewed` without a reviewer reference, then the check shall fail.
- **REQ-QUAL.CRA.SUPPORT-004** The rendered page shall be generated from the data file, shall cite legal acts by designator and article number only, and shall show the review status of each entry.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/cra/support/ok/` (app list with Inspect and Report, `lts.toml` with `min_window_months = 24`, `support.toml` with 4 entries of 36 months), when `cargo xtask qual cra support --data support.toml --apps apps.toml --lts lts.toml --out out/` runs, then it exits 0 and `out/SUPPORT.md` equals the golden file.
- **AC-2** (error path) Given `missing-line/` (no Report entry), then it exits 1 naming `report`.
- **AC-3** (error path) Given `short/` (Inspect 12 months), then it exits 1 naming `inspect`.
- **AC-4** (error path) Given `no-reviewer/`, then it exits 1.

## Design

- **Registry command(s):** none; repository tool `cargo xtask qual cra support --data <toml> --apps <toml> --lts <toml> --out <dir>` (W1).
- **Data structures:** `SupportEntry { line, months, fixes_rule, delivery, review: { status: Draft|CounselReviewed, reviewer_ref } }`.
- **Algorithm:** load app list (`apps.toml` from `ECO.APPS`), LTS policy; check; render a fixed template.
- **Errors:** `CRA-SUPPORT-PARSE`, `CRA-SUPPORT-MISSING(line)`, `CRA-SUPPORT-SHORT(line)`, `CRA-SUPPORT-REVIEWER(line)`.
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `qual::cra::support`).

## Test fixtures

`fixtures/qual/cra/support/{ok,missing-line,short,no-reviewer}/`; synthetic, CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::cra::support` with `qual cra support --data <support.toml> --apps <apps.toml> --lts <lts.toml> --out <dir>`. Schema `qual-cra-support/1`: per product line (each app in apps.toml plus `fid` and `fid-verify`) months, fixes_rule, delivery, review {status draft|counsel_reviewed, reviewer_ref}. Fail on a missing line, a period below `lts.toml` `[policy] min_window_months`, or counsel_reviewed without reviewer_ref. Render `SUPPORT.md` from a fixed template showing each status; cite legal acts by designator and article number only (never quote). No panics, deny unwrap/expect. Tests: AC-1..AC-4 of spec node QUAL.CRA.SUPPORT.

## Open questions

- The actual support periods are maintainer decisions with counsel input; the data file starts with every entry `draft`.

## Children

_Leaf: none._
