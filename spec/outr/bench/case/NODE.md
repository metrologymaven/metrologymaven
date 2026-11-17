---
id: OUTR.BENCH.CASE
title: User case studies
level: 3
parent: OUTR.BENCH
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.BENCH-007, REQ-OUTR.BENCH-008]
covers: []
depends_on: [OUTR.BRAND.LINT]
uses_artifacts: [TRUST.VERIFY.CLI, TRUST.VERIFY.REPORT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BENCH.CASE: User case studies

## Purpose

Case studies written with organisations that use the platform, published only with their written consent to the exact text, with every number either verifiable from a result package or clearly labelled as the organisation's own statement. It involves third parties, consent records and confidential data, so the rules are enforced by a gate that the site build runs before a case study can be published, and by a short written procedure for consent and withdrawal.

## Scope

**In:** the case-study source format (Markdown with a claims table); the consent and withdrawal procedure (a page in the outreach repository); the consent register format, kept outside the public repository; the `case-gate` check (consent, claims, withdrawal deadline, claim lint).
**Out:** the verifier and its report schema (`TRUST.VERIFY.CLI`, `TRUST.VERIFY.REPORT`); the claim lint rules (`OUTR.BRAND.LINT`); hosting (`OUTR.SITE`); paid partnerships or sales references (business documents outside this tree).

## Requirements

- **REQ-OUTR.BENCH.CASE-001** When the gate runs on a case study, it shall compute the SHA-256 of the case study's normalised text and fail with `consent_missing` unless the consent register holds an unwithdrawn consent record for that organisation and case whose text digest equals it.
- **REQ-OUTR.BENCH.CASE-002** Each quantitative claim shall appear in the case study's claims table as either `verified` with a result-package path and SHA-256, or `own_statement` attributed to the organisation, and any number in the body outside a declared claim (other than dates and version numbers) shall fail with `unmarked_number`.
- **REQ-OUTR.BENCH.CASE-003** For each `verified` claim, the gate shall run the released `fid-verify` on the package and fail with `claim_not_verified` unless the verification report's verdict is a pass.
- **REQ-OUTR.BENCH.CASE-004** Each `own_statement` claim shall be rendered with the label "Statement by <organisation>", and the gate shall fail with `statement_unlabelled` if the label is missing.
- **REQ-OUTR.BENCH.CASE-005** If a consent record is marked withdrawn, then the gate shall fail with `consent_withdrawn` for that case study, and shall fail with `withdrawal_overdue` if the case study is still in the site sources 14 days [unverified target] after the withdrawal date.
- **REQ-OUTR.BENCH.CASE-006** If the consent register path given to the gate lies inside the public repository's working tree, then the gate shall fail with `consent_in_repo` and read nothing from it.
- **REQ-OUTR.BENCH.CASE-007** The gate shall run the outreach claim lint on each case study and fail on any lint error.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/bench/case/ok/` (a case study with one verified claim backed by a passing test package, one labelled own statement, and a consent register in a temp folder outside the repository whose digest matches), when `fid-outreach case-gate --cases cases/ --consent <tmp>/consent.toml --verify-bin <fid-verify> --today 2027-05-01 --lint-rules claim-rules.toml --json` runs, then it exits 0.
- **AC-2** (error path) Given `edited/` (one word changed after consent), when the gate runs, then it exits 1 with `consent_missing`.
- **AC-3** (error path) Given `bad-package/` (the package fails verification), when the gate runs, then it exits 1 with `claim_not_verified`.
- **AC-4** (error path) Given `stray-number/` (the body says "cut inspection time by 40 %" outside any claim), when the gate runs, then it exits 1 with `unmarked_number`.
- **AC-5** Given `withdrawn/` (withdrawn on 2027-04-01) and `--today 2027-04-10`, when the gate runs, then it exits 1 with `consent_withdrawn`; with `--today 2027-04-20`, it also reports `withdrawal_overdue`.
- **AC-6** (error path) Given `--consent` pointing inside the repository checkout, when the gate runs, then it exits 2 with `consent_in_repo`.
- **AC-7** (error path) Given `unlabelled/` (an own statement without the label), when the gate runs, then it exits 1 with `statement_unlabelled`.
- **AC-8** (determinism) Given the same inputs and `--today`, when the gate runs twice, then the JSON reports are byte-identical.

## Design

- **Command:** `fid-outreach case-gate --cases <dir> --consent <file> --verify-bin <path> --today <date> --lint-rules <file> [--repo-root <dir>] [--json]`. Project tooling; nearest class R0. Recording consent is a human act by a maintainer (human-only); no command writes the consent register.
- **Case-study source:** Markdown with front matter `case_id`, `organisation`, `claims = [{ id, kind = "verified" | "own_statement", package, sha256 }]`; claims are marked in the body as `[[claim:<id>]]...[[/claim]]`.
- **Normalised text:** front matter and body with line endings as LF, trailing spaces removed, Unicode NFC; SHA-256 over the bytes.
- **Consent register (private):** `[[consent]] case_id, organisation, text_sha256, date, signer_role, recorded_by, withdrawn_on?`; signed originals stay with the maintainers outside any repository.
- **Number check:** a number is a digit run (with decimal separator) outside claim spans, code, links and front matter; dates (ISO 8601) and version strings (`v` prefix or dotted triples) are exempt.
- **Procedure page:** `bench/case-studies/PROCEDURE.md` in the outreach repository: how consent to the exact text is collected, where records are kept, who may read them, and that withdrawal removes the page within 14 days.
- **Errors:** `consent_missing`, `consent_withdrawn`, `withdrawal_overdue`, `consent_in_repo`, `claim_not_verified`, `unmarked_number`, `statement_unlabelled`, `lint_failed`; exit 0 ok, 1 findings, 2 setup error.
- **Determinism:** D0 given `--today` (no clock reads).
- **Crate:** `fid-outreach`, module `case_gate`.

## Test fixtures

Fictional case studies and consent registers under `fixtures/outr/bench/case/` (`ok`, `edited`, `bad-package`, `stray-number`, `withdrawn`, `unlabelled`), CC-BY-4.0, with result packages taken from the TRUST.VERIFY test fixtures (one passing, one tampered). The organisation names are fictional.

## Build prompt

> In the Rust crate `fid-outreach`, add module `case_gate` and subcommand `fid-outreach case-gate --cases <dir> --consent <file> --verify-bin <path> --today <date> --lint-rules <file> [--repo-root <dir>] [--json]`, and write `bench/case-studies/PROCEDURE.md` (consent to the exact text, where records live, who reads them, withdrawal within 14 days) in your own words.
>
> A case study is Markdown with front matter `case_id`, `organisation` and `claims` (each `id`, `kind` = `verified` or `own_statement`, and for verified a result-package `package` path and `sha256`); claims are marked in the body with `[[claim:<id>]]...[[/claim]]`.
>
> Checks per case study: (1) SHA-256 of the normalised text (LF, no trailing spaces, NFC) must equal an unwithdrawn consent record's `text_sha256` for that `case_id` and organisation, else `consent_missing`; (2) a withdrawn record gives `consent_withdrawn`, and `withdrawal_overdue` when `--today` is 14 or more days after `withdrawn_on`; (3) refuse with `consent_in_repo` (exit 2) when `--consent` resolves inside `--repo-root` (default: the git top level); (4) for each verified claim, check the package digest and run `--verify-bin verify-package <pkg> --json`, then require a pass verdict in the report, else `claim_not_verified`; (5) own statements must carry the label `Statement by <organisation>`, else `statement_unlabelled`; (6) any number outside claim spans, code, links and front matter, other than ISO dates and version strings, gives `unmarked_number`; (7) run the existing `claim_lint` module and fail on any error.
>
> Output findings sorted by case ID and code; RFC 8785 JSON with `--json`; exit 0, 1 findings, 2 setup errors. No clock reads: use `--today`.
>
> Tests with fixtures under `fixtures/outr/bench/case/` for: ok, edited after consent, failing package, stray number, withdrawn and overdue, consent inside the repo, unlabelled statement, byte-identical output.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; never print consent-register contents beyond case ID and status; never copy the consent register into outputs; BTreeMap rather than HashMap; allowlisted dependency licences only.

## Open questions

- Phase P1b: a case study needs a real user of a released app.
- The 14-day withdrawal target is a proposal for maintainers to confirm.
- Where maintainers keep the private consent register (which tool, which access control) is their choice; the gate only needs a readable path outside the repository.
- The former code dependency on TRUST.VERIFY is a use of the released `fid-verify` binary and its report schema, now `uses_artifacts` (CONSTITUTION 2.1, D-034).

## Children

_Leaf: none._
