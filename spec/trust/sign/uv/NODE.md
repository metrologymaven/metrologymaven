---
id: TRUST.SIGN.UV
title: User-verification release and S1 refusal
level: 3
parent: TRUST.SIGN
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST.SIGN-005, REQ-TRUST.SIGN-006]
covers: []
depends_on: [TRUST.SIGN.KEYSTORE.TRAIT]
cli: []
mcp: []
journal: [sign.refuse@1]
updated: 2026-11-17
---

# TRUST.SIGN.UV: User-verification release and S1 refusal

## Purpose

The gate in front of every private-key operation: it refuses any request whose principal is not a human on an attended surface, and otherwise demands a fresh OS user-verification prompt before the keystore handle signs. It is the only path to `Signer::sign_pae` for real keys. It is separate from the keystore so that the policy is one small piece of code reviewed on its own.

## Scope

**In:**
- Principal and channel check (refuse agent, MCP, control channel, script host)
- Per-OS user-verification prompt bound to a single sign operation (no caching)
- The `uv_method` value recorded for each signature
- Refusal error and its recording via `sign.refuse@1` (event shape owned by `TRUST.ESIG.BIND`)
- The fallback where the OS offers no user verification (blocked on D-011)

**Out** (and which node owns it instead):
- Key handles (`TRUST.SIGN.KEYSTORE`)
- The ceremony preview and confirmation (`TRUST.ESIG.CEREMONY`)
- Side-effect class enforcement in general (`CORE.BUS`)

## Requirements

- **REQ-TRUST.SIGN.UV-001** When a sign operation is requested by a human principal on an attended GUI session, the gate shall require a new OS user-verification prompt for that operation and shall not reuse an earlier verification.
- **REQ-TRUST.SIGN.UV-002** If the principal kind is not `human`, or the channel is MCP, the control channel or a script host, then the gate shall return `sign.s1_forbidden` before any keystore call and the refusal shall be recorded.
- **REQ-TRUST.SIGN.UV-003** If user verification fails, times out after 120 s or is cancelled, then the gate shall return `sign.uv_failed` and shall not call the keystore.
- **REQ-TRUST.SIGN.UV-004** Every successful sign operation shall report the `uv_method` used (e.g. `windows_hello`, `macos_local_auth`, `token_pin_touch`) so it can be bound into the signature statement.
- **REQ-TRUST.SIGN.UV-005** Where the OS offers no user-verification API, the gate shall behave as D-011 decides.

## Interfaces

- **Offers:** `UvGate::sign(principal, channel, handle, pae) -> Result<(SigBytes, UvMethod), SignError>`; errors `sign.s1_forbidden`, `sign.uv_failed`.
- **Consumes:** key handles from `TRUST.SIGN.KEYSTORE`; principal kind and channel from `CORE.BUS`.

## Decomposition hints

GATE (principal/channel policy + tests), WINUV, MACUV, LINUV (per-OS prompt; LINUV per D-011).

## Open questions

- D-011 (accepted 2026-11-17) chose option D: on Linux a PKCS#11 token with PIN (and touch where the token enforces it) when present, otherwise an app-owned passphrase unlocking an Argon2id-wrapped key; the Regulated profile requires the token path on Linux (its `allow_software_keys: false` already excludes wrapped software keys). `uv_method` is recorded in the signature statement. Applied in `TRUST.SIGN.UV.LINUV` and `TRUST.SIGN.UV.GATE`.
- D-012 (accepted) lets a person upgrade a CLI session to `human` after confirmation. `TRUST.SIGN.UV.GATE` therefore accepts `human` principals on the GUI channel or an upgraded CLI terminal session, and still refuses MCP, the control channel and the Python script host whatever the principal, as REQ-TRUST.SIGN-006 requires.
- The `sign.refuse@1` event shape is owned by `TRUST.ESIG.BIND` (excluded from launch-1); GATE reads it as an artifact.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| TRUST.SIGN.UV.GATE | S1 principal and channel gate, policy and uv_method | 002 003 004 | none |
| TRUST.SIGN.UV.OSUV | Windows and macOS per-operation user verification | 001 003 004 | none |
| TRUST.SIGN.UV.LINUV | Linux token PIN and passphrase verification (D-011) | 001 003 004 005 | none |
