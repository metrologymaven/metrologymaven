---
id: OUTR.GDTC.PROFILE
title: Interpretation profile registry: cap, sponsors and default
level: 3
parent: OUTR.GDTC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.GDTC-003]
covers: []
depends_on: [OUTR.GDTC.GUARD]
uses_artifacts: [GDT.RES.STATUS, GDT.CHAR.DPROF]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# OUTR.GDTC.PROFILE: Interpretation profile registry: cap, sponsors and default

## Purpose

An interpretation profile is a named set of rule choices: in the engine, a defaults profile (`GDT.CHAR.DPROF`). Examples are a strict reading of one standard edition, a dialect's default reading, or one organisation's documented practice. Profiles let real disagreements be stated precisely and compared (`GDT.CHAR.PDIFF`) instead of argued in prose. Too many profiles would split reviewer attention and make every comparison table unreadable, so the project publishes only a capped number, each kept alive by a sponsor, with exactly one default per dialect and edition. This leaf owns that registry, its checks and the profile pages. Users can still create local profiles in their own projects; those are not published and are always labelled provisional in results (`GDT.RES.STATUS`, `not_in_register`).

## Scope

**In:**
- Registry `gdt-profiles/v1` (TOML, in the repository): per profile its full profile ID, title, kind (`edition_strict`, `dialect_default`, `organisation_practice`), sponsor, date of last sponsor confirmation, state (`active` or `retired`) and default flag
- Checks: cap on active profiles, sponsor present and confirmed within 12 months, one default per dialect and edition, every profile published in the status register
- Profile pages: rule choices, review status, sponsor, and the choices where the profile differs from its dialect's default

**Out:**
- Profile objects and their immutability (owned by `GDT.CHAR.DPROF`)
- Profile sign-off and status (owned by `GDT.RES.STATUS` and `GDT.RES.ROWFLOW`)
- Numeric comparison on data (owned by `GDT.CHAR.PDIFF`)
- Recording the profile in each result (owned by `GDT.CHAR.INTERP`, REQ-GDT-001)

## Requirements

- **REQ-OUTR.GDTC.PROFILE-001** The registry check shall fail when more than 8 profiles are `active` [cap proposed; maintainers confirm by RFC].
- **REQ-OUTR.GDTC.PROFILE-002** The registry check shall fail for every `active` profile with no sponsor, or whose last sponsor confirmation is more than 12 months before the `--as-of` date.
- **REQ-OUTR.GDTC.PROFILE-003** The registry check shall fail unless, for every (dialect, edition) that has an `active` profile, exactly one `active` profile of that dialect and edition carries the default flag.
- **REQ-OUTR.GDTC.PROFILE-004** The registry check shall fail for every profile ID that the status register does not hold as a published profile.
- **REQ-OUTR.GDTC.PROFILE-005** Each profile page shall show the full profile ID with digest, kind, sponsor, state, default flag, its status from the status register, and every rule choice in which it differs from the default profile of its dialect and edition.
- **REQ-OUTR.GDTC.PROFILE-006** Every page shall state that the default reaches a result only by being written into the characteristic's governing spec, so every result names its profile explicitly (REQ-GDT-001, REQ-GDT.CHAR-003); the registry's default flag shall never be read by the engine.
- **REQ-OUTR.GDTC.PROFILE-007** A `retired` profile shall keep its page, marked retired with the retirement date, so results that name it stay traceable.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/gdtc/profile/ok/` (5 active profiles, 1 retired, one default per dialect and edition, all sponsors confirmed within 12 months of the `--as-of` date), when `fid-outreach gdt-profile-pages --registry profiles.toml --status status.json --profiles profiles/ --as-of 2027-01-15 --out out/ --json` runs, then it exits 0 and writes 6 pages and an index.
- **AC-2** (error path) Given `over_cap/` (9 active), then exit 1 naming the cap.
- **AC-3** (error path) Given `no_sponsor/` and `stale_sponsor/` (confirmed 2025-12-01, `--as-of` 2027-01-15), then exit 1 naming each profile.
- **AC-4** (error path) Given `two_defaults/` and `no_default/`, then exit 1 naming the dialect and edition.
- **AC-5** (error path) Given `unpublished/` (a profile ID absent from the status register), then exit 1 naming it.
- **AC-6** Given `ok/`, then the page of a non-default profile lists exactly the keys whose values differ from the default's, and the retired profile's page is marked retired.
- **AC-7** (determinism) Running AC-1 twice gives byte-identical output.

## Design

- **Command:** `fid-outreach gdt-profile-pages --registry <file> --status <file> --profiles <dir> --as-of <date> --out <dir> [--json]`. Project tooling; nearest class W1. `--as-of` replaces the clock.
- **Registry (`gdt-profiles/v1`):** `[[profile]] id` (full `dp:` reference with digest), `title` (at most 80 characters), `kind`, `sponsor` (an organisation name or a forge login, published with the sponsor's consent; no contact data), `sponsor_org` (the sponsoring organisation's pseudonymous `org-…` ID from the reviewer register's ID space, used by conflict checks), `confirmed` (date), `state`, `default` (bool), `retired` (date, required when retired). Duplicate IDs and duplicate keys are refused.
- **Algorithm:** validate the registry; join with the status register and the profile files (`fid.defaults-profile/1`); compute key differences against the default of the same dialect and edition; render; run the GUARD standards-text lint on the output; move into place.
- **Errors:** `over_cap`, `sponsor_missing`, `sponsor_stale`, `default_count{dialect, edition}`, `unpublished_profile`, `lint_failed`; exit 0, 1, 2.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `gdtc::profiles`.

## Test fixtures

Synthetic registries and small placeholder profiles under `fixtures/outr/gdtc/profile/` (invented keys and values, invented sponsor names; CC-BY-4.0).

## Build prompt

> In crate `fid-outreach`, add module `gdtc::profiles` and subcommand `fid-outreach gdt-profile-pages --registry <file> --status <file> --profiles <dir> --as-of <date> --out <dir> [--json]`. The registry `gdt-profiles/v1` (TOML) lists profiles with full `dp:` ID, title (≤ 80 chars), kind (`edition_strict` | `dialect_default` | `organisation_practice`), sponsor (name or login, no contact data), `confirmed` date, state (`active` | `retired`, with `retired` date), `default` flag; refuse duplicate IDs and keys. Fail (exit 1, no output) when more than 8 profiles are active, an active profile has no sponsor or was last confirmed more than 12 months before `--as-of`, a (dialect, edition) with active profiles does not have exactly one active default, or a profile ID is not a published profile in the status register (`gdt-rule-status/v1`). Write one page per profile (ID with digest, kind, sponsor, state, default flag, status from the register, the rule choices that differ from the default of its dialect and edition, a retired mark when retired) and an index, each stating that the default reaches results only through the characteristic's governing spec. Run the GDTC standards-text lint on the output before moving it into place. Tests per AC-1..AC-7 with fixtures under `fixtures/outr/gdtc/profile/`. Constraints: no panics, no `unsafe`; deny `unwrap`/`expect`/`panic`; no clock reads; BTreeMap only; never include standards text.

## Open questions

- The cap of 8 and the 12-month confirmation are proposals (see `OUTR.GDTC`).
- A sponsor's organisation-practice profile describes that organisation's choices in the project's own words; it is not an endorsement by the project, and the page says so.

## Children

_Leaf: none._
