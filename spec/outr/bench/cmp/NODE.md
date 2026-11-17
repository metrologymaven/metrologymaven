---
id: OUTR.BENCH.CMP
title: Comparisons with incumbent products
level: 3
parent: OUTR.BENCH
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.BENCH-005]
covers: []
depends_on: [OUTR.BENCH.RUN]
uses_artifacts: [OUTR.BRAND.LINT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BENCH.CMP: Comparisons with incumbent products

## Purpose

Benchmark results that place the platform next to a named incumbent product on the same data. They are the strongest claims the project could make and the most exposed legally. D-021 (accepted) settles the policy: no incumbent comparisons in the first releases (option A); later, only comparisons run by third parties who hold a licence, published as their own signed statement next to our reproducible half (option B), and only after a counsel review. Project-run comparisons (option C) are not allowed. This node turns that policy into a recipe section and a publication gate that keeps every comparison unpublished until its conditions are met.

## Scope

**In:**
- The `[comparison]` recipe section (incumbent, version, settings, who ran it, licence basis, our reproducible half)
- The comparison register: one entry per incumbent with its counsel-review record, maintained by maintainers only
- The `bench-cmp gate` check that decides whether each comparison may be published
- The comparison fragment shown on result pages

**Out** (and which node owns it instead):
- Our own half of each result (owned by `OUTR.BENCH.ACC`, `OUTR.BENCH.FIDEL`, `OUTR.BENCH.PERF`)
- Running and publishing (owned by `OUTR.BENCH.RUN`)
- The claim lint and its CL002 rule (owned by `OUTR.BRAND.LINT`)
- The counsel review itself and any licence terms (people and business documents outside this tree)

## Requirements

- **REQ-OUTR.BENCH.CMP-001** The `[comparison]` section shall state the incumbent product name (one of the claim lint's incumbent-name list), its version, a published settings file with its SHA-256, the `our_recipe` ID of the project's reproducible half, the name of the third party who ran the incumbent, and a `statement` file with its SHA-256.
- **REQ-OUTR.BENCH.CMP-002** If `obtained_by` is anything other than `third_party`, then the validator shall fail with `project_run_not_allowed` (D-021 excludes project-run comparisons).
- **REQ-OUTR.BENCH.CMP-003** If the comparison register has no counsel-review entry for the recipe's incumbent with a review date and a reviewer reference, then the gate shall mark the comparison `not_publishable` with reason `counsel_review_missing`.
- **REQ-OUTR.BENCH.CMP-004** If the third party's attestation that their licence permits publishing the result is not recorded in the register entry for that statement digest, then the gate shall mark the comparison `not_publishable` with reason `licence_attestation_missing`.
- **REQ-OUTR.BENCH.CMP-005** The comparison fragment shall show the incumbent name, version, settings file link and digest, the third party's name, the statement as their own words, and a link to our half's result page, so a third party with that product can repeat the run on the published dataset.
- **REQ-OUTR.BENCH.CMP-006** If the recipe named in `our_recipe` does not exist or uses a different dataset digest, then the validator shall fail with `our_half_mismatch`.
- **REQ-OUTR.BENCH.CMP-007** Register entries shall be added or changed only by a pull request approved by a maintainer other than its author, and the gate shall refuse a register entry whose `recorded_by` equals its `approved_by`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/bench/cmp/empty-register/` (one comparison recipe and an empty register, as in the first releases), when `fid-outreach bench-cmp gate --recipes recipes/ --register register.toml --json` runs, then the comparison is `not_publishable` with `counsel_review_missing` and the command exits 0.
- **AC-2** Given `ready/` (register entry with counsel review for the incumbent and a licence attestation for the statement digest, recorded by `alice` and approved by `bob`), when the gate runs, then the comparison is `publishable`.
- **AC-3** (error path) Given `project-run.toml` (`obtained_by = "project"`), when `fid-outreach bench-recipe check` runs with this validator registered, then it exits 1 with `project_run_not_allowed`.
- **AC-4** Given `no-attestation/` (counsel review present, no attestation for the statement digest), when the gate runs, then the comparison is `not_publishable` with `licence_attestation_missing`.
- **AC-5** (error path) Given `self-approved/` (`recorded_by = approved_by = "alice"`), when the gate runs, then it exits 1 with `register_self_approved`.
- **AC-6** (error path) Given `wrong-half.toml` (`our_recipe` uses another dataset digest), when the check runs, then it exits 1 with `our_half_mismatch`.
- **AC-7** Given `ready/`, when `fid-outreach bench-cmp fragment` runs, then the fragment contains the incumbent name and version, the settings digest, the third party's name, the statement and a link starting with the benchmark prefix to our half's page.

## Design

- **Commands:** `fid-outreach bench-cmp gate --recipes <dir> --register <file> [--json]` and `fid-outreach bench-cmp fragment --recipe <file> --register <file> --bench-prefix <url> [--out <file>]`; kind validator `comparison` registered with `bench-recipe check`. Project tooling; nearest class R0. Recording a register entry is a human act through a reviewed pull request (human-only in spirit; no command writes the register).
- **Schema `[comparison]`:** `incumbent`, `incumbent_version`, `settings = { path, sha256 }`, `obtained_by = "third_party"`, `third_party`, `statement = { path, sha256 }`, `our_recipe`.
- **Register (`bench/comparisons/register.toml`):** `[[counsel]] incumbent, review_date, reviewer_ref, recorded_by, approved_by`; `[[attestation]] statement_sha256, third_party, date, recorded_by, approved_by`. The register holds references only; review notes and signed originals stay outside the public repository.
- **Gate outcome:** per comparison `publishable` or `not_publishable` with reasons; the harness treats `not_publishable` like REQ-OUTR.BENCH.RUN.HARNESS-004, so the pages show no comparison number.
- **Errors:** `project_run_not_allowed`, `our_half_mismatch`, `incumbent_unknown`, `register_self_approved`, `register_parse`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `bench::comparison`.

## Test fixtures

Hand-written recipes, registers, settings and statement files under `fixtures/outr/bench/cmp/` (`empty-register`, `ready`, `project-run.toml`, `no-attestation`, `self-approved`, `wrong-half.toml`), CC-BY-4.0. Incumbent names are taken from the claim lint's incumbent list and used nominatively; statements are fictional and say so.

## Build prompt

> In the Rust crate `fid-outreach`, add module `bench::comparison` with a `KindValidator` for kind `comparison` (registered with `bench::recipe`) and two subcommands: `fid-outreach bench-cmp gate --recipes <dir> --register <file> [--json]` and `fid-outreach bench-cmp fragment --recipe <file> --register <file> --bench-prefix <url> [--out <file>]`.
>
> Policy to enforce (project decision D-021): no comparison with a named competitor product is published until a counsel review for that product is recorded; only comparisons run by a third party who holds a licence for the competitor product are allowed, published as their own statement next to the project's reproducible result; project-run comparisons are refused.
>
> `[comparison]` section: `incumbent` (must be in `rules/incumbents.txt`), `incumbent_version`, `settings = { path, sha256 }`, `obtained_by` (only `third_party`; otherwise `project_run_not_allowed`), `third_party`, `statement = { path, sha256 }`, `our_recipe` (an existing recipe with the same dataset digest; otherwise `our_half_mismatch`).
>
> Register TOML: `[[counsel]]` entries (`incumbent`, `review_date`, `reviewer_ref`, `recorded_by`, `approved_by`) and `[[attestation]]` entries (`statement_sha256`, `third_party`, `date`, `recorded_by`, `approved_by`). Gate: `not_publishable` with `counsel_review_missing` or `licence_attestation_missing` when the matching entry is absent; exit 1 with `register_self_approved` when `recorded_by == approved_by`; exit 0 otherwise, listing every comparison's outcome sorted by recipe ID. Fragment: incumbent and version, settings link and digest, third party, statement presented as their words, link to the project's half under `--bench-prefix`.
>
> Tests with fixtures under `fixtures/outr/bench/cmp/` for: empty register (all `counsel_review_missing`), ready (publishable), project-run refused, missing attestation, self-approved register entry, wrong half, and fragment content.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; BTreeMap rather than HashMap; no network; allowlisted dependency licences only. Fixtures must not contain any competitor's text or screenshots.

## Open questions

- **D-021 condition:** counsel review must happen before this node's output is published or released. REQ-003 enforces it per incumbent; the empty register is the option-A state for the first releases.
- Incumbent licence terms may forbid publishing benchmark comparisons [unverified]; under option B each third party attests to their own terms (REQ-004), and counsel reviews the arrangement.
- Moving from option A to option B is a maintainer act (first counsel entry in the register); no further decision is needed unless option C is wanted.

## Children

_Leaf: none._
