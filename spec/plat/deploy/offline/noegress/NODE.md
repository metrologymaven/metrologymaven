---
id: PLAT.DEPLOY.OFFLINE.NOEGRESS
title: Reference workflow and sandboxed no-egress CI job
level: 4
parent: PLAT.DEPLOY.OFFLINE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY.OFFLINE-003, REQ-PLAT.DEPLOY.OFFLINE-004]
covers: []
depends_on: []
uses_artifacts: [PLAT.DEPLOY.OFFLINE.BUNDLE, ECO.DATA.EX, PLAT.MATRIX.CI, ECO.APPS.LIST]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.DEPLOY.OFFLINE.NOEGRESS: No-egress CI job

## Purpose

Proves on every release that an air-gapped machine works: on each Tier 1 platform the offline bundle is installed inside a network sandbox that records every connection attempt, every app is started, and the reference non-device workflow is run through `fid`. One DNS lookup or socket attempt fails the release. It also owns the reference workflow definition other deployment tests reuse.

## Scope

**In:**
- `platform/ref-workflow.toml`: an ordered list of `fid` CLI invocations on the P1a example project (open, import, align, evaluate, report export to a local root, save), with expected exit codes
- Per-OS sandbox: Linux network namespace with no interfaces and a logging resolver stub; Windows firewall rules blocking all outbound traffic with audit logging [unverified]; macOS sandbox profile denying network with logging [unverified]
- App start check: each app in the app list starts, reaches its main window, and exits cleanly
- Job entries `<os>-<arch>-noegress` (Tier 1, release-blocking)

**Out** (and which node owns it instead):
- Bundle contents (`PLAT.DEPLOY.OFFLINE.BUNDLE`)
- In-process socket guard for commands (`CORE.BUS.POL.EGRESS`)
- The example project itself (`ECO.DATA.EX`)

## Requirements

- **REQ-PLAT.DEPLOY.OFFLINE.NOEGRESS-001** The reference workflow file shall list at least: open the example project, run one import, one alignment, one evaluation, export one report to an approved local export root, and save, each as a `fid` CLI invocation with its expected exit code.
- **REQ-PLAT.DEPLOY.OFFLINE.NOEGRESS-002** When the job runs, it shall install the offline bundle, start and cleanly close every app in the app list whose status is not `planned`, and run the reference workflow, all inside the sandbox with every network interface except loopback unavailable.
- **REQ-PLAT.DEPLOY.OFFLINE.NOEGRESS-003** If the sandbox log records any outbound connection attempt or DNS query from the installer, an app, `fid` or their child processes, then the job shall fail listing each attempt with process name and destination.
- **REQ-PLAT.DEPLOY.OFFLINE.NOEGRESS-004** If any workflow step exits with a code other than expected, or an app fails to reach its main window within 60 seconds, then the job shall fail naming the step or app.
- **REQ-PLAT.DEPLOY.OFFLINE.NOEGRESS-005** The job shall first run a self-test that deliberately attempts one connection from a probe process and fail with `SandboxBlind` if that attempt is not logged, so a broken sandbox cannot pass silently.

## Acceptance criteria

- **AC-1** Given the release-candidate bundle for each Tier 1 platform, when the job runs, then the self-test attempt is logged, no other attempt is logged, every step returns its expected code, and the job passes.
- **AC-2** (error path) Given fixture binary `fixtures/plat/deploy/phone-home` inserted as a workflow step, when the job runs on Linux, then it fails listing a DNS query from `phone-home`.
- **AC-3** (error path) Given a sandbox configured without logging, then the self-test fails with `SandboxBlind` and no further step runs.
- **AC-4** Given `platform/ref-workflow.toml`, when parsed by `xtask plat-deploy ref-workflow --check`, then it lists the required step kinds and exits 0.

## Design

- **Registry command(s):** none; CI tooling (`xtask plat-deploy noegress --bundle <dir>`, `xtask plat-deploy ref-workflow --check`).
- **Data structures:** `RefWorkflow { schema_version, project: example ID, steps: Vec<Step { kind, argv: Vec<String>, expect_exit: i32 }> }`; `EgressAttempt { process, dest, kind: dns | tcp | udp }`.
- **Algorithm:** set up the sandbox → self-test → install → for each app start, wait for window, close → run steps → collect the sandbox log → filter to the process tree → report.
- **Errors:** `SandboxBlind`, `InstallFailed`, `AppStart { app }`, `StepFailed { step }`, `Egress { attempts }`.
- **Determinism:** N.
- **Performance budget:** under 20 minutes per platform [unverified].
- **Crate:** `xtask` (module `plat_deploy_noegress`); per-OS sandbox scripts in `ci/noegress/`.

## Test fixtures

`fixtures/plat/deploy/phone-home` (test binary built from source in CI that resolves a hostname), the P1a example project from ECO.DATA.EX. Licensed with the code.

## Build prompt

> Write `platform/ref-workflow.toml` (schema 1): steps as `fid` CLI argv lists with expected exit codes covering open project, import, align, evaluate, export a report to an approved local export root, save, on the P1a example project. Add `xtask plat-deploy ref-workflow --check` and `xtask plat-deploy noegress --bundle <dir>`: create a network sandbox per OS (Linux: network namespace, loopback only, logging resolver stub; Windows: outbound-deny firewall with audit logging; macOS: network-deny sandbox profile with logging), run a self-test connection that must be logged (`SandboxBlind` otherwise), install the offline bundle, start and close every non-planned app from the app list (60 s window timeout), run the workflow, then fail on any logged egress attempt from the process tree. Emit job entries `<os>-<arch>-noegress` (tier1, release-blocking). Tests: AC-1..AC-4. Constraints: CONSTITUTION §8, C7; tooling reads artifacts only. Commit: `feat(xtask): no-egress CI job`.

## Open questions

- Windows and macOS logging mechanisms need checking on hosted runners [unverified].

## Children

_Leaf: none._
