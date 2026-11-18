---
id: ECO.REL.PROC.FLOW
title: Release workflow from tag to publication
level: 4
parent: ECO.REL.PROC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.PROC-002]
covers: []
depends_on: [ECO.CI.WF, ECO.REL.PROC.VERSION, ECO.REL.PROC.CHANNEL, ECO.REL.PROC.GATES, ECO.REL.PROC.NOTES]
uses_artifacts: [ECO.REL.PKG.PLAN, ECO.REL.WEB.ASSEMBLE, ECO.REL.SIGN.CUSTODY, ECO.REL.SIGN.SUMS, ECO.REL.SIGN.SBOM, ECO.REL.SIGN.OSSIGN, ECO.REL.REPRO.REBUILD, ECO.PARITY.GEN, ECO.PARITY.STATUS, ECO.DATA.BENCH]
side_effect: X1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.PROC.FLOW: Release workflow from tag to publication

## Purpose

The CI workflow that a release tag starts. It runs every runbook step in order, records progress in one state file, stops at "awaiting signing approval" when the gates pass, and after a person approves runs the post-approval steps (OS signing, checksums, statement signing, publication to the enabled venues, feed update, rebuild comparison) without further human action. A re-run resumes from the last completed step and never repeats a publication.

## Scope

**In:**
- The release workflow file (in the CI system chosen by `ECO.CI.WF`) with one job per `ci` runbook step
- `release-state.json`: state machine `tagged → built → gated → awaiting_approval → approved → signed → published → feeds_updated`, plus `blocked` and `failed`
- Pre-sign manifest digest: the digest of the unsigned artefact set that the approver approves
- Idempotent publication to the venues `release-gates.json` leaves enabled
- Nightly variant: scheduled, no approval step, nightly key only

**Out:**
- What each step computes (owned by the producer nodes in `uses_artifacts`)
- Key custody and the approval mechanism (owned by `ECO.REL.SIGN.CUSTODY`)
- Gate rules (owned by `ECO.REL.PROC.GATES`)

## Requirements

- **REQ-ECO.REL.PROC.FLOW-001** When a tag matching the release tag format is pushed, the workflow shall run every `ci` step of the runbook that applies to the tag's channel, in runbook order, with no manual trigger.
- **REQ-ECO.REL.PROC.FLOW-002** When the gate verdict is `pass`, the workflow shall write the pre-sign manifest digest into `release-state.json`, set state `awaiting_approval`, and run no signing or publication job until the approval recorded by `ECO.REL.SIGN.CUSTODY` names that exact digest.
- **REQ-ECO.REL.PROC.FLOW-003** If the gate verdict is `block`, then the workflow shall set state `blocked`, attach `release-gates.json` to the workflow run, and request no approval.
- **REQ-ECO.REL.PROC.FLOW-004** When the workflow is re-run for the same tag, it shall resume after the last completed step and shall not upload an artefact to a venue that already holds a file of the same name and SHA-256.
- **REQ-ECO.REL.PROC.FLOW-005** If a venue already holds a file of the same name with a different SHA-256, then the workflow shall stop with state `failed` and name the file and venue.
- **REQ-ECO.REL.PROC.FLOW-006** The workflow shall publish only to venues that `release-gates.json` leaves enabled.
- **REQ-ECO.REL.PROC.FLOW-007** Jobs before the approval shall run with no access to release signing or OS signing secrets.

## Acceptance criteria

- **AC-1** Given the dry-run harness `cargo xtask rel flow-sim --tag v1.4.0-beta.1 --fixtures fixtures/rel/flow/pass/`, when it runs the workflow's step graph with stub jobs, then the final state is `awaiting_approval` and the state file holds the pre-sign digest.
- **AC-2** Given the same run plus fixture approval `approval-ok.json` naming that digest, when the simulator resumes, then the state reaches `feeds_updated` and every enabled venue stub holds the expected files.
- **AC-3** (error path) Given `fixtures/rel/flow/block/` (gate verdict block), then the final state is `blocked` and no approval request is emitted.
- **AC-4** (error path) Given an approval naming a different digest, then signing jobs do not start and the state stays `awaiting_approval` with reason `approval_digest_mismatch`.
- **AC-5** Given a run interrupted after publishing to one of two venues, when re-run, then the first venue receives no upload and the second receives all files.
- **AC-6** (error path) Given a venue stub holding `fid-1.4.0-x86_64.msi` with a different SHA-256, then the run stops `failed` naming file and venue.
- **AC-7** Given the workflow file, when the secrets-scope test parses it, then no job before `sign-approval` references a signing secret or the signing environment.

