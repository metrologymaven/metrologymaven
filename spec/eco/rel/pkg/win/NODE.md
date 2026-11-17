---
id: ECO.REL.PKG.WIN
title: Windows packages
level: 4
parent: ECO.REL.PKG
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.PKG-001, REQ-ECO.REL.PKG-002, REQ-ECO.REL.PKG-003, REQ-ECO.REL.PKG-006]
covers: []
depends_on: [ECO.CI.WF]
uses_artifacts: [ECO.REL.PKG.PLAN, ECO.REL.PKG.REG, PLAT.DEPLOY.SILENT, PLAT.DEPLOY.NOADMIN]
side_effect: W1
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.PKG.WIN: Windows packages

## Purpose

Builds the Windows packages listed in the package plan: one MSI per app, one MSI and one portable ZIP for the `fid` CLI, and one MSI and one portable ZIP for standalone `fid-verify`. MSIs install per user by default with no administrator prompt, honour the property and exit-code contract that `PLAT.DEPLOY.SILENT` publishes for machine-wide silent installs, write the registration entries `ECO.REL.PKG.REG` defines, and make no network request.

## Scope

**In:**
- MSI authoring with a declarative installer toolset [unverified choice: WiX Toolset v4 or later], dual-purpose package defaulting to per-user scope
- Portable ZIP for `fid` and `fid-verify`
- Per-user install location under the user's local application data folder, as `PLAT.DEPLOY.NOADMIN` specifies
- Registry and file-association entries from `ECO.REL.PKG.REG`
- An install, run and uninstall smoke test on a standard-user Windows runner with outbound network blocked

**Out:**
- Authenticode signing (owned by `ECO.REL.SIGN.OSSIGN`)
- The silent-install property contract itself (owned by `PLAT.DEPLOY.SILENT`)
- MSIX or store packages (not planned; maintainer decision)

## Requirements

- **REQ-ECO.REL.PKG.WIN-001** For every Windows entry in the package plan, the build shall produce the file named in the plan for that entry's architecture.
- **REQ-ECO.REL.PKG.WIN-002** When an MSI is run by a standard user with no command-line properties, it shall install into the per-user location without an elevation prompt.
- **REQ-ECO.REL.PKG.WIN-003** When an MSI is run with the machine-wide properties of the `PLAT.DEPLOY.SILENT` contract by an administrator, it shall install machine-wide and return the exit codes that contract names.
- **REQ-ECO.REL.PKG.WIN-004** The MSI shall contain no custom action that opens a network connection, and the smoke test shall observe no outbound connection attempt during install or uninstall.
- **REQ-ECO.REL.PKG.WIN-005** The `fid-verify` MSI and ZIP shall contain only `fid-verify.exe`, licence files and documentation.
- **REQ-ECO.REL.PKG.WIN-006** Uninstall shall remove every file and registry value the MSI installed and leave user projects and settings untouched.
- **REQ-ECO.REL.PKG.WIN-007** Each MSI's product version shall equal the release version without the pre-release suffix, and the full version shall be stored in a product property [unverified MSI version field limits].

## Acceptance criteria

- **AC-1** Given the fixture plan `fixtures/rel/pkgwin/plan.json` and stub binaries, when the Windows build job runs, then every planned file exists with the planned name.
- **AC-2** Given the built `report` MSI on a standard-user runner with outbound traffic blocked by firewall rule, when `msiexec /i <msi> /qn` runs, then it exits 0, no elevation occurs, the files are under the per-user location, the firewall log shows no outbound attempt from the installer processes, and the registration entry exists.
- **AC-3** Given an administrator runner, when installed with the `PLAT.DEPLOY.SILENT` machine-wide properties, then it installs machine-wide and returns the contract's success code.
- **AC-4** Given AC-2's install, when uninstalled, then no installed file or registry value remains and a fixture project in the user's documents folder is unchanged.
- **AC-5** Given the `fid-verify` ZIP, when its file list is read, then it contains only `fid-verify.exe`, licence files and docs.

## Design

- **Registry command(s):** none; CI build job plus `cargo xtask rel pkg-win --plan <file> --bin <dir> --out <dir>` that generates installer sources from templates and calls the toolset (W1).
- **Data structures:** installer source templates under `packaging/windows/`; stable upgrade codes per package recorded in `packaging/windows/upgrade-codes.toml` (never change once published).
- **Algorithm:** template the installer source per plan entry; build; run smoke tests on hosted runners.
- **Errors:** build failure per package reported to `ECO.REL.PKG.PLAN`'s result directory.
- **Determinism:** D1 (same bytes per platform toolchain, before signing) [unverified for MSI; see ECO.REL.REPRO.STRIP].
- **Performance budget:** under 10 min for all Windows packages on a hosted runner [unverified].
- **Crate:** `fid-xtask` (module `rel::pkg_win`); templates under `packaging/windows/`.

## Test fixtures

`fixtures/rel/pkgwin/` with a plan and stub executables built from a hello-world Rust crate; CC-BY-4.0.

## Build prompt

> Build Windows packaging for the release. Add `cargo xtask rel pkg-win --plan package-plan.json --bin <dir> --out <dir>` that, for each Windows plan entry, generates installer sources from templates in `packaging/windows/` and builds: one MSI per app; MSI plus portable ZIP for `fid`; MSI plus portable ZIP for standalone `fid-verify` (only fid-verify.exe, licences, docs). MSIs are dual-purpose and default to per-user install under the local application data folder with no elevation; machine-wide install follows the PLAT.DEPLOY.SILENT property and exit-code contract. No custom action may use the network. Write registration and file-association entries as ECO.REL.PKG.REG defines. Keep stable upgrade codes in `packaging/windows/upgrade-codes.toml`. Add CI smoke tests on a standard-user runner with outbound traffic blocked: install silently, check location and registration, check no outbound attempt, uninstall and check nothing remains and user projects are untouched. Tests: AC-1..AC-5 of spec node ECO.REL.PKG.WIN. Do not sign (ECO.REL.SIGN.OSSIGN does that after approval).

## Open questions

- MSIX would give cleaner per-user installs but restricts some file-system access [unverified]; left out unless PLAT asks.

## Children

_Leaf: none._
