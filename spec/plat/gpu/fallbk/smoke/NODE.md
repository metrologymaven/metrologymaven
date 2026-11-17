---
id: PLAT.GPU.FALLBK.SMOKE
title: No-GPU headless and forced-fallback start tests
level: 4
parent: PLAT.GPU.FALLBK
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.GPU.FALLBK-001, REQ-PLAT.GPU.FALLBK-004]
covers: []
depends_on: []
uses_artifacts: [PLAT.GPU.FALLBK.TIERS, CORE.REG.MAN, PLAT.DEPLOY.OFFLINE.NOEGRESS, VIZ.RENDER.FRAME, ECO.APPS.LIST]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.GPU.FALLBK.SMOKE: No-GPU start tests

## Purpose

Two release tests that keep the "no GPU, still works" promise honest: `fid` and `fid mcp` run every non-view command on a machine with no GPU and no display, and every app started with the backend forced to `software` (and to `reduced`) reaches its main window, reports the forced tier, and completes the reference non-device workflow.

## Scope

**In:**
- Headless test: a Linux container with no GPU device nodes and no display server; `fid` runs the reference workflow; `fid mcp` lists tools and runs one R0 and one R1 command
- Forced-fallback test: each non-planned app started with the backend override `software` and then with a fake adapter list that triggers `reduced`; `fid view get-render-info --json` must report the tier
- Job entries for `PLAT.GPU.CI` to schedule

**Out** (and which node owns it instead):
- Tier definitions (`PLAT.GPU.FALLBK.TIERS`)
- Renderer behaviour (`VIZ`)

## Requirements

- **REQ-PLAT.GPU.FALLBK.SMOKE-001** When the headless test runs, `fid` shall complete every step of `platform/ref-workflow.toml` with its expected exit code in an environment with no GPU device and no display.
- **REQ-PLAT.GPU.FALLBK.SMOKE-002** In the headless test, `fid mcp` shall start, list its tools, and complete one R0 and one R1 tool call over stdio.
- **REQ-PLAT.GPU.FALLBK.SMOKE-003** For each non-planned app, when started with backend override `software`, the app shall reach its main window within 60 seconds and `view.get_render_info@1` shall report the software tier.
- **REQ-PLAT.GPU.FALLBK.SMOKE-004** In the forced-software run, every non-device command listed in the registry manifest for that app shall be present and enabled in the app's command set, and the test shall fail naming any that is disabled.
- **REQ-PLAT.GPU.FALLBK.SMOKE-005** If any app fails to start, the test shall fail with the app name and the last 200 log lines.

## Acceptance criteria

- **AC-1** Given the release-candidate build in the headless container, when the test runs, then all workflow steps and both MCP calls succeed.
- **AC-2** Given each app with `--gpu-backend software` on a Tier 1 runner, then each reaches its window and reports the software tier.
- **AC-3** (error path) Given a fixture build in which one command is gated on a hardware tier, then the test fails naming that command.
- **AC-4** (error path) Given a fixture that requires `/dev/dri`, then the headless test fails at the first step that touches it.

## Design

- **Registry command(s):** none; CI tooling (`xtask plat-gpu smoke --mode headless|forced`). Reads `view.get_render_info@1` (R0) and the registry manifest.
- **Algorithm:** headless: container without device nodes → run workflow → spawn `fid mcp` → JSON-RPC list and calls. Forced: start app with override → wait for window → query render info → query command availability through the control channel's command listing → close.
- **Errors:** `StepFailed`, `McpFailed`, `AppStart`, `WrongTier`, `CommandDisabled`.
- **Determinism:** N.
- **Crate:** `xtask` (module `plat_gpu_smoke`).

## Test fixtures

Fixture builds with injected faults (built from test sources); the reference workflow from `PLAT.DEPLOY.OFFLINE.NOEGRESS`. Licensed with the code.

## Build prompt

> Add `xtask plat-gpu smoke --mode headless|forced`. Headless: in a Linux container with no GPU device nodes and no display, run every step of `platform/ref-workflow.toml` with `fid`, then start `fid mcp` over stdio, list tools and call one R0 and one R1 tool. Forced: for each non-planned app in the app list, start with backend override `software` (and with a fake adapter list that yields `reduced`), wait ≤ 60 s for the main window, check `fid view get-render-info --json` reports the forced tier, and check every non-device command from the registry manifest is enabled; on failure report the app and last 200 log lines. Emit job entries for PLAT.GPU.CI. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8, C2; tooling reads artifacts only. Commit: `feat(xtask): GPU fallback smoke tests`.

## Open questions

- How the test reads an app's enabled command set depends on the control channel (AUTO); the test uses its R0 listing when available [unverified].

## Children

_Leaf: none._
