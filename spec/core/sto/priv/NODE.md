---
id: CORE.STO.PRIV
title: Offline operation and telemetry consent
level: 3
parent: CORE.STO
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.STO-007, REQ-CORE.STO-008]
covers: []
depends_on: [CORE.STO.SET]
side_effect: W3
determinism: D0
cli: [fid settings set-telemetry-consent, fid settings show-privacy]
mcp: [settings_show_privacy]
journal: [settings.set_telemetry_consent@1, settings.show_privacy@1]
updated: 2026-11-16
---

# CORE.STO.PRIV: Offline operation and telemetry consent

## Purpose

Makes CONSTITUTION C7 testable: the core creates, opens, edits and saves projects with no network and no account, and telemetry stays off until a human explicitly consents. Consent is a recorded, revocable act tied to a specific description of what would be collected; if that description changes, consent lapses.

## Scope

**In:** the telemetry consent setting and its command, the privacy status query, the offline test harness, the no-account guarantee.
**Out:** any telemetry collection or upload (owned by `OUTR`, if ever built); the general settings system (`CORE.STO.SET`); network guard of the bus (`CORE.BUS.POL`).

## Requirements

- **REQ-CORE.STO.PRIV-001** The core shall create, open, edit and save projects with no network interface available.
- **REQ-CORE.STO.PRIV-002** No command of the core shall require a user account or sign-in.
- **REQ-CORE.STO.PRIV-003** The telemetry consent setting shall default to off in every scope.
- **REQ-CORE.STO.PRIV-004** `settings.set_telemetry_consent@1` shall be side-effect class W3 and shall commit only for a human principal.
- **REQ-CORE.STO.PRIV-005** When consent is turned on, the system shall record the digest of the telemetry data description in force, and if that description's digest later changes, then the system shall treat consent as off until a human consents again.
- **REQ-CORE.STO.PRIV-006** When consent is turned off, the system shall stop any telemetry before the command returns.
- **REQ-CORE.STO.PRIV-007** `settings.show_privacy@1` shall be side-effect class R0 and shall report the consent state, the time and principal kind of the last change, and the description digest.

## Acceptance criteria

- **AC-1** Given fixture script `fixtures/core/sto/priv/offline_session.json` (create project, import a test-kit element, edit, save, close, reopen), when run on Linux inside a network namespace with no interfaces, then every step succeeds and the session completes.
- **AC-2** Given a fresh installation, when `fid settings show-privacy --json` runs, then it reports consent `off`.
- **AC-3** (error path) Given an agent principal over MCP, when it tries to set telemetry consent, then no MCP tool exists for it, and a direct bus call returns `policy` and journals nothing.
- **AC-4** (error path) Given a call from a `script` principal (for example `fid settings set-telemetry-consent on` from the CLI without any human upgrade that D-012 may introduce), then it fails with `policy` because the principal is not human.
- **AC-5** Given consent set on by a human in the GUI test harness, when the telemetry description fixture is changed, then `show-privacy` reports consent `off` with reason `description_changed`.
- **AC-6** Given consent turned off by a human, then the Record Journal holds one W3 `settings.set_telemetry_consent@1` entry and `show-privacy` reports `off`.

## Design

- **Registry command(s):**
  - `settings.set_telemetry_consent@1` W3, MCP projection `request_only`, D0; params `{ consent: bool }`; result `{ consent, description_digest }`; requires human principal (and the human assertion that W3 entries carry, per `CORE.JRN.ENTRY`).
  - `settings.show_privacy@1` R0; result `{ consent, reason?, changed_at?, changed_by_kind?, description_digest }`.
- **Data structures:** setting key `privacy.telemetry_consent` (user scope, admin flag false, owned by this leaf, written only by its own command); `telemetry-description.json` shipped with the application, hashed with `fid-canon`.
- **Algorithm:** consent is effective only when the stored value is on and the stored description digest equals the shipped one.
- **Errors:** `policy` (non-human principal).
- **Determinism:** D0.
- **Performance budget:** not critical.
- **Crate:** `fid-store` (module `privacy`).

## Test fixtures

`fixtures/core/sto/priv/offline_session.json` (command script), `fixtures/core/sto/priv/telemetry-description.v1.json` and `v2.json`; CC-BY-4.0. The offline test runs on Linux CI with a network namespace; Windows and macOS runs use a socket-deny test hook [unverified that the hook covers all socket paths].

## Build prompt

> In crate `fid-store`, add module `privacy`. Declare the user-scope setting `privacy.telemetry_consent` (default off) and register two commands: `settings.set_telemetry_consent@1` (side effect W3, MCP projection request_only, determinism D0; only a human principal may commit it; on `true` record the SHA-256 of the shipped `telemetry-description.json`) and `settings.show_privacy@1` (R0; consent, reason, last change time, principal kind, description digest). Effective consent requires the stored digest to equal the shipped description digest; otherwise report off with reason `description_changed`. No core command may require an account. Add an offline integration test that runs `fixtures/core/sto/priv/offline_session.json` inside a Linux network namespace with no interfaces. CLI `fid settings set-telemetry-consent | show-privacy`; MCP exposes only `settings_show_privacy`. Tests: AC-1..AC-6. Constraints: CONSTITUTION C7 (local first, telemetry off unless opted in), §3 (W3 is human-only), §8 (no panic, no unsafe).

## Open questions

- **Revise for D-035 (accepted 2026-11-17):** a user's own telemetry or update consent is W2 (journaled, default off); site-wide policy is A0 (CONSTITUTION §3.1).
- Telemetry consent is classed W3 because the concept docs list consent among controlled-record changes; CONSTITUTION §3.1 does not name it. If maintainers prefer A0, change the class before the leaf is built.

## Children

_Leaf: none._
