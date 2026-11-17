---
id: ECO.REL.REPRO.REBUILD
title: Independent rebuild, comparison report and provenance
level: 4
parent: ECO.REL.REPRO
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.REPRO-001, REQ-ECO.REL.REPRO-003]
covers: []
depends_on: [ECO.CI.WF, ECO.REL.REPRO.PIN, ECO.REL.REPRO.STRIP]
uses_artifacts: [ECO.REL.PKG.PLAN, ECO.REL.PROC.CHANNEL, TRUST.SIGN.DSSE]
side_effect: W1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.REPRO.REBUILD: Independent rebuild, comparison report and provenance

## Purpose

For every release, rebuilds every artefact a second time on a fresh runner from a clean pinned image, compares each pair with the per-format rule, and publishes `rebuild-report.json`. It also writes a build provenance statement per artefact (source commit, builder image digest, pinned environment digest, output digest) so auditors can link a signed installer to its inputs.

## Scope

**In:**
- Second build job on a separate runner instance, started after the primary build, with no shared cache
- `rebuild-report.json` listing every artefact with its comparison status
- Provenance statements as in-toto style statements in DSSE envelopes [unverified: exact SLSA provenance predicate version], signed in the release signing step
- Release-note and evidence-bundle hand-off of the report digest

**Out:**
- Pinning (owned by `ECO.REL.REPRO.PIN`) and comparison rules (owned by `ECO.REL.REPRO.STRIP`)
- Signing the statements (owned by `ECO.REL.SIGN.CUSTODY`)
- The third-party recipe (owned by `ECO.REL.REPRO.RECIPE`)

## Requirements

- **REQ-ECO.REL.REPRO.REBUILD-001** For every artefact in the package plan and the web bundle, the rebuild job shall produce a second copy on a runner instance distinct from the primary build's, with no shared build cache.
- **REQ-ECO.REL.REPRO.REBUILD-002** The rebuild job shall pass the pin check before building and shall fail the rebuild (not the release) if the pin check fails.
- **REQ-ECO.REL.REPRO.REBUILD-003** `rebuild-report.json` shall list every artefact with its format, its status from the comparison tool, and the SHA-256 of both copies.
- **REQ-ECO.REL.REPRO.REBUILD-004** If the `fid` CLI, `fid-verify` or the web bundle is not `identical` or `identical_after_strip`, then the report shall mark the release `repro_regression`, and the next release's gate input shall include it.
- **REQ-ECO.REL.REPRO.REBUILD-005** For every artefact, the job shall write a provenance statement naming the source repository and commit, builder image digest, `build-env.lock` digest, build command and output SHA-256.

## Acceptance criteria

- **AC-1** Given the fixture release workflow run in the simulator (`cargo xtask rel flow-sim --fixtures fixtures/rel/rebuild/pass/`), when the rebuild step runs with stub artefacts that compare identical, then `rebuild-report.json` equals `expected-report.json`.
- **AC-2** (error path) Given stub outputs where `fid-verify` differs by one byte, then the report marks `repro_regression` naming `fid-verify`.
- **AC-3** (error path) Given a rebuild environment that fails the pin check, then the rebuild step fails with `PIN-MISMATCH`, the release state is unchanged and the report records `rebuild_not_run`.
- **AC-4** Given the provenance statement for a fixture artefact, then it validates against `schemas/provenance.v1.json` and its subject digest equals the artefact's SHA-256.

## Design

- **Registry command(s):** none; CI job plus `cargo xtask rel rebuild-report --primary <dir> --second <dir> --formats repro-formats.toml --out <dir>` and `rel provenance --artefacts <dir> --env build-env.lock --commit <sha> --out <dir>` (W1).
- **Data structures:** `RebuildReport { release, artefacts: [{name, format, status, sha256_primary, sha256_second}], repro_regression }`; provenance statement fields as listed in REQ-005.
- **Algorithm:** call the STRIP comparison for each pair; aggregate; write canonical JSON.
- **Errors:** `REBUILD-MISSING(artefact)`, `PIN-MISMATCH`.
- **Determinism:** N for the CI job; the report generator is D0.
- **Performance budget:** the rebuild doubles release build time; acceptable within the release time budget [unverified].
- **Crate:** `fid-xtask` (module `rel::rebuild`); workflow job.

## Test fixtures

`fixtures/rel/rebuild/` (pass, verify-differs, pin-fail) with stub artefact pairs; CC-BY-4.0.

## Build prompt

> Add a rebuild job to the release workflow: after the primary build, on a separate runner instance from a clean pinned image with no shared cache, run `rel pin-check --probe`, rebuild every planned artefact and the web bundle, then run `cargo xtask rel rebuild-report --primary <dir> --second <dir> --formats repro-formats.toml --out <dir>` which calls the ECO.REL.REPRO.STRIP comparison per pair and writes `rebuild-report.json` (name, format, status, both SHA-256, `repro_regression` if fid, fid-verify or the web bundle is not identical or identical_after_strip). Add `rel provenance` writing one statement per artefact (repository, commit, builder image digest, build-env.lock digest, build command, subject SHA-256) as canonical JSON payload for DSSE signing in the signing step, with schema `schemas/provenance.v1.json`. A pin failure fails the rebuild job only. Tests: AC-1..AC-4 of spec node ECO.REL.REPRO.REBUILD. Constraints: no panics, deny unwrap/expect.

## Open questions

- The rebuild runs after signing approval in the runbook order (step `rebuild-compare`), so it does not delay approval; a regression is reported in the next release's gates rather than blocking the current one. Maintainers may prefer it to block.

## Children

_Leaf: none._
