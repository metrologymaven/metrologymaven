---
id: VIZ.CAPTURE.REC.POLICY
title: Recording policy keys and policy evaluation
level: 4
parent: VIZ.CAPTURE.REC
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE.REC-003, REQ-VIZ.CAPTURE.REC-004]
covers: []
depends_on: [CORE.STO.SET]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.CAPTURE.REC.POLICY: Recording policy keys and policy evaluation

## Purpose

Declares the org recording-policy settings and offers one pure evaluation API that every capture medium (screenshots, view track, viewport video, full-window video) asks before it captures anything. Policy is read from CORE settings; changing it is the A0 command `settings.set_admin_value@1`, owned by `CORE.STO.SET`.

## Scope

**In:** declaration of the admin-flagged, machine-scope keys `capture.recording` (`allowed | forbidden | mandated`, default `allowed`), `capture.window_capture` (`allowed | forbidden`, default `allowed`) and `capture.retention_days` (integer 0 to 36,500, 0 meaning "keep", default 0); declaration of `capture.screenshots` (`allowed | forbidden`, default `allowed`, admin, machine) on behalf of `VIZ.CAPTURE.SHOT`, which reads it through this module; `RecordingPolicy::current()` and `RecordingPolicy::check(MediaKind)`; computing `retain_until` for a new recording.
**Out:** the settings store and the A0 command (`CORE.STO.SET`); starting and stopping (`VIZ.CAPTURE.REC.SESSION`); deleting media at end of retention (`TRUST`, see Open questions); the screenshot command (`VIZ.CAPTURE.SHOT`).

## Requirements

- **REQ-VIZ.CAPTURE.REC.POLICY-001** The module shall declare `capture.recording`, `capture.window_capture` and `capture.retention_days` as admin-flagged settings allowed only in machine scope, with the defaults `allowed`, `allowed` and 0.
- **REQ-VIZ.CAPTURE.REC.POLICY-002** `check(kind)` shall return `Forbid` for `ViewTrack`, `ViewportVideo` and `WindowVideo` while `capture.recording` is `forbidden`, `Forbid` for `WindowVideo` while `capture.window_capture` is `forbidden`, `Forbid` for `Screenshot` while `capture.screenshots` is `forbidden`, and `Allow` otherwise.
- **REQ-VIZ.CAPTURE.REC.POLICY-003** `current()` shall report `mandated: true` exactly while `capture.recording` is `mandated`.
- **REQ-VIZ.CAPTURE.REC.POLICY-004** When a recording starts, `retain_until(start_utc)` shall return the start date plus `capture.retention_days` days, or `None` when the value is 0.
- **REQ-VIZ.CAPTURE.REC.POLICY-005** The evaluation API shall be read-only and shall give the same answer for the same settings snapshot.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/capture/policy-matrix.csv` (every combination of the four keys x four media kinds with the expected answer), when `check` runs for each row against a settings snapshot built from the row, then every answer matches.
- **AC-2** Given a fresh install, when `fid settings list-values --prefix capture --json` runs, then the three new keys report their defaults, `admin: true` and allowed scope `machine` only.
- **AC-3** (error path) Given `fid settings set-value capture.recording forbidden`, then it fails with `admin_setting` (owned by CORE.STO.SET) and the policy is unchanged.
- **AC-4** Given `capture.retention_days = 30` and start `2026-11-17T10:00:00Z`, then `retain_until` is `2026-12-17T10:00:00Z`; given 0, then `None`.

## Design

- **Registry command(s):** none; library API. Settings are declared through the CORE.STO.SET declaration API.
- **Data structures:** `enum MediaKind { Screenshot, ViewTrack, ViewportVideo, WindowVideo }`; `enum Decision { Allow, Forbid { key } }`; `Policy { recording: Allowed|Forbidden|Mandated, window_capture, screenshots, retention_days: u32 }`.
- **Algorithm:** resolve the four keys through the settings resolver; total `match` over kind and policy.
- **Errors:** none. Absent: default; unreadable, invalid or wrong-scope machine value: refused (`Forbid{cause: Unknown}`).
- **Determinism:** D0.
- **Performance budget:** `check` < 10 µs from a cached snapshot [unverified until measured].
- **Crate:** `fid-capture` (module `rec::policy`).

## Test fixtures

`fixtures/viz/capture/policy-matrix.csv`, hand-written from this node; CC-BY-4.0.

## Build prompt

> In crate `fid-capture`, add module `rec::policy`. Declare three admin-flagged, machine-scope settings through the CORE settings declaration API: `capture.recording` (`allowed|forbidden|mandated`, default `allowed`), `capture.window_capture` (`allowed|forbidden`, default `allowed`), `capture.retention_days` (0..=36500, default 0 = keep). Read the existing key `capture.screenshots`. Provide `RecordingPolicy::current() -> Policy`, `check(MediaKind) -> Decision` (Forbid view track and both video kinds when recording is forbidden; forbid window video when window capture is forbidden; forbid screenshots when screenshots are forbidden) and `retain_until(start_utc) -> Option<UtcInstant>`. Read-only and pure over a settings snapshot. Tests: AC-1..AC-4 with `fixtures/viz/capture/policy-matrix.csv`. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; policy changes only through the A0 settings command, never through this module; nothing is sent off the machine (CONSTITUTION §3.1).

## Open questions

- Retention may later depend on an export-control classification in project metadata (`CORE.PRJ.META` tags are a candidate owner); this leaf only computes `retain_until`. Disposal of expired media payloads is assigned to TRUST in `CORE.STO.CAS` scope; no TRUST node yet reads `retain_until`, so expired recordings are flagged, not deleted [cross-domain gap].
- Partial build (`fid-capture` module `rec::policy`, viewer layer): the leaf is not marked built. AC-1, AC-3 and AC-4 are met against real settings files through the fid-store resolver and settings commands. Open acceptance criteria:
  - AC-2: the keys list with their defaults, `admin: true` and machine scope only through the real `settings.list_values@1` handler over the workspace settings plus `capture_settings`, but the shipped workspace settings manifest does not collect them: its collector `fid-registry-build` (services) cannot depend on a viewer crate (the same gap leaves `fid-view-cmd` and `fid-overlay` commands out of the registry), and `fid-cli` has no `fid settings` subcommand yet.
- Build notes: the policy fails closed. A key that is not stored resolves to its default, and the default is used. A key is unknown, never the default, when the snapshot lacks it (the resolver returned no entry for it), holds it twice or got it from a scope other than machine or the default, when a machine-scope warning names it or the machine file is unreadable, or when its value is outside the key's values; every medium that depends on an unknown key is refused (`Forbid { key, cause: Unknown }`), an unknown `capture.recording` is not `mandated`, and an unknown retention refuses the recording media and makes `retain_until` an error rather than "keep". Warnings about user or project scope are ignored, since those values are never used for a machine-only key. `capture.screenshots` (`allowed|forbidden`, default `allowed`, admin, machine) is declared in the same `capture_settings()` because `VIZ.CAPTURE.SHOT` is not built; SHOT reuses that declaration.

## Children

_Leaf: none._
