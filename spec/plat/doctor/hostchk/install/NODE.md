---
id: PLAT.DOCTOR.HOSTCHK.INSTALL
title: Install-integrity check (--check-install)
level: 4
parent: PLAT.DOCTOR.HOSTCHK
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-PLAT.DOCTOR.HOSTCHK-001, REQ-PLAT.DOCTOR.HOSTCHK-003, REQ-PLAT.DOCTOR.HOSTCHK-004]
covers: []
depends_on: [PLAT.DOCTOR.CMD, PLAT.DOCTOR.REPORT, PLAT.DEPLOY.OFFLINE.BUNDLE]
side_effect: R0
determinism: N
cli: [fid doctor run-check]
mcp: [doctor_run_check]
journal: [doctor.run_check@1]
updated: 2026-11-17
---

# PLAT.DOCTOR.HOSTCHK.INSTALL: Install-integrity check

## Purpose

Lets a user or IT department confirm that an installation is exactly what the release shipped: `fid doctor run-check --check-install` hashes every installed file and compares it with the install manifest written at install time, reporting missing, modified and extra files. It is how an air-gapped site checks a machine after copying, updating or suspected tampering.

## Scope

**In:**
- Parameter `check_install: bool` on `doctor.run_check@1` (default false)
- Locating the install root and its `install-manifest.json`
- Report section `install` (`PLAT.DOCTOR.REPORT` schema bump to 2) with counts and lists

**Out** (and which node owns it instead):
- The manifest format (`PLAT.DEPLOY.OFFLINE.BUNDLE`)
- The host check (`PLAT.DOCTOR.HOSTCHK.HOSTS`)

## Requirements

- **REQ-PLAT.DOCTOR.HOSTCHK.INSTALL-001** When `check_install` is true, the command shall hash every file below the install root and report each manifest file that is missing or whose SHA-256 or size differs, and each file present but not in the manifest, with paths relative to the install root.
- **REQ-PLAT.DOCTOR.HOSTCHK.INSTALL-002** If the install manifest is missing, unreadable or invalid, then the `install` section shall be `unknown` with reason `no_manifest` or `bad_manifest` and the rest of the report shall be produced.
- **REQ-PLAT.DOCTOR.HOSTCHK.INSTALL-003** When `check_install` is false, the command shall not read the manifest or hash files, and the 10-second budget shall apply as before; when true, the install check shall run after the budgeted probes and outside their deadline.
- **REQ-PLAT.DOCTOR.HOSTCHK.INSTALL-004** Any difference shall not change the hardware outcome or exit code of REQ-PLAT.DOCTOR.CMD-004, except that a non-empty missing or modified list shall raise exit code 5 (`install_damaged`) when no higher-precedence code (2, 3) applies.
- **REQ-PLAT.DOCTOR.HOSTCHK.INSTALL-005** The check shall make no network request and write no file.

## Acceptance criteria

- **AC-1** Given a fixture install tree `fixtures/plat/doctor/install-ok/` with its manifest and a `FakeEnv` meeting recommended, when `run-check --check-install --json` runs, then `install` reports 0 missing, 0 modified, 0 extra and the exit code is 0.
- **AC-2** Given `install-modified/` (one byte changed in `bin/fid`), then `modified` lists `bin/fid` and the exit code is 5.
- **AC-3** Given `install-nomanifest/`, then `install` is `unknown(no_manifest)` and the exit code is the hardware outcome's.
- **AC-4** Given AC-1 without `--check-install`, then the report has no `install` section and a file-open monitor records no read of the manifest.
- **AC-5** Given an MCP agent, when `doctor_run_check` runs with `check_install = true`, then it succeeds (R0) and lists relative paths only.

## Design

- **Registry command(s):** extends `doctor.run_check@1` (R0) with param `check_install: bool`; CLI flag `--check-install`. Exit code 5 added to the code table (`install_damaged`).
- **Data structures:** `InstallSection { status: known | unknown(reason), missing: Vec<RelPath>, modified: Vec<RelPath>, extra: Vec<RelPath> }`, lists sorted by byte order.
- **Algorithm:** resolve install root from the running executable; parse the manifest with `fid-platform::install_manifest`; walk without following symlinks; hash; diff.
- **Errors:** none escape; all become `unknown`.
- **Determinism:** N (reads the disk); D0 given a tree.
- **Performance budget:** at disk speed; about 30 s for 1 GiB [unverified].
- **Crate:** `fid-doctor` (module `install_check`).

## Test fixtures

Small install trees with manifests under `fixtures/plat/doctor/`. Licensed with the code.

## Build prompt

> In crate `fid-doctor`, add module `install_check` and a `check_install: bool` parameter (CLI `--check-install`) to `doctor.run_check@1` (R0). When set, after the budgeted probes and outside their deadline, locate the install root, parse `install-manifest.json` with `fid-platform::install_manifest`, hash every file without following symlinks, and report missing, modified and extra relative paths (byte-sorted) in a new `install` section (report schema 2); missing or invalid manifest → `unknown(no_manifest|bad_manifest)`. Exit 5 `install_damaged` when missing or modified is non-empty and no code 2 or 3 applies. No network, no writes. Tests: AC-1..AC-5. Constraints: CONSTITUTION §3 (R0), §8, C7. Commit: `feat(fid-doctor): install integrity check`.

## Open questions

- Exit code 5 extends the table of `PLAT.DOCTOR.CMD` REQ-004; that leaf should list it when built (sibling note).

## Children

_Leaf: none._
