---
id: TRUST.SIGN.UV.GATE
title: S1 principal and channel gate, policy and uv_method
level: 4
parent: TRUST.SIGN.UV
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.SIGN.UV-002, REQ-TRUST.SIGN.UV-003, REQ-TRUST.SIGN.UV-004]
covers: []
depends_on: [TRUST.SIGN.KEYSTORE.TRAIT, CORE.BUS.PRIN]
uses_artifacts: [TRUST.ESIG.BIND]
side_effect: S1
determinism: N
cli: []
mcp: []
journal: [sign.refuse@1]
updated: 2026-11-17
---

# TRUST.SIGN.UV.GATE: S1 principal and channel gate, policy and uv_method

## Purpose

`UvGate::sign` is the only path from a caller to a real private-key operation. It refuses before touching the keystore unless the principal is `human` on an attended channel (GUI, or a CLI terminal session upgraded per D-012), applies the caller's UV policy (which methods are acceptable, derived from the governance profile), asks the platform `UvProvider` for one fresh verification, then signs through the key handle and returns the signature with its `uv_method`. Refusals produce a `sign.refuse@1` event value for the calling command to commit.

## Scope

**In:**
- Principal and channel check, before any keystore call
- `UvProvider` trait and `UvMethod` enum
- `UvPolicy` (accepted methods) and its check
- Timeout (120 s) and cancellation handling around the provider
- Refusal event value shaped per `TRUST.ESIG.BIND`

**Out:**
- Platform prompts (`TRUST.SIGN.UV.OSUV`, `TRUST.SIGN.UV.LINUV`)
- Key handles (`TRUST.SIGN.KEYSTORE.*`)
- Principal assignment and session upgrade (`CORE.BUS.PRIN`)
- The ceremony and committing the event (`TRUST.ESIG.CEREMONY`)

## Requirements

- **REQ-TRUST.SIGN.UV.GATE-001** If the principal kind is not `human`, or the channel is MCP, the control channel or the Python script host, then the gate shall return `sign.s1_forbidden` before any keystore or provider call and shall return a `sign.refuse@1` event value naming principal kind, channel and reason.
- **REQ-TRUST.SIGN.UV.GATE-002** The gate shall accept a `human` principal only on the desktop GUI channel or on a CLI terminal session whose upgrade to `human` was recorded by the bus.
- **REQ-TRUST.SIGN.UV.GATE-003** If the provider's method is not in the caller's `UvPolicy`, then the gate shall return `sign.uv_method_not_allowed` naming the method and shall not call the keystore.
- **REQ-TRUST.SIGN.UV.GATE-004** If the provider reports failure or cancellation, or does not complete within 120 s, then the gate shall return `sign.uv_failed` with reason `failed`, `cancelled` or `timeout` and shall not call the keystore.
- **REQ-TRUST.SIGN.UV.GATE-005** On success the gate shall return the signature bytes together with the `uv_method` the provider used, from the closed set `windows_hello`, `macos_local_auth`, `token_pin`, `token_pin_touch`, `passphrase`.
- **REQ-TRUST.SIGN.UV.GATE-006** The gate shall request a new provider verification for every call and shall hold no verification state between calls.

## Acceptance criteria

- **AC-1** Given a fake provider and `MemKeyStore`, when an `agent` principal, a `script` principal, a `human` on MCP and a `human` on the Python host each call `sign`, then each gets `sign.s1_forbidden` with a refusal event, and the fake keystore and provider record zero calls.
- **AC-2** Given a `human` on the GUI channel and on an upgraded CLI session, then `sign` succeeds and returns the fake provider's `uv_method`; two consecutive calls trigger two provider calls.
- **AC-3** (error path) Given a provider returning `passphrase` and a policy allowing only token methods, then `sign.uv_method_not_allowed`; given a provider that never answers, then `sign.uv_failed(timeout)` after 120 s (test clock).
- **AC-4** Given a provider that is cancelled, then `sign.uv_failed(cancelled)` and no keystore call.

## Design

- **Registry command(s):** none; a library used by the signing ceremony's S1 command. Refusal event `sign.refuse@1` is returned as a value for the caller's handler to commit.
- **Data structures:** `trait UvProvider { fn verify(&self, ctx: &UvRequest) -> Result<UvToken, UvError>; }`; `UvToken` (single-use, consumed by the key handle); `UvMethod` closed enum; `UvPolicy { allowed: BTreeSet<UvMethod> }`; `Channel` from CORE.BUS.PRIN.
- **Algorithm:** check principal and channel → call provider with timeout → check method against policy → `handle.sign_with(token, pae)` → return `(sig, method)`.
- **Errors:** `sign.s1_forbidden`, `sign.uv_method_not_allowed`, `sign.uv_failed(failed|cancelled|timeout)`.
- **Determinism:** N (user interaction).
- **Performance budget:** gate overhead < 1 ms excluding the prompt.
- **Crate:** `fid-uv`, module `gate` (never linked by `fid-verify`).

## Test fixtures

Fake `UvProvider` and `MemKeyStore` (feature `test-backend`); a test clock for the timeout.

## Build prompt

> Create crate `fid-uv` (depends on `fid-keystore`, `fid-sign-core` and the bus principal types). Module `gate`: `UvGate::sign(principal, channel, policy: &UvPolicy, handle: &KeyHandle, pae: &[u8]) -> Result<(SigBytes, UvMethod), SignError>`. Refuse with `sign.s1_forbidden` (and return a `sign.refuse@1` event value: principal kind, channel, reason) unless principal is `human` on the GUI channel or on a bus-recorded upgraded CLI session; MCP, control channel and Python host are always refused. Then call the platform `UvProvider` with a 120 s timeout (`sign.uv_failed` with `failed|cancelled|timeout`), check its method against the policy (`sign.uv_method_not_allowed`), and sign once with the single-use `UvToken`. Closed `UvMethod` set: `windows_hello`, `macos_local_auth`, `token_pin`, `token_pin_touch`, `passphrase`. No state between calls. Tests: AC-1..AC-4. Constraints: CONSTITUTION §3 (S1 human only, agents never sign), §8, REQ-TRUST.SIGN-005/006, D-011, D-012.

## Open questions

- The policy is supplied by the caller (from the profile via `TRUST.PROFILE.GATE.QUERY`), so this leaf does not depend on PROFILE; the ceremony must map `allow_software_keys: false` to excluding `passphrase`.

## Children

_Leaf: none._
