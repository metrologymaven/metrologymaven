---
id: ECO.REL.UPD
title: Opt-in update check
level: 3
parent: ECO.REL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REL-007]
covers: []
depends_on: [CORE.STO.SET, CORE.BUS.POL, TRUST.SIGN.DSSE]
uses_artifacts: [ECO.REL.PROC.CHANNEL, ECO.REL.SIGN.GUIDE, QUAL.VULN]
side_effect: [X1, W2, R0]
determinism: D0
cli: [fid update check, fid update set-consent, fid update show-status]
mcp: [update_check, update_set_consent, update_show_status]
journal: [update.check@1, update.set_consent@1]
updated: 2026-11-17
---

# ECO.REL.UPD: Opt-in update check

## Purpose

The in-product way to learn that a newer release exists on the user's channel: off by default (C7), enabled only by the user's own consent and allowed by site policy, verifying the channel feed's signature before showing anything, and never downloading or installing by itself. It is the only part of ECO.REL that runs inside the product, so its crate sits in layer L3 (surfaces) even though its parent's other children are tooling.

## Scope

**In:**
- `update.set_consent@1` (W2, `human_only`): the user's own consent and channel choice (CONSTITUTION §3.1, D-031)
- Site-wide policy as admin-flagged settings changed only through `settings.set_admin_value@1` (A0) or locked by a managed policy file: `update.site_policy` (`user_choice` or `disabled`) and `update.feed_base_url` (for internal mirrors)
- `update.check@1` (X1): one HTTPS GET of the channel feed through the CORE.BUS egress primitive, DSSE verification against bundled keys, rollback and staleness checks, version comparison
- `update.show_status@1` (R0): consent, policy, last result
- Showing the result with links to release notes and the download page; offline behaviour without error noise

**Out** (and which node owns it instead):
- Channel feeds and their signing (owned by `ECO.REL.PROC.CHANNEL` and `ECO.REL.SIGN.CUSTODY`)
- Installing updates (people do that, or enterprise tooling under `PLAT.DEPLOY`)
- The settings system, admin command and managed-policy lock (owned by `CORE.STO.SET`; the lock file by `PLAT.DEPLOY.MCONF`)
- X1 approval and the network guard (owned by `CORE.BUS.POL`)
- Advisory content (owned by `QUAL.VULN`; this leaf shows advisory IDs the feed carries)

## Requirements

- **REQ-ECO.REL.UPD-001** A fresh installation shall have consent `off` and shall make no network request for updates until a human sets consent `on`.
- **REQ-ECO.REL.UPD-002** `update.check@1` shall be side-effect class X1 and request-only on MCP, and shall perform its single network request only through the CORE.BUS egress primitive `egress.send@1`.
- **REQ-ECO.REL.UPD-003** `update.set_consent@1` shall be side-effect class W2 with `human_only: true`, shall be journaled, and shall take `consent` (`on` or `off`) and `channel` (`beta`, `stable` or `lts`).
- **REQ-ECO.REL.UPD-004** While `update.site_policy` is `disabled`, `update.check@1` shall refuse with `update.disabled_by_policy` and make no network request, whatever the user's consent.
- **REQ-ECO.REL.UPD-005** If the feed's DSSE envelope does not verify against a release public key bundled with the installed version (or a key reached from it through a verified key-transition statement), then the check shall report `feed_not_trusted` and show no version information from it.
- **REQ-ECO.REL.UPD-006** If a verified feed's `sequence` is lower than the highest sequence previously accepted for that channel, then the check shall report `feed_rollback` and show no version information from it.
- **REQ-ECO.REL.UPD-007** If a verified feed's `expires_at` is earlier than the current time, then the check shall report `feed_stale` together with the version information.
- **REQ-ECO.REL.UPD-008** The request shall carry no user, machine, install or usage identifier, no cookie and no query string; its URL shall be the feed base URL plus `<channel>.json`, and its User-Agent shall be the product name and version only.
- **REQ-ECO.REL.UPD-009** If the network is unavailable or the request fails or times out after 15 s, then the check shall return `offline` without raising an error dialog or a non-zero CLI exit.
- **REQ-ECO.REL.UPD-010** On the lts channel, the check shall compare only against the entry for the installed `MAJOR.MINOR` line, and shall report `line_ended` when that line is absent from the feed.
- **REQ-ECO.REL.UPD-011** The product shall never download or install an update; the result shall offer only links to the release notes and download page.

## Acceptance criteria

- **AC-1** Given a fresh test installation with a socket-deny hook, when the app starts, runs for 60 s and `fid update show-status --json` runs, then no connection is attempted and the status reports `consent: off`.
- **AC-2** Given consent `on` (set by a human in the GUI test harness) and fixture feed `fixtures/rel/upd/stable-1.5.0.dsse.json` signed by the fixture release key served from a loopback server, when `fid update check --json` runs on version 1.4.2 with an approved X1 request, then it returns `update_available` with version 1.5.0, the notes and download URLs, and advisory IDs; one `update.check@1` entry is journaled with the feed digest.
- **AC-3** (error path) Given a feed with one altered payload byte, then the result is `feed_not_trusted` and no version is shown.
- **AC-4** (error path) Given a previously accepted sequence 12 and a feed with sequence 11, then the result is `feed_rollback`.
- **AC-5** (error path) Given `update.site_policy = disabled` set through `settings.set_admin_value@1`, when the check runs with consent `on`, then it fails with `update.disabled_by_policy` and the loopback server logs no request.
- **AC-6** (error path) Given an agent principal over MCP, when it calls `update_set_consent`, then the call becomes a pending request for a human and consent is unchanged; when it calls `update_check`, it becomes a request, not an execution.
- **AC-7** Given the server stopped, when the check runs, then it returns `offline` within 16 s and the CLI exits 0.
- **AC-8** Given the loopback server's request log for AC-2, then the request has no query string, no cookie, and a User-Agent equal to `<product>/<version>`.
- **AC-9** Given installed version 1.2.4 on the lts channel and a feed listing lines 1.2 (latest 1.2.6) and 1.4, then the result is `update_available` with 1.2.6; given a feed without 1.2, the result is `line_ended`.

