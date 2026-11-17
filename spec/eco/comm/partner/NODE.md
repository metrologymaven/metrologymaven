---
id: ECO.COMM.PARTNER
title: Integrator and support-partner program
level: 3
parent: ECO.COMM
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.COMM-009]
covers: []
depends_on: []
uses_artifacts: [OUTR.PROG.CERT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.COMM.PARTNER: Integrator and support-partner program

## Purpose

Open-source metrology software is adopted through integrators who install, train, migrate and support. This leaf defines how a company becomes a listed partner, what its staff must have passed (certifications from OUTR.PROG.CERT), how partners escalate defects to maintainers, how maintainers employed by partners declare conflicts of interest, and the public directory data that lists partners. A standard-library check keeps the directory honest: expired listings and missing certifications fail. Commercial terms between partners and customers stay outside the tree.

## Scope

**In:**
- `partners/PROGRAM.md`: listing criteria, renewal, training requirements, escalation path and defect-report quality bar, conflict-of-interest rules, delisting
- `partners/partners.toml`: directory data, one entry per listed partner
- `partners/coi.toml`: maintainers' declared affiliations with partners
- `tools/partners.py check`: the directory check
- The escalation issue template (a defect report form with the quality bar fields)

**Out:**
- Commercial terms, pricing and contracts (business documents outside the tree)
- Certification curriculum, exams and records (owned by `OUTR.PROG.CERT`; read as data)
- Rendering the directory on the website (owned by `OUTR.SITE`)
- Maintainer nomination (owned by `ECO.COMM.LADDER` and GOVERNANCE.md)
- Use of the project name and logo by partners (follows D-002 and the trademark policy owned by `OUTR.BRAND`)

## Requirements

- **REQ-ECO.COMM.PARTNER-001** The project shall publish `partners/PROGRAM.md` stating listing criteria, renewal period, required certifications, escalation path to maintainers, the defect-report quality bar and conflict-of-interest rules, and the escalation issue template shall carry the quality-bar fields (version, platform, minimal reproduction or journal excerpt, expected and observed result).
- **REQ-ECO.COMM.PARTNER-002** Each `partners.toml` entry shall have `id`, `name`, `regions`, `services` (from `install`, `training`, `migration`, `support`, `integration`), `contact_url`, `listed_on`, `renew_by` and `certified_staff` (list of certification record IDs), and the check shall fail with `PARTNER-SCHEMA` on a missing or malformed field.
- **REQ-ECO.COMM.PARTNER-003** If `renew_by` is earlier than the `--today` input, then the check shall fail with `PARTNER-EXPIRED` naming the partner.
- **REQ-ECO.COMM.PARTNER-004** If a partner lists fewer certified staff than the minimum in `PROGRAM.md`'s machine-readable block (initially 2 [unverified as an appropriate threshold; maintainers adjust]), or a listed certification record ID is absent or expired in the supplied certification register, then the check shall fail with `PARTNER-CERT`.
- **REQ-ECO.COMM.PARTNER-005** If a maintainer handle appears in `coi.toml` with an affiliation to a partner ID that is not in `partners.toml`, or a partner entry claims a maintainer affiliation that `coi.toml` lacks, then the check shall fail with `PARTNER-COI`.
- **REQ-ECO.COMM.PARTNER-006** The check shall run with only the Python standard library, offline, and print findings sorted by partner ID and code.

## Acceptance criteria

- **AC-1** Given `fixtures/partners/ok/` (two partners, each with two valid certification records in `certs.json`, consistent `coi.toml`), when `python tools/partners.py check --dir fixtures/partners/ok --certs fixtures/partners/ok/certs.json --today 2027-05-01` runs, then it exits 0.
- **AC-2** (error path) With `--today 2028-05-01` on the same fixture, then both partners fail with `PARTNER-EXPIRED`.
- **AC-3** (error path) Given `fixtures/partners/one-cert/`, then `PARTNER-CERT acme`; given a certification ID missing from `certs.json`, then `PARTNER-CERT` naming the ID.
- **AC-4** (error path) Given `fixtures/partners/coi-mismatch/`, then `PARTNER-COI`.
- **AC-5** `partners/PROGRAM.md` contains the sections Listing criteria, Renewal, Certification, Escalation, Defect-report quality bar, Conflicts of interest and Delisting, and the escalation issue template exists.
- **AC-6** (determinism) Output is byte-identical across two runs on Windows, Linux and macOS.

## Design

- **Registry command(s):** none; repository tool `python tools/partners.py check --dir partners --certs <json> --today <date>`.
- **Data structures:** `partners.toml` as REQ-002 (TOML parsed with the standard library's TOML reader); `coi.toml` `[[affiliation]] maintainer, partner, since`; certification register JSON `[{id, holder_org, level, valid_until}]` (format to be published by OUTR.PROG.CERT); program parameters in a fenced `toml` block in `PROGRAM.md` (`min_certified_staff`, `renewal_months`).
- **Algorithm:** parse; validate; cross-check certificates and affiliations; sort findings.
- **Errors:** `PARTNER-SCHEMA`, `PARTNER-EXPIRED`, `PARTNER-CERT`, `PARTNER-COI`, `PARTNER-IO`.
- **Determinism:** D0; the date is an input.
- **Performance budget:** under 1 s for 500 partners [unverified until measured].
- **Crate:** not Rust; `tools/partners.py` (Python 3.11+ standard library, for `tomllib`).

## Test fixtures

Hand-written directory, affiliation and certificate files under `fixtures/partners/` (MIT OR Apache-2.0); no real company data.

## Build prompt

> In the spec repository (shared with code repositories later), write `partners/PROGRAM.md` (sections: Listing criteria, Renewal, Certification, Escalation, Defect-report quality bar, Conflicts of interest, Delisting; a fenced `toml` block with `min_certified_staff = 2` and `renewal_months = 12`), an empty `partners/partners.toml`, `partners/coi.toml`, an escalation issue template with fields version, platform, minimal reproduction or journal excerpt, expected and observed result, and `tools/partners.py check --dir <partners> --certs <json> --today <YYYY-MM-DD>` using only the Python 3.11+ standard library. Findings sorted by partner then code: `PARTNER-SCHEMA` (fields id, name, regions, services ⊆ {install, training, migration, support, integration}, contact_url, listed_on, renew_by, certified_staff), `PARTNER-EXPIRED` (renew_by < today), `PARTNER-CERT` (fewer than min certified staff, or a cert ID missing or expired at today in the cert register `[{id, holder_org, level, valid_until}]`), `PARTNER-COI` (affiliations in `coi.toml` and partner entries disagree), `PARTNER-IO`. Exit 1 on any finding. Tests for AC-1..AC-6 of spec node ECO.COMM.PARTNER. No commercial terms in any file.

## Open questions

- Program parameters (minimum certified staff, renewal period) are proposals for maintainers to set in the PR; they are data, so changing them needs no code change.
- The certification register format belongs to OUTR.PROG.CERT, which is still open; until it publishes one, the check reads the JSON shape above.
- Whether listed partners may use the project name in their marketing depends on D-002 and the trademark policy; PROGRAM.md links that policy rather than restating it.

## Children

_Leaf: none._
