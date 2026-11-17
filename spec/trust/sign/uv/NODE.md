---
id: TRUST.SIGN.UV
title: User-verification release and S1 refusal
level: 3
parent: TRUST.SIGN
kind: module
status: decision
decision: D-011
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST.SIGN-005, REQ-TRUST.SIGN-006]
covers: []
depends_on: [TRUST.SIGN.KEYSTORE, CORE]
cli: []
mcp: []
journal: [sign.refuse@1]
updated: 2026-11-16
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

- Blocked on D-011 (Linux fallback). Windows and macOS parts could proceed, but a split now would guess at the shape of the Linux path and at `uv_method` values.
- REQ-CORE.BUS-002 assigns principal kind `script` to every CLI invocation, so an "interactive CLI" signing session (REQ-TRUST.ESIG-001) is refused by REQ-TRUST.SIGN.UV-002 unless CORE.BUS defines an attended-terminal human principal. This is the open decision D-012 (principal kind of a human at the CLI).

## Children

_None yet: status `decision` (D-011)._
