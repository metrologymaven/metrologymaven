---
id: PLAT.GPU.FALLBK.RDP
title: Windows remote desktop start test
level: 4
parent: PLAT.GPU.FALLBK
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-PLAT.GPU.FALLBK-001, REQ-PLAT.GPU.FALLBK-006]
covers: []
depends_on: []
uses_artifacts: [VIZ.RENDER.FRAME, ECO.APPS.LIST, ECO.REL.PKG.WIN]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.GPU.FALLBK.RDP: Remote desktop start test

## Purpose

Many shop-floor PCs are used through Windows Remote Desktop. This test opens a real RDP session to a Windows runner, starts each app inside it, and records which backend or fallback tier it chose. Starting is mandatory; the chosen tier is recorded so the policy and docs can state measured behaviour instead of a guess.

## Scope

**In:**
- A two-machine job: an RDP client runner connects to a Windows target runner [unverified: hosted runner support]
- Starting each non-planned app inside the session and reading `view.get_render_info@1`
- `docs/deploy/remote-desktop.md` table of observed backend and tier per Windows version, generated from job results

**Out** (and which node owns it instead):
- Backend selection (`PLAT.GPU.DETECT`)
- Fallback tiers (`PLAT.GPU.FALLBK.TIERS`)

## Requirements

- **REQ-PLAT.GPU.FALLBK.RDP-001** When the job runs, each non-planned app shall start inside the RDP session and reach its main window within 60 seconds.
- **REQ-PLAT.GPU.FALLBK.RDP-002** The job shall record per app the backend, adapter and tier reported by `view.get_render_info@1` and fail if the app reports a hardware tier while its frame-presentation probe reports zero presented frames in 10 seconds.
- **REQ-PLAT.GPU.FALLBK.RDP-003** The job shall regenerate the observed-behaviour table and fail if the committed docs table differs, so a behaviour change is reviewed.

## Acceptance criteria

- **AC-1** Given the release candidate, when the job runs against each Tier 1 Windows version, then every app starts and the table is regenerated.
- **AC-2** (error path) Given a fixture build that exits when the session is remote, then the job fails naming the app.
- **AC-3** Given recorded results `fixtures/plat/gpu/rdp-results.json`, when the table generator runs, then it equals `fixtures/plat/gpu/remote-desktop.md`.

## Design

- **Registry command(s):** none; CI tooling (`xtask plat-gpu rdp`).
- **Algorithm:** client runner opens RDP with a test account → launches each app via a scheduled logon script → polls render info → collects results → generates the table.
- **Errors:** `SessionFailed`, `AppStart`, `NoFrames`, `TableDrift`.
- **Determinism:** N; table generation D0.
- **Crate:** `xtask` (module `plat_gpu_rdp`).

## Test fixtures

Recorded results and expected table under `fixtures/plat/gpu/`. Licensed with the code.

## Build prompt

> Add `xtask plat-gpu rdp`: from a client runner, open an RDP session to a Windows target runner with a test account, start each non-planned app (app list) inside it, require the main window within 60 s, record `view.get_render_info@1` (backend, adapter, tier) and fail with `NoFrames` if a hardware tier presents no frame in 10 s. Generate `docs/deploy/remote-desktop.md` (observed backend and tier per Windows version and app) and fail on drift. Tests: AC-1..AC-3. Constraints: CONSTITUTION §8; tooling reads artifacts only. Commit: `feat(xtask): remote desktop start test`.

## Open questions

- Two-runner RDP jobs may need self-hosted runners (D-014 hosting) [unverified].

## Children

_Leaf: none._
