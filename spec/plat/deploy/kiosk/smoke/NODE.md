---
id: PLAT.DEPLOY.KIOSK.SMOKE
title: Windows kiosk image smoke and restart test
level: 4
parent: PLAT.DEPLOY.KIOSK
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY.KIOSK-002, REQ-PLAT.DEPLOY.KIOSK-004]
covers: []
depends_on: []
uses_artifacts: [PLAT.DEPLOY.KIOSK.GUIDE, VIZ.KIOSK.SESSION, ECO.REL.PKG.WIN, PLAT.MATRIX.CI]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.DEPLOY.KIOSK.SMOKE: Kiosk smoke and restart test

## Purpose

Proves on every release that the Windows kiosk recipe actually works: a VM image is configured from the guide's `kiosk-config-windows` block, the release MSI is installed, the VM boots, and the operator app must report kiosk mode within 120 seconds, then again after a forced restart. It is release tooling that reads published artifacts only.

## Scope

**In:**
- An image-build script that applies `kiosk-config-windows` to a clean Windows 11 VM image [unverified: base image licensing for CI]
- The test driver: install the MSI, arm kiosk mode with a fixture project and program, reboot, poll `fid view get-kiosk-status --json`, force a restart, poll again
- The job entry added to the platform job list as Tier 1 release-blocking

**Out** (and which node owns it instead):
- Guides and config snippets (`PLAT.DEPLOY.KIOSK.GUIDE`)
- Kiosk mode and its status command (`VIZ.KIOSK.SESSION`)
- Runners with nested virtualisation and the workflow YAML (`ECO.CI`)

## Requirements

- **REQ-PLAT.DEPLOY.KIOSK.SMOKE-001** When a release is built, the job shall build a Windows VM image from the `kiosk-config-windows` block of the Windows guide, install the release MSI and arm kiosk mode with fixture project `fixtures/plat/kiosk/station-project/`.
- **REQ-PLAT.DEPLOY.KIOSK.SMOKE-002** After boot, the driver shall poll `fid view get-kiosk-status --json` every 5 seconds and pass only if `active` is true within 120 seconds of the login event [unverified: timing to be measured].
- **REQ-PLAT.DEPLOY.KIOSK.SMOKE-003** After the first pass, the driver shall force a hard restart of the VM and pass only if `active` is true again within 120 seconds with the same `station_id` and program, with no input sent to the VM.
- **REQ-PLAT.DEPLOY.KIOSK.SMOKE-004** The driver shall also check that, during the test, no process other than the app and OS services owns a visible top-level window [unverified: detection method].
- **REQ-PLAT.DEPLOY.KIOSK.SMOKE-005** On failure the job shall attach a screenshot of the VM console and the status outputs to the job log, and shall report `failed` to the release gate.

## Acceptance criteria

- **AC-1** Given the release candidate MSI and the shipped Windows guide, when the job runs on a runner with nested virtualisation, then both polls report `active: true` within 120 s and the job passes.
- **AC-2** (error path) Given a fixture guide whose `shell_command` starts `notepad.exe` instead of the app, when the job runs, then it fails after 120 s with `KioskNotActive` and attaches a screenshot.
- **AC-3** (error path) Given an image where auto-login is disabled, when the job runs, then it fails with `NoLogin` within 180 s.
- **AC-4** Given a passing run, when the gate reads the job result, then the job ID `windows-kiosk-smoke` is listed as passed.

## Design

- **Registry command(s):** none; CI tooling. It invokes `fid view get-kiosk-status` (R0) inside the VM.
- **Data structures:** job entry `{ id: "windows-kiosk-smoke", tier: tier1, kind: kiosk_smoke, runner: <nested-virt label> }` appended to the job list format of `PLAT.MATRIX.CI`.
- **Algorithm:** build image (unattended OS install answer file plus config block) → install MSI silently → run `fid view enter-kiosk` as the admin account with the fixture station → reboot → wait for login event from the guest log → poll status → hard reset → poll again.
- **Errors:** `ImageBuild`, `InstallFailed`, `NoLogin`, `KioskNotActive`, `ForeignWindow`, `StatusUnreadable`.
- **Determinism:** N (VM timing).
- **Performance budget:** whole job under 30 minutes [unverified].
- **Crate:** `xtask` (module `plat_kiosk_smoke`); guest scripts in PowerShell under `ci/kiosk/`.

## Test fixtures

`fixtures/plat/kiosk/station-project/` (a small project with one released fixture program, from the P1a example projects), a broken guide fixture for AC-2. Licensed with the code. The Windows base image is supplied by the runner host, not stored in the repository.

## Build prompt

> Add `xtask plat-kiosk smoke --msi <path> --guide docs/deploy/kiosk/windows.md` and guest scripts in `ci/kiosk/`. Build a Windows VM image from the guide's `kiosk-config-windows` TOML block (account, autologin, shell_command, restart_policy), install the MSI silently, arm kiosk mode with `fid view enter-kiosk` using `fixtures/plat/kiosk/station-project/`, reboot, poll `fid view get-kiosk-status --json` every 5 s until `active` is true (fail `KioskNotActive` after 120 s from login), hard-reset the VM and poll again expecting the same station and program, and check no foreign top-level window exists. Attach a console screenshot and outputs on failure. Emit job `windows-kiosk-smoke` (tier1, release-blocking) in the platform job list format. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8; tooling reads artifacts only (no product crate linked). Commit: `feat(xtask): windows kiosk smoke test`.

## Open questions

- Hosted runners with nested virtualisation for Windows guests may not exist; a self-hosted runner per D-014 hosting may be needed [unverified].
- How `fid view get-kiosk-status` reaches the running app (control channel) is VIZ/AUTO's contract; this test only reads its JSON.

## Children

_Leaf: none._
