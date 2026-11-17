---
id: TRUST.PROFILE.GATE.INVAR
title: Registry-invariance CI test across profiles
level: 4
parent: TRUST.PROFILE.GATE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.PROFILE.GATE-001]
covers: []
depends_on: []
uses_artifacts: [CORE.REG.MAN, TRUST.PROFILE.DEFS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.PROFILE.GATE.INVAR: Registry-invariance CI test across profiles

## Purpose

The CI evidence for REQ-TRUST.PROFILE-003: profiles change gating, never the command set or its classes. The job exports the canonical registry manifest once per built-in profile and fails if the digests differ. It is tooling and reads only published artifacts.

## Scope

**In:**
- CI job script and its expected-output check
- Running the manifest export with each built-in profile active

**Out:**
- Manifest export itself (`CORE.REG.MAN`)
- Profile definitions (`TRUST.PROFILE.DEFS`)

## Requirements

- **REQ-TRUST.PROFILE.GATE.INVAR-001** The CI job shall export the registry manifest with each of `lean`, `regulated` and `assistive_only` active and shall fail if the three SHA-256 digests are not equal.
- **REQ-TRUST.PROFILE.GATE.INVAR-002** On failure, the job shall print the first differing command ID and field between the manifests.
- **REQ-TRUST.PROFILE.GATE.INVAR-003** The job shall run on every pull request that touches `fid-profile`, the registry or any command definition.

## Acceptance criteria

- **AC-1** Given the current tree, when `ci/profile/registry-invariance.sh` runs, then it exits 0 and prints three equal digests.
- **AC-2** (error path) Given a test branch where a profile-conditional `#[cfg]` hides one command, then the job exits non-zero naming that command.

## Design

- **Registry command(s):** none (CI tooling).
- **Data structures:** none.
- **Algorithm:** for each profile, start a headless `fid` with that profile selected in a temporary tenant config, run the manifest export (`CORE.REG.MAN`), hash, compare, and diff by command ID on mismatch.
- **Errors:** non-zero exit with message.
- **Determinism:** D0.
- **Performance budget:** < 2 min in CI [unverified].
- **Crate:** none; `ci/profile/` (tooling layer).

## Test fixtures

None beyond the built tree; the AC-2 mutant is a throwaway test branch.

## Build prompt

> Add CI script `ci/profile/registry-invariance.sh` and a workflow step: for each built-in governance profile (`lean`, `regulated`, `assistive_only`), set it active in a temporary tenant config, export the canonical registry manifest with the generated `fid` manifest export command, compute SHA-256, and fail if the three digests differ, printing the first differing command ID and field (diff the canonical JSON by command). Run on PRs that touch `fid-profile`, the registry crate or command definitions. Tooling reads artifacts only; link no product crate. Constraints: CONSTITUTION §3 (profiles never change the class taxonomy), §8 honest status.

## Open questions

- None.

## Children

_Leaf: none._
