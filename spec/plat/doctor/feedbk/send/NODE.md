---
id: PLAT.DOCTOR.FEEDBK.SEND
title: Consent, site policy and send through the egress primitive
level: 4
parent: PLAT.DOCTOR.FEEDBK
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.DOCTOR.FEEDBK-001]
covers: []
depends_on: [PLAT.DOCTOR.FEEDBK.COMPOSE, CORE.BUS.POL.EGRESS, CORE.STO.SET, CORE.BUS]
uses_artifacts: [PLAT.DEPLOY.MCONF]
side_effect: [X1, W2]
determinism: N
cli: [fid doctor send-report, fid doctor set-feedback-consent]
mcp: [doctor_send_report, doctor_set_feedback_consent]
journal: [doctor.send_report@1, doctor.set_feedback_consent@1]
updated: 2026-11-17
---

# PLAT.DOCTOR.FEEDBK.SEND: Consent and send

## Purpose

Sends a previewed feedback bundle to the configured issue intake, and only when three things hold: the site has not disabled feedback, the user has opted in, and a human approves this specific bundle digest and destination. The send itself goes through the platform's single egress primitive, so the network path, approval and journal entry are the same as for every other byte that leaves the machine.

## Scope

**In:**
- `doctor.send_report@1` (X1, request-only for agents)
- `doctor.set_feedback_consent@1` (W2, `human_only`)
- Setting declarations: `feedback.consent` (user scope, `on|off`, default `off`), `feedback.site_policy` (machine scope, admin flag, `user_choice|disabled`, default `user_choice`, lockable by the managed policy file), `feedback.destination` (machine, admin flag, an egress destination ID, default empty)
- Intake contract: HTTPS POST of the bundle bytes with content type `application/vnd.fid.feedback+json`; the intake replies with an untrusted receipt ID
- Resetting the crash counters after a successful send

**Out** (and which node owns it instead):
- The egress primitive, approval dialog and destination storage (`CORE.BUS.POL.EGRESS`, `CORE.STO.SET`)
- The bundle (`PLAT.DOCTOR.FEEDBK.COMPOSE`)
- The intake service itself (project infrastructure, outside the product)

## Requirements

- **REQ-PLAT.DOCTOR.FEEDBK.SEND-001** `doctor.send_report@1` shall be class X1, take a staged bundle digest, and transmit only by invoking `egress.send@1` with that digest and the `feedback.destination` ID; it shall open no socket itself.
- **REQ-PLAT.DOCTOR.FEEDBK.SEND-002** If `feedback.site_policy` is `disabled`, then the command shall fail with `feedback.disabled_by_policy` and send nothing, whatever the user's consent.
- **REQ-PLAT.DOCTOR.FEEDBK.SEND-003** If `feedback.consent` is `off` or `feedback.destination` is empty, then the command shall fail with `feedback.not_opted_in` or `feedback.no_destination` and send nothing.
- **REQ-PLAT.DOCTOR.FEEDBK.SEND-004** The command shall send only with a per-invocation human approval of the exact digest and destination (from `egress.send@1`); calls from agent or script principals shall become pending requests.
- **REQ-PLAT.DOCTOR.FEEDBK.SEND-005** If the digest is not a staged feedback bundle that validates against `feedback-bundle.json` v1, then the command shall fail with `feedback.bad_bundle` and send nothing.
- **REQ-PLAT.DOCTOR.FEEDBK.SEND-006** `doctor.set_feedback_consent@1` shall be class W2 with `human_only: true`, take `on` or `off`, be journaled in the per-user settings journal, and fresh installs shall have consent `off`.
- **REQ-PLAT.DOCTOR.FEEDBK.SEND-007** After a send with outcome `sent`, the command shall reset the crash session counters and remove the sent crash record from the queue; after any other outcome it shall change nothing.

## Acceptance criteria

- **AC-1** Given consent on, policy `user_choice`, a test destination served by a local HTTPS server, and a bundle staged by COMPOSE AC-1, when a GUI human runs send and confirms the approval, then the server receives bytes whose SHA-256 equals the digest, the journal holds `doctor.send_report@1` and `egress.send@1` entries with the digest, and the crash record is gone from the queue.
- **AC-2** (error path) Given policy `disabled` set through the managed policy fixture `fixtures/plat/doctor/feedbk/policy-disabled.toml`, then `feedback.disabled_by_policy` and the server receives nothing.
- **AC-3** (error path) Given a fresh install, then `fid settings list-values --prefix feedback` shows consent `off` and send fails with `feedback.not_opted_in`.
- **AC-4** (agent) Given an MCP agent calling `doctor_send_report`, then a pending request is filed and nothing is sent; `doctor_set_feedback_consent` from an agent also becomes a request and changes nothing.
- **AC-5** (error path) Given a digest of a non-feedback blob, then `feedback.bad_bundle`.
- **AC-6** Given the server stopped, when a human approves, then outcome `failed` is journaled, the queue and counters are unchanged, and nothing retries.

## Design

- **Registry command(s):**
  - `doctor.send_report@1`: X1, projection request_only, N; params `{ bundle: Digest }`; result `{ outcome: sent|failed|cancelled, receipt?: Untrusted<String> }`. Handler checks policy, consent, destination and bundle, then calls `egress.send@1` as a nested invocation carrying the approval.
  - `doctor.set_feedback_consent@1`: W2, `human_only: true`, D0; params `{ consent: on|off }`.
- **Settings:** declared with the `fid_settings!` arm; `feedback.site_policy` and `feedback.destination` admin-flagged (A0 changes via `settings.set_admin_value@1`).
- **Errors:** `feedback.disabled_by_policy`, `feedback.not_opted_in`, `feedback.no_destination`, `feedback.bad_bundle`, plus egress errors passed through.
- **Determinism:** N (network outcome).
- **Crate:** `fid-doctor` (module `feedback::send`).

## Test fixtures

A local HTTPS test server with a self-signed certificate created at test time, policy fixtures under `fixtures/plat/doctor/feedbk/`. Licensed with the code.

## Build prompt

> In crate `fid-doctor`, module `feedback::send`, declare settings `feedback.consent` (user, on|off, default off), `feedback.site_policy` (machine, admin, user_choice|disabled), `feedback.destination` (machine, admin, egress destination ID, default empty), and register `doctor.send_report@1` (X1, request-only on MCP; param staged bundle digest) and `doctor.set_feedback_consent@1` (W2, `human_only: true`). CLI `fid doctor send-report|set-feedback-consent` and MCP tools are generated from the registry. Send checks site policy, consent, destination and that the digest is a valid `feedback-bundle.json` v1 blob, then invokes `egress.send@1` (never opens a socket itself; human approval per invocation; agent/script calls become pending requests). On `sent`, reset crash session counters and remove the sent crash record; otherwise change nothing; never retry. Tests: AC-1..AC-6. Constraints: CONSTITUTION §3.1 (egress only via `egress.send@1`; consent W2; site policy A0), C7, §8. Commit: `feat(fid-doctor): send feedback through egress`.

## Open questions

- The project's intake URL and who runs it are a maintainer choice; the product ships with no destination configured.

## Children

_Leaf: none._
