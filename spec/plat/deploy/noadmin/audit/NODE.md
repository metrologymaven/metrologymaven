---
id: PLAT.DEPLOY.NOADMIN.AUDIT
title: Standard-user install and write-audit CI job
level: 4
parent: PLAT.DEPLOY.NOADMIN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY.NOADMIN-001, REQ-PLAT.DEPLOY.NOADMIN-002, REQ-PLAT.DEPLOY.NOADMIN-003, REQ-PLAT.DEPLOY.NOADMIN-004]
covers: []
depends_on: [PLAT.DEPLOY.NOADMIN.LAYOUT]
uses_artifacts: [ECO.REL.PKG.WIN, ECO.REL.PKG.LINUX, ECO.REL.PKG.MAC, PLAT.DEPLOY.OFFLINE.NOEGRESS, PLAT.MATRIX.CI]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.DEPLOY.NOADMIN.AUDIT: Write-audit CI job

## Purpose

On every release and every Tier 1 platform, installs the per-user package as a standard user, runs the reference non-device workflow, and records every file-system write and OS registration. Any write outside the allowed locations, any elevation prompt and any forbidden registration fails the release.

## Scope

**In:**
- Per-OS audit drivers: Windows (file-system activity tracing [unverified: ETW provider choice]), Linux (`fanotify` or audit subsystem [unverified]), macOS (endpoint-security events or `fs_usage` [unverified])
- Standard-user execution of install, the reference workflow and uninstall
- Before/after snapshots of services, drivers, file associations and scheduled tasks
- The audit report and its comparison with `platform/peruser-layout.toml`

**Out** (and which node owns it instead):
- The allowed-locations file (`PLAT.DEPLOY.NOADMIN.LAYOUT`)
- The reference workflow script (`PLAT.DEPLOY.OFFLINE.NOEGRESS`)
- Runners and workflow YAML (`ECO.CI`)

## Requirements

- **REQ-PLAT.DEPLOY.NOADMIN.AUDIT-001** When a release is built, the job shall run on each effective Tier 1 platform, installing the per-user package as a standard user with no elevation token available, and fail with `ElevationRequested` if the installer requests elevation or exits non-zero.
- **REQ-PLAT.DEPLOY.NOADMIN.AUDIT-002** The job shall record every file created, modified, renamed or deleted by the installer and app processes and their children during install and the reference workflow.
- **REQ-PLAT.DEPLOY.NOADMIN.AUDIT-003** If any recorded write is not allowed by `is_allowed_write` for the expanded layout, then the job shall fail listing each path (with the user name redacted) and the writing process.
- **REQ-PLAT.DEPLOY.NOADMIN.AUDIT-004** If the snapshot difference after install shows a new service, driver, machine-wide file association or scheduled task, then the job shall fail naming it.
- **REQ-PLAT.DEPLOY.NOADMIN.AUDIT-005** The job shall emit an audit report JSON (writes grouped by allowed location, violations, registrations) and attach it to the release evidence, sorted by path.

## Acceptance criteria

- **AC-1** Given the release-candidate per-user packages, when the job runs on each Tier 1 runner, then it passes and the report has zero violations.
- **AC-2** (error path) Given test package `fixtures/plat/deploy/pkg-writes-programdata` (a fixture binary that writes `%ProgramData%\x.txt`), when audited on Windows, then the job fails listing that path.
- **AC-3** (error path) Given test package `fixtures/plat/deploy/pkg-registers-task` that creates a scheduled task, then the job fails with `ForbiddenRegistration { kind: scheduled_task }`.
- **AC-4** (error path) Given a fixture installer that requests elevation, then `ElevationRequested` and the job stops within 60 s.
- **AC-5** (determinism) Given a recorded event log `fixtures/plat/deploy/audit-events-linux.json`, when the comparison runs twice, then the reports are byte-identical.

## Design

- **Registry command(s):** none; CI tooling (`xtask plat-deploy audit --os <os> --package <path>`).
- **Data structures:** `WriteEvent { path, op, pid, process }`; `AuditReport { platform, allowed: BTreeMap<location, count>, violations: Vec<WriteEvent>, registrations: Vec<Registration> }`.
- **Algorithm:** start tracer (privileged helper on the runner, separate from the unprivileged test user) → install as standard user → run the reference workflow script → stop tracer → filter events to the process tree → compare with the expanded layout → snapshot diff of registrations.
- **Errors:** `ElevationRequested`, `InstallFailed`, `TracerUnavailable` (fails the job, never skipped), `Violation`, `ForbiddenRegistration`.
- **Determinism:** N for tracing; D0 for the comparison given an event log.
- **Performance budget:** under 20 minutes per platform [unverified].
- **Crate:** `xtask` (module `plat_deploy_audit`), using `fid-platform::peruser_layout`.

## Test fixtures

Tiny fixture packages that write to forbidden locations or register tasks (built in CI from test sources), recorded event logs under `fixtures/plat/deploy/`. Licensed with the code.

## Build prompt

> Add `xtask plat-deploy audit --os <windows|ubuntu|macos> --package <path> --workflow <script>`. With a privileged tracer helper running beside an unprivileged standard test user, install the per-user package (fail `ElevationRequested` on any elevation request), run the reference non-device workflow script from PLAT.DEPLOY.OFFLINE.NOEGRESS, record all file writes of the installer and app process trees, snapshot services, drivers, machine-wide file associations and scheduled tasks before and after, and compare writes with `fid-platform::peruser_layout::is_allowed_write`. Emit a sorted JSON report, redact the user name in paths, and fail on any violation or forbidden registration; a missing tracer fails the job. Add job entries `<os>-noadmin-audit` (tier1, release-blocking) in the platform job list format. Tests: AC-1..AC-5. Constraints: CONSTITUTION §8; tooling links no product crate other than `fid-platform`. Commit: `feat(xtask): per-user write audit`.

## Open questions

- Tracer mechanisms per OS and whether hosted runners permit them are unverified; self-hosted runners may be needed.

## Children

_Leaf: none._
