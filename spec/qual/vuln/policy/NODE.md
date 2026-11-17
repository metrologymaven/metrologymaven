---
id: QUAL.VULN.POLICY
title: SECURITY.md, reporting channel and policy check
level: 3
parent: QUAL.VULN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.VULN-001, REQ-QUAL.VULN-002]
covers: []
depends_on: []
uses_artifacts: [ECO.REL.LTS, ECO.REL.PROC.CHANNEL]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.VULN.POLICY: SECURITY.md, reporting channel and policy check

## Purpose

Gives every project repository the same `SECURITY.md`, generated from one source file so supported versions, reporting channels and target times never drift between repositories, and checks in CI that each repository's copy matches. The reporting channels include one that needs no account on any single commercial platform: encrypted e-mail to a project address with a published public key.

## Scope

**In:**
- Source file `security/policy.toml`: supported-version rules, channels, target response times, safe-harbour text reference, what a report should include
- Generator that writes `SECURITY.md` from the source file, the LTS line list and the channel definitions
- Check that each repository's `SECURITY.md` equals the generated text
- Channel rule: at least one channel of kind `email` with a published OpenPGP key fingerprint, and optionally a platform private-reporting channel

**Out** (and which node owns it instead):
- Handling reports once received (owned by `QUAL.VULN.TRIAGE`)
- Advisory publication (owned by `QUAL.VULN.ADVIS`)
- LTS line dates (owned by `ECO.REL.LTS`)

## Requirements

- **REQ-QUAL.VULN.POLICY-001** The generated `SECURITY.md` shall list the supported versions (each supported LTS line from `lts.toml` plus the latest stable minor), the reporting channels, what to include in a report, the acknowledgement and assessment target times and a safe-harbour statement.
- **REQ-QUAL.VULN.POLICY-002** If `policy.toml` has no channel of kind `email` with an OpenPGP key fingerprint, then the check shall fail.
- **REQ-QUAL.VULN.POLICY-003** If any repository listed in `security/repos.toml` has a `SECURITY.md` that differs from the generated text, or has none, then the check shall fail and name the repository.
- **REQ-QUAL.VULN.POLICY-004** The policy shall state that report content is never posted to a public issue, and the generator shall not emit any public issue tracker as a reporting channel.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/vuln/policy/ok/` (`policy.toml`, `lts.toml` with lines 1.2 supported and 1.0 ended, stable minor 1.4), when `cargo xtask qual security-md --policy policy.toml --lts lts.toml --stable 1.4 --out out/` runs, then `out/SECURITY.md` equals the golden file and lists 1.2 and 1.4 but not 1.0.
- **AC-2** (error path) Given `no-email/` (only a platform channel), then the check exits 1.
- **AC-3** (error path) Given `repos/` with one repository copy edited by hand, then `qual security-md --check --repos repos.toml` exits 1 naming that repository.
- **AC-4** (error path) Given a channel of kind `public_issue`, then the generator exits 1.

## Design

- **Registry command(s):** none; repository tool `cargo xtask qual security-md --policy <toml> --lts <toml> --stable <M.m> (--out <dir> | --check --repos <toml>)` (R0 in check mode; generation writes into a local output root).
- **Data structures:** `Policy { channels: Vec<Channel{ kind: Email|PlatformPrivate, address, pgp_fingerprint }>, targets: { ack_working_days, assessment_working_days }, include_list, safe_harbour }`.
- **Algorithm:** derive supported versions; render a fixed Markdown template; compare byte for byte after LF normalisation.
- **Errors:** `POLICY-PARSE`, `POLICY-NO-EMAIL`, `POLICY-PUBLIC-CHANNEL`, `POLICY-DRIFT(repo)`.
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `qual::vuln::policy`).

## Test fixtures

`fixtures/qual/vuln/policy/{ok,no-email,repos}/` with synthetic addresses and a test key fingerprint; CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::vuln::policy` with `qual security-md --policy <toml> --lts <toml> --stable <M.m> (--out <dir> | --check --repos <toml>)`. `policy.toml` holds channels (kind email with address and OpenPGP fingerprint, or platform_private), target times in working days (acknowledge 3, initial assessment 10, as maintainers set), the report checklist and the safe-harbour text. Supported versions = LTS lines with status supported in `lts.toml` plus the given stable minor. Render a fixed Markdown template; refuse with no email channel or any public-issue channel; in check mode compare each listed repository's SECURITY.md (LF-normalised) with the generated text. No panics, deny unwrap/expect. Tests: AC-1..AC-4 of spec node QUAL.VULN.POLICY.

## Open questions

- The project address, key custody and whether to also enable a platform private-reporting feature are maintainer choices (interacts with C7 and capacity).
- Target times (3 and 10 working days) are proposals [unverified, to be set by maintainers].

## Children

_Leaf: none._