## Design

- **Registry command(s):**
  - `update.check@1` X1, MCP request-only; params none (channel and feed URL come from settings); result `{ status: update_available|up_to_date|feed_not_trusted|feed_rollback|feed_stale|line_ended|offline, installed, latest?, notes_url?, download_url?, advisories?, feed_sha256? }`.
  - `update.set_consent@1` W2, `human_only: true`; params `{ consent: on|off, channel: beta|stable|lts }`; result the new state. Nightly is not offered: nightly builds use the nightly feed and key and are for testers [see Open questions].
  - `update.show_status@1` R0; result `{ consent, channel, site_policy, locked_keys, last_check_at?, last_status?, highest_sequence }`.
- **Settings declared (CORE.STO.SET):** `update.consent` (user scope, not admin), `update.channel` (user), `update.site_policy` (machine scope, admin flag), `update.feed_base_url` (machine scope, admin flag; default the project's release host), `update.highest_sequence.<channel>` (user, written only by `update.check@1`).
- **Algorithm:** check policy and consent → stage an empty request artefact with destination `<feed_base_url>/<channel>.json` → `egress.send@1` (15 s timeout, 1 MiB response cap) → bounded DSSE parse → verify with bundled keys and verified transitions (`TRUST.SIGN.DSSE` verifier) → schema-validate payload (feed schema v1 of `ECO.REL.PROC.CHANNEL`) → sequence and expiry checks → compare with installed version by Semantic Versioning 2.0.0 precedence (same rule as `ECO.REL.PROC.VERSION`) → result.
- **Errors:** `update.disabled_by_policy`, `update.consent_off`, `policy` (non-human principal on set_consent); feed problems are results, not errors.
- **Determinism:** D0 for verification and comparison given the feed bytes and clock value; the fetch itself is observed I/O, and the journal records the feed digest so replay re-uses it.
- **Performance budget:** verification under 50 ms for a 1 MiB feed.
- **Crate:** `fid-update` (layer L3), using `fid-sign-core` for DSSE verification and the CORE.BUS egress primitive.

## Test fixtures

`fixtures/rel/upd/`: feeds for stable, lts and a tampered and a rolled-back variant, signed with a fixture-only Ed25519 key marked "TEST KEY, NEVER TRUST"; a loopback HTTPS server with a test certificate; CC-BY-4.0.

## Build prompt

> Create crate `fid-update` (layer L3, depends on the settings, bus and `fid-sign-core` crates only). Register three commands: `update.check@1` (X1, MCP request-only), `update.set_consent@1` (W2, `human_only: true`, params consent on|off and channel beta|stable|lts), `update.show_status@1` (R0); CLI `fid update check|set-consent|show-status` and MCP `update_check`, `update_set_consent`, `update_show_status` are generated from the registry. Declare settings `update.consent`, `update.channel` (user), `update.site_policy` (user_choice|disabled) and `update.feed_base_url` (machine, admin flag, changed only by `settings.set_admin_value@1`, A0, and lockable by the managed policy file), and `update.highest_sequence.<channel>`. Defaults: consent off. The check refuses when site policy is disabled or consent is off; otherwise it sends one GET for `<feed_base_url>/<channel>.json` through `egress.send@1` with no identifiers, cookies or query string, User-Agent `<product>/<version>`, 15 s timeout, 1 MiB cap; verifies the DSSE envelope against bundled release keys or keys reached by verified key-transition statements; validates the channel-feed v1 payload; rejects lower sequences (feed_rollback); flags expired feeds (feed_stale); on lts compares within the installed MAJOR.MINOR line (line_ended if absent); compares by Semantic Versioning 2.0.0 precedence; returns links only and never downloads. Network failures return `offline` with exit 0. Journal the feed digest. Tests: AC-1..AC-9 of spec node ECO.REL.UPD with fixture-only keys and a loopback server. Constraints: CONSTITUTION §3 (X1 request-only for agents, human_only consent), C7 (offline, off by default), §8 (no panics, deny unwrap/expect, bounded parsers, fail closed).

## Open questions

- **Applied (D-031, CONSTITUTION §3.1):** site-wide update-check policy is A0 (admin settings through `settings.set_admin_value@1`, lockable by `PLAT.DEPLOY.MCONF`); a user's own consent is W2, journaled and never on by default. The earlier question about W2 versus A0 is settled.
- Automatic periodic checks (for example at start-up) would run an X1 command without a per-invocation human approval, which `CORE.BUS.POL` REQ-007 forbids. This leaf only runs checks on explicit request. If maintainers want start-up checks, `CORE.BUS.POL` must define whether a recorded consent counts as a standing approval for this one command (cross-domain).
- `egress.send@1` is defined in CONSTITUTION §3.1 as taking a staged artefact and a destination; this leaf needs it to return the response body for a GET. `CORE.BUS.POL` (EGRESS child) should confirm that shape (cross-domain).
- Nightly builds use a separate nightly feed signed with the nightly key; a nightly build checks only that feed and trusts only the nightly key. This is left to a later change because nightly builds are for testers.
- PLAT.DEPLOY depends on ECO.REL, so this leaf does not declare a dependency on `PLAT.DEPLOY.MCONF`; the lock reaches it through `CORE.STO.SET`.

## Children

_Leaf: none._
