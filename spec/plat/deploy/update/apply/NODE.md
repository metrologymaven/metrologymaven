---
id: PLAT.DEPLOY.UPDATE.APPLY
title: Verify-bundle and apply-bundle commands with atomic install
level: 4
parent: PLAT.DEPLOY.UPDATE
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY.UPDATE-001, REQ-PLAT.DEPLOY.UPDATE-002, REQ-PLAT.DEPLOY.UPDATE-003, REQ-PLAT.DEPLOY.UPDATE-006]
covers: []
depends_on: [PLAT.DEPLOY.UPDATE.VERIFY, CORE.BUS, CORE.BUS.POL.FS]
uses_artifacts: [PLAT.DEPLOY.SILENT.WIN, PLAT.DEPLOY.SILENT.UNIX]
side_effect: [R0, A0]
determinism: N
cli: [fid update verify-bundle, fid update apply-bundle]
mcp: [update_verify_bundle]
journal: [update.verify_bundle@1, update.apply_bundle@1]
updated: 2026-11-17
---

# PLAT.DEPLOY.UPDATE.APPLY: Offline update commands

## Purpose

Exposes offline updating as two registry commands: a read-only `update.verify_bundle@1` that anyone (including an agent, through an approved import root) can run to see whether a bundle is genuine and applicable, and an admin-only `update.apply_bundle@1` that verifies again, copies the verified files to a staging area, re-checks them, and installs so that an interruption leaves the previous installation runnable.

## Scope

**In:**
- Both registry commands, their parameters, results and errors
- Staging: copy verified files into a fresh staging directory and re-hash them there (no time-of-check/time-of-use gap with removable media)
- Install step per installation kind: Windows MSI upgrade with the silent contract (relying on Windows Installer rollback [unverified]); per-user and Linux/macOS side-by-side versioned directories with an atomic pointer switch
- Downgrade option `--allow-downgrade` on apply only

**Out** (and which node owns it instead):
- Verification logic (`PLAT.DEPLOY.UPDATE.VERIFY`, `PLAT.DEPLOY.UPDATE.KEYROT`)
- Online update checks (`ECO.REL.UPD`)
- Installer switch contract (`PLAT.DEPLOY.SILENT`)

## Requirements

- **REQ-PLAT.DEPLOY.UPDATE.APPLY-001** `update.verify_bundle@1` shall be class R0, take a bundle handle in an approved import root (never a raw path for agent or script principals), and return the `VerifiedBundle` summary or the `Refusal` without changing any state.
- **REQ-PLAT.DEPLOY.UPDATE.APPLY-002** `update.apply_bundle@1` shall be class A0 with `human_only: true`, never projected to MCP, and shall journal the manifest digest, from-version, to-version, downgrade flag and outcome in the per-machine settings journal.
- **REQ-PLAT.DEPLOY.UPDATE.APPLY-003** When applying, the command shall copy every listed file into a new staging directory, re-verify each staged file's SHA-256 against the verified manifest, and refuse with `StagingMismatch` without installing if any differs.
- **REQ-PLAT.DEPLOY.UPDATE.APPLY-004** If verification refuses the bundle, then apply shall change nothing on disk outside its staging directory, remove that directory, and report the refusal check.
- **REQ-PLAT.DEPLOY.UPDATE.APPLY-005** For side-by-side installs, the command shall install into a new versioned directory and switch the active-version pointer by a single atomic rename only after the new version's install manifest verifies, so that a kill at any point leaves the previous version active and runnable.
- **REQ-PLAT.DEPLOY.UPDATE.APPLY-006** `--allow-downgrade` shall exist only on `apply-bundle` and shall be recorded in the journal entry.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/deploy/update/ok/` copied into a test import root and installed 1.2.0, when an MCP agent calls `update_verify_bundle` with the handle, then it returns `verified` for 1.3.0 and no file changes.
- **AC-2** Given the same bundle, when a human-upgraded CLI session runs `fid update apply-bundle`, then the side-by-side test install switches to 1.3.0, its install manifest verifies, and one A0 entry is journaled with the manifest digest.
- **AC-3** (error path) Given `tampered-file/`, when applied, then it refuses with `FileMismatch`, the active version stays 1.2.0 and no staging directory remains.
- **AC-4** (kill test) Given a fault-injection hook that kills the process after N files are installed, for every N from 0 to the file count, when the test restarts, then the active version is either 1.2.0 or 1.3.0, and the active version's install manifest verifies.
- **AC-5** (error path) Given an MCP agent session, then no `update_apply_bundle` tool exists; a script principal calling apply gets `policy`.
- **AC-6** (error path) Given a staged file altered between copy and re-hash by a test hook, then `StagingMismatch` and nothing installed.

## Design

- **Registry command(s):**
  - `update.verify_bundle@1`: R0, MCP exposed; params `{ bundle: ImportHandle }`; result `{ status: verified|refused, version?, files?, refusal? }`.
  - `update.apply_bundle@1`: A0, projection never, `human_only: true`; params `{ bundle: ImportHandle, allow_downgrade: bool }`; result `{ from, to, outcome: installed|refused|failed }`. Human CLI sessions may pass a path after the D-012 upgrade.
- **Algorithm (apply):** verify → create staging dir under the install's update area → copy and re-hash → per kind: MSI upgrade via silent contract, or install into `versions/<to>/`, verify install manifest, atomic rename of `current` pointer → remove staging → journal.
- **Errors:** the `Refusal` set, `StagingMismatch`, `InstallFailed { code }`, `policy`.
- **Determinism:** N (file system and installer); verification itself D0.
- **Crate:** `fid-update-offline` (module `commands`), registered with the CORE registry.

## Test fixtures

Bundles from `PLAT.DEPLOY.UPDATE.VERIFY`; a fake side-by-side install tree in a temp dir; a kill-injection hook in test builds. Licensed with the code.

## Build prompt

> In crate `fid-update-offline`, add module `commands` registering `update.verify_bundle@1` (R0, MCP exposed, param bundle import handle; never a raw path for agents/scripts) and `update.apply_bundle@1` (A0, never on MCP, `human_only: true`, params bundle handle and `allow_downgrade`). CLI `fid update verify-bundle|apply-bundle` and MCP `update_verify_bundle` are generated from the registry. Apply: verify (PLAT.DEPLOY.UPDATE.VERIFY), copy listed files to a fresh staging dir and re-hash (`StagingMismatch`), then install: Windows machine installs via the MSI upgrade using the silent contract; side-by-side installs into `versions/<to>/`, verify its install manifest, then atomically rename the active pointer. On refusal or failure leave the previous version active and remove staging. Journal manifest digest, versions, downgrade flag and outcome. Tests: AC-1..AC-6 including the kill-at-every-step test. Constraints: CONSTITUTION §3 (A0 never on MCP, human_only), §8, C7 (no network). Commit: `feat(fid-update-offline): verify and apply commands`.

## Open questions

- Updating a running app on Windows (locked files) relies on the MSI's restart handling (exit 3010) [unverified].

## Children

_Leaf: none._
