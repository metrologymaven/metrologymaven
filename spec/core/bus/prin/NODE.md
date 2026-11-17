---
id: CORE.BUS.PRIN
title: Principal model and channel binding
level: 3
parent: CORE.BUS
kind: task
status: open
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-CORE.BUS-001, REQ-CORE.BUS-002]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.BUS.PRIN: Principal model and channel binding

## Purpose

Defines who is acting. A principal is derived only from the authenticated channel a call arrives on, never from the request body. This leaf defines the principal type, the channel trait each surface implements, and the fixed mapping from channel kind to principal kind that the policy and journal rely on.

## Scope

**In:** `Principal` type, `PrincipalKind` enum, `Channel` trait, the channel-to-kind mapping, handling of untrusted self-asserted client data, rejection of unauthenticated channels.
**Out:** the authentication protocols of each surface (`AUTO`); policy decisions based on the principal (`CORE.BUS.POL`); identity pseudonymisation for external principals (`CORE.JRN.STREAM`, Access Ledger).

## Requirements

- **REQ-CORE.BUS.PRIN-001** The principal model shall have exactly the kinds `human`, `agent`, `script`, `service`, `device` and `system`.
- **REQ-CORE.BUS.PRIN-002** When a call arrives, the bus shall take the principal kind and identity only from the channel's authenticated identity and shall not read any principal, actor or on-behalf-of field in the request parameters.
- **REQ-CORE.BUS.PRIN-003** When a call arrives through the CLI, the Python host, the loopback control channel or any other headless caller, the bus shall assign principal kind `script`.
- **REQ-CORE.BUS.PRIN-004** When a call arrives through an MCP channel, the bus shall assign principal kind `agent`.
- **REQ-CORE.BUS.PRIN-005** When a call arrives from a GUI widget in an interactive OS session, the bus shall assign principal kind `human`.
- **REQ-CORE.BUS.PRIN-006** When a call originates inside the core (recompute, replay, migration), the bus shall assign principal kind `system` and record the principal that triggered it as `on_behalf_of`.
- **REQ-CORE.BUS.PRIN-007** The principal model shall type self-asserted client and model names as untrusted text and shall never use them in an authorisation decision.
- **REQ-CORE.BUS.PRIN-008** If a channel cannot supply an authenticated identity, then the bus shall return a typed `unauthenticated` error.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/bus/principal/channels.json` (one test channel per channel kind: gui, cli, python, control, mcp, service, device, internal), when `derive_principal` runs for each, then the kinds are human, script, script, script, agent, service, device, system respectively.
- **AC-2** Given an MCP call whose params contain `{"principal": {"kind": "human"}}`, when derived, then the principal kind is `agent` and the params field has no effect on the result.
- **AC-3** Given a CLI call run by an interactive user, when derived, then the kind is `script`, not `human`.
- **AC-4** (error path) Given a test channel that returns no identity, when a command is invoked, then the bus returns `unauthenticated` and journals nothing.
- **AC-5** Given an internal recompute triggered by a human, when derived, then the kind is `system` and `on_behalf_of` names the human.

## Design

- **Registry command(s):** none.
- **Data structures:** `enum PrincipalKind { Human, Agent, Script, Service, Device, System }`; `Principal { kind, id: PrincipalId, on_behalf_of: Option<PrincipalId>, client_id: Option<String>, self_asserted: Option<Untrusted<SelfAsserted>> }`; `trait Channel { fn kind(&self) -> ChannelKind; fn authenticated_identity(&self) -> Option<Identity>; }`.
- **Algorithm:** a total `match` from `ChannelKind` to `PrincipalKind`; no default arm, so adding a channel kind forces an explicit mapping.
- **Errors:** `unauthenticated`.
- **Determinism:** D0.
- **Performance budget:** < 1 µs per derivation [unverified until measured].
- **Crate:** `fid-bus` (module `principal`).

## Test fixtures

`fixtures/core/bus/principal/channels.json`: synthetic channel descriptors with expected kinds; CC-BY-4.0.

## Build prompt

> In crate `fid-bus`, add module `principal` with `PrincipalKind` (human, agent, script, service, device, system), `Principal { kind, id, on_behalf_of, client_id, self_asserted }` where `self_asserted` is wrapped in the project's `Untrusted<T>` type, and `trait Channel { kind(); authenticated_identity(); }`. Implement `derive_principal(channel) -> Result<Principal, BusError>`: gui -> human; cli, python, control channel and other headless callers -> script; mcp -> agent; service -> service; device -> device; internal core calls -> system with `on_behalf_of` set to the triggering principal. Never read principal data from request parameters. A channel with no authenticated identity yields `BusError::Unauthenticated`. Use an exhaustive match without a wildcard arm. Tests: AC-1..AC-5 with `fixtures/core/bus/principal/channels.json`. Constraints: CONSTITUTION §8 (no panic, no unsafe, deny unwrap/expect), C6 (untrusted text is data).

## Open questions

- **Unblocked (2026-11-17):** D-012 (Principal kind of a human at the CLI or starting a Python script) is accepted. Apply its Decision section (`decisions/D-012-*.md`) when this node is expanded.
- **Blocked on D-012** (open, raised concurrently by AUTO): whether a person at the CLI, or a script a person starts, is `human` or `script`. The content above is written to option A, which is REQ-CORE.BUS-002 as accepted. If D-012 picks C, add a per-session confirmation that upgrades the principal (new requirement and acceptance criterion) and keep `script` as the default; if it picks B, change REQ-003 and AC-3. Return this node to `leaf` once D-012 is decided.
- Whether a GUI call driven by an accessibility automation tool should count as `human`; this leaf treats every GUI-widget call in an interactive session as `human`.

## Children

_Leaf: none._
