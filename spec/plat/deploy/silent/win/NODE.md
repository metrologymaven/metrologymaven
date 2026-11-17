---
id: PLAT.DEPLOY.SILENT.WIN
title: Silent-install contract file and Windows MSI install, upgrade and uninstall test
level: 4
parent: PLAT.DEPLOY.SILENT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY.SILENT-001, REQ-PLAT.DEPLOY.SILENT-002, REQ-PLAT.DEPLOY.SILENT-003, REQ-PLAT.DEPLOY.SILENT-004, REQ-PLAT.DEPLOY.SILENT-005, REQ-PLAT.DEPLOY.SILENT-007]
covers: []
depends_on: []
uses_artifacts: [ECO.REL.PKG.WIN, PLAT.DEPLOY.MCONF, PLAT.MATRIX.CI, PLAT.MATRIX.FILE]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.DEPLOY.SILENT.WIN: MSI contract and test

## Purpose

Publishes the silent-install contract that ECO's Windows MSI must meet (public properties, exit codes, logging) as a machine-readable file, and proves it on every release on each Tier 1 Windows version: silent machine-wide install, upgrade from the previous release, and uninstall that leaves user data and the managed policy file untouched.

## Scope

**In:**
- `platform/silent-contract.toml`: MSI public properties `INSTALLDIR`, `FID_APPS` (comma-free list syntax: space-separated app IDs), `FID_SHORTCUTS` (`0`/`1`); exit codes; log rule; a `[unix]` table read by `PLAT.DEPLOY.SILENT.UNIX`
- `docs/deploy/silent-install.md` Windows section generated from the contract
- CI job `<winver>-silent-msi` per Tier 1 Windows row: install, verify, upgrade, verify, uninstall, verify

**Out** (and which node owns it instead):
- Building the MSI (`ECO.REL.PKG.WIN`)
- Linux and macOS (`PLAT.DEPLOY.SILENT.UNIX`)
- Per-user installs (`PLAT.DEPLOY.NOADMIN`)

## Requirements

- **REQ-PLAT.DEPLOY.SILENT.WIN-001** The contract shall state exit codes 0 (success), 3010 (success, restart required), 1603 (fatal error) and 1618 (another install in progress) [unverified: Windows Installer code meanings], and the test shall fail on any other code for the success path.
- **REQ-PLAT.DEPLOY.SILENT.WIN-002** When the MSI is run with the quiet switch and the log switch to a given path, the test shall check that no window was created by the installer process tree and that the log file exists, is non-empty and contains the product version.
- **REQ-PLAT.DEPLOY.SILENT.WIN-003** When run with `FID_APPS` naming a subset and `FID_SHORTCUTS=0`, the test shall check that only the named apps plus `fid` are installed under `INSTALLDIR` and that no shortcut was created.
- **REQ-PLAT.DEPLOY.SILENT.WIN-004** When the previous release's MSI is installed, a fixture project and user settings are created, and the current MSI is then installed silently, the test shall check exit code 0 or 3010, the new version, and byte-identical project and settings files.
- **REQ-PLAT.DEPLOY.SILENT.WIN-005** After silent uninstall, the test shall check that `INSTALLDIR` and the MSI's registry keys are gone and that user projects, user settings and `%ProgramData%\Fiducial\policy.toml` are byte-identical to before.
- **REQ-PLAT.DEPLOY.SILENT.WIN-006** The job shall run on every effective Tier 1 Windows row of the support matrix and be release-blocking.

## Acceptance criteria

- **AC-1** Given the release-candidate MSI and the previous release's MSI, when the job runs on each Tier 1 Windows runner, then all three phases pass.
- **AC-2** (error path) Given fixture MSI `fixtures/plat/deploy/msi-shows-dialog` that opens a dialog, then the job fails with `UiShown`.
- **AC-3** (error path) Given a fixture MSI whose uninstall deletes the policy file, then `PolicyFileRemoved`.
- **AC-4** Given `platform/silent-contract.toml`, when `xtask plat-deploy silent-contract --check` runs, then it validates and the generated doc matches the committed one.

## Design

- **Registry command(s):** none; CI tooling (`xtask plat-deploy silent-msi`, `xtask plat-deploy silent-contract --check`).
- **Data structures:** `SilentContract { schema_version, msi: MsiContract { properties, exit_codes, log_switch }, unix: UnixContract }`.
- **Algorithm:** snapshot user data and policy file digests → `msiexec /i <msi> /qn /l*v <log> <props>` [unverified exact switches] → watch window creation → verify → repeat for upgrade and `/x` uninstall.
- **Errors:** `ExitCode`, `UiShown`, `LogMissing`, `WrongApps`, `ShortcutCreated`, `DataChanged`, `PolicyFileRemoved`, `Leftovers`.
- **Determinism:** N.
- **Performance budget:** under 15 minutes per runner [unverified].
- **Crate:** `xtask` (module `plat_deploy_silent`).

## Test fixtures

Fixture MSIs built in CI from tiny test sources (one shows a dialog, one deletes the policy file); a fixture project and settings file. Licensed with the code.

## Build prompt

> Write `platform/silent-contract.toml` (schema 1): MSI properties INSTALLDIR, FID_APPS (space-separated app IDs), FID_SHORTCUTS (0/1); exit codes 0, 3010, 1603, 1618; log switch; and a `[unix]` table with per-OS unattended commands and the same exit/log rule. Add `xtask plat-deploy silent-contract --check` (validate and regenerate the Windows section of `docs/deploy/silent-install.md`) and `xtask plat-deploy silent-msi --msi <new> --previous <old>` that runs quiet install with logging, subset/shortcut checks, upgrade over the previous release with fixture project and settings unchanged, and uninstall leaving user data and `%ProgramData%\Fiducial\policy.toml` intact, failing on any window created by the installer tree. Emit job entries per effective Tier 1 Windows row (release-blocking). Tests: AC-1..AC-4. Constraints: CONSTITUTION §8; tooling reads artifacts only. Commit: `feat(xtask): silent MSI contract and test`.

## Open questions

- `FID_APPS` list syntax and exact msiexec switches need checking against ECO.REL.PKG.WIN [unverified].

## Children

_Leaf: none._
