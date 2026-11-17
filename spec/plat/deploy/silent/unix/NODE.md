---
id: PLAT.DEPLOY.SILENT.UNIX
title: Unattended Linux and macOS install contract, guide and test
level: 4
parent: PLAT.DEPLOY.SILENT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY.SILENT-002, REQ-PLAT.DEPLOY.SILENT-005, REQ-PLAT.DEPLOY.SILENT-006]
covers: []
depends_on: []
uses_artifacts: [PLAT.DEPLOY.SILENT.WIN, ECO.REL.PKG.LINUX, ECO.REL.PKG.MAC, ECO.REL.PKG.PLAN, PLAT.MATRIX.CI]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.DEPLOY.SILENT.UNIX: Unattended Linux and macOS installs

## Purpose

Gives fleet administrators on Linux and macOS the same promise as the Windows MSI: one documented unattended command per Tier 1 platform and package format, with the shared exit-code and log rule, and a CI test that installs, uninstalls and checks nothing user-owned was touched.

## Scope

**In:**
- The `[unix]` table of `platform/silent-contract.toml` (owned file of `PLAT.DEPLOY.SILENT.WIN`; this node fills and tests the table's rows)
- Per package format in the package plan for Linux and macOS: install command, uninstall command, log destination
- Linux and macOS sections of `docs/deploy/silent-install.md`
- CI job `<os>-<arch>-silent-unattended`

**Out** (and which node owns it instead):
- Package building (`ECO.REL.PKG.LINUX`, `ECO.REL.PKG.MAC`)
- Windows (`PLAT.DEPLOY.SILENT.WIN`)

## Requirements

- **REQ-PLAT.DEPLOY.SILENT.UNIX-001** The contract shall give, for every Linux and macOS package format in the package plan for an effective Tier 1 row, an install command and an uninstall command that need no terminal input, and the check shall fail naming any format without both.
- **REQ-PLAT.DEPLOY.SILENT.UNIX-002** Each command shall exit 0 on success and non-zero on failure and shall write a log to a caller-given path through a wrapper script `fid-unattended.sh` shipped in the package plan's admin tools.
- **REQ-PLAT.DEPLOY.SILENT.UNIX-003** When the job runs on each Tier 1 Linux and macOS runner with stdin closed, install and uninstall shall complete within 10 minutes each, and the job shall fail if either prompts or blocks.
- **REQ-PLAT.DEPLOY.SILENT.UNIX-004** After uninstall, the job shall check that program files are gone and that user projects, user settings and the machine policy file at its MCONF path are byte-identical to before.

## Acceptance criteria

- **AC-1** Given the release-candidate packages, when the job runs on Tier 1 Ubuntu and macOS runners, then install, uninstall and the leftover check pass and logs exist.
- **AC-2** (error path) Given a contract row missing an uninstall command, then `silent-contract --check` exits 1 with `MissingCommand { format, kind: uninstall }`.
- **AC-3** (error path) Given a fixture package whose post-install script reads from stdin, then the job fails with `Blocked` within the timeout.
- **AC-4** (error path) Given a fixture package that deletes `/etc/fiducial/policy.toml` on removal, then `PolicyFileRemoved`.

## Design

- **Registry command(s):** none; CI tooling (`xtask plat-deploy silent-unix`).
- **Data structures:** `UnixContract { rows: Vec<{ os, format, install: Vec<String>, uninstall: Vec<String> }> }`.
- **Algorithm:** snapshot digests of user data and policy file → run wrapper with stdin closed and timeout → verify → uninstall → verify.
- **Errors:** `MissingCommand`, `Blocked`, `ExitCode`, `LogMissing`, `DataChanged`, `PolicyFileRemoved`, `Leftovers`.
- **Determinism:** N.
- **Crate:** `xtask` (module `plat_deploy_silent_unix`); wrapper script `packaging/fid-unattended.sh`.

## Test fixtures

Fixture packages built in CI from test sources (one blocking on stdin, one deleting the policy file). Licensed with the code.

## Build prompt

> Fill the `[unix]` table of `platform/silent-contract.toml` with install and uninstall argv per Linux and macOS package format listed in the package plan for Tier 1 rows, write `packaging/fid-unattended.sh` (runs the command, logs to a given path, exit 0/non-zero), extend `xtask plat-deploy silent-contract --check` to require both commands per format, and add `xtask plat-deploy silent-unix --package <path>` that snapshots user projects, settings and `/etc/fiducial/policy.toml` or the macOS policy path, installs and uninstalls with stdin closed and a 10-minute timeout each, and checks leftovers. Generate the Linux and macOS sections of `docs/deploy/silent-install.md`. Emit job entries per effective Tier 1 Linux and macOS row (release-blocking). Tests: AC-1..AC-4. Constraints: CONSTITUTION §8. Commit: `feat(xtask): unattended unix install test`.

## Open questions

- Package formats are ECO.REL's choice; this node tests what the plan lists.

## Children

_Leaf: none._
