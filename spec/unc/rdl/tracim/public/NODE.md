---
id: UNC.RDL.TRACIM.PUBLIC
title: Nightly public-test job
level: 4
parent: UNC.RDL.TRACIM
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [tools/rdl]
built_pr: 503
satisfies: [REQ-UNC.RDL.TRACIM-001]
covers: []
depends_on: []
uses_artifacts: [UNC.RDL.TRACIM.RUN, KRN.MAN.MANI]
side_effect: X1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.TRACIM.PUBLIC: Nightly public-test job

## Purpose

A CI job (tooling) that runs the free TraCIM public test every night against the release-candidate `fid` binary, using `fid rdl submit-tracim --test-kind public` with the key from a CI secret. It publishes the run record (verdict, kernel semver and manifest digest) as a build artifact for `KRN.MAN` and `OUTR`. It never orders paid tests.

## Scope

**In:**
- CI job definition: schedule, the binary under test, key from a CI secret, the configured `tracim` destination
- Published artifact `tracim-public-latest.json` (run record, no key, no raw test data)
- Failure handling: a failed verdict fails the job and opens a tracking issue through the CI's standard mechanism

**Out:**
- The command (owned by `UNC.RDL.TRACIM.RUN`; used as a built binary artifact)
- Paid tests (human-run)

## Requirements

- **REQ-UNC.RDL.TRACIM.PUBLIC-001** The job shall run the public test only (`--test-kind public`) and shall fail its configuration check if the paid-order key secret is reachable from it.
- **REQ-UNC.RDL.TRACIM.PUBLIC-002** The job shall publish the run record with the kernel semver and manifest digest, and shall contain no key or raw test data.
- **REQ-UNC.RDL.TRACIM.PUBLIC-003** If the verdict is not `passed` or the service is unreachable, then the job shall fail and the artifact shall state the reason.

## Acceptance criteria

- **AC-1** Given the job run against a loopback mock service in CI (`fixtures/rdl/tracim/mock/`), then the artifact holds verdict, semver and manifest digest, and a scan for the sentinel key finds nothing.
- **AC-2** Given a mock returning a failed verdict, then the job fails and the artifact reason is `verdict_failed`.
- **AC-3** Given a job configuration that exposes the paid-key secret, then the configuration check fails.

## Design

- **Registry command(s):** none (CI job calling the built `fid` binary).
- **Data structures:** artifact schema `tracim-public/v1`.
- **Algorithm:** build, run the command, extract and publish the record.
- **Errors:** `verdict_failed`, `service_unreachable`, `paid_key_exposed`.
- **Determinism:** N.
- **Performance budget:** under 10 minutes per night [unverified].
- **Crate:** none (CI workflow and a small script under `tools/rdl/`).

## Test fixtures

Reuses the mock service of `UNC.RDL.TRACIM.RUN` (CC-BY-4.0).

## Build prompt

> Add a nightly CI workflow that builds the release-candidate `fid` binary and runs `fid rdl submit-tracim --test-kind public --key-ci-secret --json` against the configured `tracim` destination, with the public-test key from a CI secret. Publish artifact `tracim-public-latest.json` (`tracim-public/v1`: verdict, kernel semver, manifest digest, reason); no key, no raw test data. Fail on verdict other than `passed` or on an unreachable service. Add a configuration check that fails if the paid-order key secret is reachable from the job. Test with the loopback mock (AC-1..AC-3). Tooling: consume the binary as an artifact, no crate dependencies.

## Open questions

- Whether PTB permits automated nightly public tests is [unverified].

## Children

_Leaf: none._
