---
id: PLAT.DEPLOY.KIOSK.GUIDE
title: Per-OS kiosk deployment guides and guide lint
level: 4
parent: PLAT.DEPLOY.KIOSK
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY.KIOSK-001, REQ-PLAT.DEPLOY.KIOSK-002]
covers: []
depends_on: [PLAT.MATRIX.FILE]
uses_artifacts: [VIZ.KIOSK.SESSION, PLAT.DEPLOY.MCONF, PLAT.DEPLOY.KIOSK.DISPLAY]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.DEPLOY.KIOSK.GUIDE: Kiosk deployment guides

## Purpose

Gives a site administrator a tested recipe per Tier 1 desktop OS to turn a PC into a measuring station: a dedicated local account that logs in automatically, starts the operator app in kiosk mode, exposes no other application or shell, and comes back to kiosk mode after a restart. A lint keeps every guide complete and in step with the support matrix.

## Scope

**In:**
- `docs/deploy/kiosk/windows.md`, `docs/deploy/kiosk/ubuntu.md`, `docs/deploy/kiosk/macos.md`, one per Tier 1 desktop OS family
- Fixed section set per guide: Account, Auto-login, Single-app mode, Shell lockdown, Power and screen lock, Restart behaviour, Touch and on-screen keyboard, Managed policy, Verification, Limits
- Machine-readable configuration snippets in fenced blocks tagged `kiosk-config-<os>` (consumed by `PLAT.DEPLOY.KIOSK.SMOKE` for Windows)
- `xtask plat-kiosk lint-guides`

**Out** (and which node owns it instead):
- The CI smoke test (`PLAT.DEPLOY.KIOSK.SMOKE`)
- The reference display data (`PLAT.DEPLOY.KIOSK.DISPLAY`)
- Kiosk mode inside the app and `view.enter_kiosk@1` (`VIZ.KIOSK.SESSION`)

## Requirements

- **REQ-PLAT.DEPLOY.KIOSK.GUIDE-001** The project shall publish one kiosk guide per OS family that has an effective Tier 1 desktop row in the support matrix, and the lint shall fail naming any such OS family without a guide.
- **REQ-PLAT.DEPLOY.KIOSK.GUIDE-002** Each guide shall contain the ten fixed sections in order, and the lint shall fail naming the first missing or out-of-order section.
- **REQ-PLAT.DEPLOY.KIOSK.GUIDE-003** The Windows guide shall configure a dedicated standard (non-administrator) local account whose shell is the operator app via the OS's assigned-access or shell-replacement feature [unverified: feature availability per Windows edition], with Task Manager, Win+R and the Start menu unreachable.
- **REQ-PLAT.DEPLOY.KIOSK.GUIDE-004** The Ubuntu guide shall configure a dedicated account with auto-login into a session whose only client is the operator app [unverified: compositor choice], with virtual-terminal switching disabled.
- **REQ-PLAT.DEPLOY.KIOSK.GUIDE-005** The macOS guide shall configure auto-login and the app as a login item in full-screen kiosk mode, and its Limits section shall state that the OS shell cannot be fully hidden from a determined user [unverified].
- **REQ-PLAT.DEPLOY.KIOSK.GUIDE-006** Each guide's Restart behaviour section shall configure the OS so that after a restart or power loss the station logs in and starts the app with kiosk mode resumed, relying on the app's resume behaviour (`VIZ.KIOSK.SESSION`), with no operator input.
- **REQ-PLAT.DEPLOY.KIOSK.GUIDE-007** Each guide shall show how to lock kiosk-relevant settings through the managed policy file of `PLAT.DEPLOY.MCONF` and shall name the reference display from `platform/kiosk-display.toml`.
- **REQ-PLAT.DEPLOY.KIOSK.GUIDE-008** Each `kiosk-config-<os>` block shall parse as TOML with keys `account`, `autologin`, `shell_command` and `restart_policy`, and the lint shall fail naming the guide if parsing fails.

## Acceptance criteria

- **AC-1** Given the shipped guides and `platform/support-matrix.toml` at phase P1a, when `cargo xtask plat-kiosk lint-guides` runs, then it exits 0 and prints `3 guides, 0 errors`.
- **AC-2** (error path) Given `fixtures/plat/kiosk/guides-missing-ubuntu/` and `fixtures/plat/matrix/valid.toml`, when linted, then it exits 1 with `MissingGuide { os: ubuntu }`.
- **AC-3** (error path) Given `fixtures/plat/kiosk/guide-no-restart.md` lacking `## Restart behaviour`, when linted, then it exits 1 with `MissingSection { guide, section: "Restart behaviour" }`.
- **AC-4** (error path) Given `fixtures/plat/kiosk/guide-bad-config.md` whose `kiosk-config-windows` block is not TOML, when linted, then it exits 1 with `BadConfigBlock`.
- **AC-5** (determinism) Given AC-2's inputs, when linted twice, then the diagnostics are byte-identical.

## Design

- **Registry command(s):** none; documentation plus `xtask plat-kiosk lint-guides [--docs <dir>] [--matrix <file>] [--phase <P>]`.
- **Data structures:** `KioskConfig { account: String, autologin: bool, shell_command: String, restart_policy: always | on_failure }`.
- **Algorithm:** read effective Tier 1 desktop OS families through `fid-platform::matrix`; for each, open the guide; scan `##` headings in order against the fixed list; extract fenced blocks by tag and parse with a 16 KiB cap.
- **Errors:** `MissingGuide`, `MissingSection`, `SectionOrder`, `BadConfigBlock`, `TooLarge`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-platform` (module `kiosk_guides`) and the workspace `xtask`.

## Test fixtures

Hand-written Markdown guides under `fixtures/plat/kiosk/`; matrix fixtures from `PLAT.MATRIX.FILE`. Licensed with the code.

## Build prompt

> Write three kiosk guides, `docs/deploy/kiosk/{windows,ubuntu,macos}.md`, each with these `##` sections in order: Account, Auto-login, Single-app mode, Shell lockdown, Power and screen lock, Restart behaviour, Touch and on-screen keyboard, Managed policy, Verification, Limits. Windows: dedicated standard local account whose shell is the operator app via assigned access or shell replacement (state edition limits; mark unchecked claims [unverified]); Ubuntu: dedicated account, auto-login into a session whose only client is the app, VT switching off; macOS: auto-login, login item, full-screen kiosk, and a Limits section stating reduced guarantees. Each guide shows how to lock settings with the managed policy file (`policy.toml`, PLAT.DEPLOY.MCONF), names the reference display from `platform/kiosk-display.toml`, uses `fid view enter-kiosk` (admin) to arm kiosk mode, and has a fenced block tagged `kiosk-config-<os>` with TOML keys `account`, `autologin`, `shell_command`, `restart_policy`. In crate `fid-platform` add module `kiosk_guides` and `xtask plat-kiosk lint-guides` that requires a guide per effective Tier 1 desktop OS family (via `fid-platform::matrix`), checks the section order and parses the config blocks (16 KiB cap). Tests: AC-1..AC-5 with fixtures in `fixtures/plat/kiosk/`. Constraints: CONSTITUTION §8 (no panic, no `unwrap`, bounded input), C9 (own words; no vendor text copied). Commit: `feat(fid-platform): kiosk guides and lint`.

## Open questions

- Which Windows editions support assigned access for a desktop app, and the Ubuntu compositor choice, need checking on real machines [unverified].

## Children

_Leaf: none._