## Design

- **Registry command(s):** none. CI workflow plus a simulator `cargo xtask rel flow-sim` that executes the step graph with stub jobs for tests. Publication sends data off the machine (X1 in spirit; it runs in CI, not through the product bus).
- **Data structures:** `release-state.json { tag, channel, state, steps: [{id, status, outputs_sha256}], presign_digest, approval_ref, reasons }`. Pre-sign digest = SHA-256 over the canonical JSON list of (name, sha256) of every unsigned artefact plus `release-gates.json` and SBOMs.
- **Algorithm:** derive the job graph from `release/runbook.toml`; each job checks the state file first and skips completed steps; uploads compare remote (name, sha256) before sending.
- **Errors:** `blocked`, `failed(step, reason)`, `approval_digest_mismatch`, `venue_conflict(file, venue)`.
- **Determinism:** N (depends on CI scheduling and remote venues); the simulator is D0.
- **Performance budget:** pre-approval part under 2 h for a full Tier 1 release on hosted runners [unverified].
- **Crate:** workflow files under `.github/workflows/` or the CI system `ECO.CI.WF` selects; simulator in `fid-xtask` (module `rel::flow`).

## Test fixtures

`fixtures/rel/flow/` (pass, block, approval-ok, approval-wrong, venue-conflict) with stub job outputs and in-memory venue stubs; CC-BY-4.0.

## Build prompt

> Implement the release workflow and its simulator. Workflow: triggered by tags matching the release tag format; one job per `ci` step in `release/runbook.toml` that applies to the tag's channel, in order; it calls reusable workflows from the shared CI (`ECO.CI.WF`) and the xtask subcommands of the release nodes (pkg plan and builds, web bundle, SBOM, gates, notes, manifest, feeds, sums, sign, rebuild compare). Keep `release-state.json` (tag, channel, state tagged|built|gated|awaiting_approval|approved|signed|published|feeds_updated|blocked|failed, per-step status and output digests, pre-sign digest, approval reference). On gate `pass`, compute the pre-sign digest (SHA-256 over canonical JSON of the unsigned artefact names and digests, the SBOMs and `release-gates.json`) and stop at `awaiting_approval`; signing jobs run only in the protected signing environment and only when the recorded approval names that digest. On `block`, stop at `blocked`. Re-runs resume and skip uploads whose name and SHA-256 already exist at the venue; a same-name different-digest file stops the run as `failed`. Publish only to venues the gate verdict enables. Nightly: scheduled, nightly key, no approval. Add `cargo xtask rel flow-sim` that executes the same step graph with stub jobs and in-memory venues. Tests: AC-1..AC-7 of spec node ECO.REL.PROC.FLOW, including a parser test that no pre-approval job references signing secrets. Constraints: fork PRs never trigger this workflow; no agent credentials can approve (approval belongs to ECO.REL.SIGN.CUSTODY).

## Open questions

- The CI host and its protected-environment feature are chosen in `ECO.CI.WF`; the design assumes a host offering environment-scoped secrets with required human reviewers [unverified for the final host].
- SDK crate publication to a package registry is a separate step that needs a registry account; it is left out until `ECO.SDK.API` names the registry (maintainer call).
- Until this leaf is built, `release/release-workflow.yml` is a job-ID skeleton (never run) that `cargo xtask rel runbook-check` (ECO.REL.PROC.RUNBOOK) uses as its default `--workflow`. When the real workflow lands, delete the skeleton and change the `--workflow` default (`DEFAULT_WORKFLOW` in `crates/fid-xtask/src/rel/runbook.rs`, and the RUNBOOK preamble) to the real workflow file.

## Children

_Leaf: none._
