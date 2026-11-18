---
id: CORE.BUS.PRIN
title: Principal model, channel binding and human session upgrade
level: 3
parent: CORE.BUS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.BUS-001, REQ-CORE.BUS-002]
covers: []
depends_on: [CORE.JRN.ENTRY, CORE.JRN.CHAIN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.BUS.PRIN: Principal model, channel binding and human session upgrade

## Purpose

Defines who is acting. A principal is derived only from the authenticated channel a call arrives on, never from the request body. This leaf defines the principal type, the channel trait each surface implements, the fixed mapping from channel kind to principal kind (CONSTITUTION §3, D-012 option C, D-030 option B), and the one way a CLI session or script run becomes `human`: an explicit per-session confirmation by a person in the running app, or an OS user-presence check when headless. The policy and journal modules rely on this mapping.

## Scope

**In:** `Principal` type, `PrincipalKind` enum, `Channel` trait and `ChannelKind` enum, the channel-to-kind mapping (desktop, CLI, Python, control channel, MCP, device, internal, self-hosted server sessions and server service credentials), handling of untrusted self-asserted client data, rejection of unauthenticated channels, the session upgrade from `script` to `human` (D-012) and the record it produces.
**Out:** the authentication protocols of each surface and the confirmation prompt UI (`AUTO`, desktop apps); the OS user-presence integrations per platform (`PLAT`); identity-provider mapping on the self-hosted server (`ENT.IAM`); policy decisions based on the principal, including the raw-path refusal for `script` (`CORE.BUS.POL.FS`) and the `human_only` refusal (`CORE.BUS.POL.CLASS`); identity pseudonymisation for external principals (`CORE.JRN.STREAM.LEDGER`); appending the upgrade entry's bytes (`CORE.JRN.CHAIN.APPEND`).

## Requirements

- **REQ-CORE.BUS.PRIN-001** The principal model shall have exactly the kinds `human`, `agent`, `script`, `service`, `device` and `system`.
- **REQ-CORE.BUS.PRIN-002** When a call arrives, the bus shall take the principal kind and identity only from the channel's authenticated identity and shall not read any principal, actor or on-behalf-of field in the request parameters.
- **REQ-CORE.BUS.PRIN-003** When a call arrives through the CLI, the Python host, the loopback control channel or any other headless local caller, and the session has not been upgraded, the bus shall assign principal kind `script`.
- **REQ-CORE.BUS.PRIN-004** When a call arrives through an MCP channel, local or on the self-hosted server, the bus shall assign principal kind `agent`.
- **REQ-CORE.BUS.PRIN-005** When a call arrives from a GUI widget of a desktop app in an interactive OS session, the bus shall assign principal kind `human`.
- **REQ-CORE.BUS.PRIN-006** When a call originates inside the core (recompute, replay, migration), the bus shall assign principal kind `system` and record the principal that triggered it as `on_behalf_of`.
- **REQ-CORE.BUS.PRIN-007** The principal model shall type self-asserted client and model names as untrusted text and shall never use them in an authorisation decision.
- **REQ-CORE.BUS.PRIN-008** If a channel cannot supply an authenticated identity, then the bus shall return a typed `unauthenticated` error.
- **REQ-CORE.BUS.PRIN-009** When a CLI session or Python script run requests an upgrade and a person confirms it through the `HumanPresence` provider (a prompt in the running desktop app, or an OS user-presence check when headless), the bus shall assign principal kind `human`, with the confirming OS user as identity, to every later call of that session until the session ends.
- **REQ-CORE.BUS.PRIN-010** If an upgrade is requested on a channel other than CLI or Python, or the confirmation is declined, times out after 120 s, or names an OS user different from the session's OS user, then the bus shall return a typed `upgrade_refused` error and the session shall stay `script`.
- **REQ-CORE.BUS.PRIN-011** When an upgrade is confirmed, the bus shall append one `principal_upgrade` entry to the Record Journal carrying the session ID, the confirmation method, the identity commitment and the time, before any call runs as `human`.
- **REQ-CORE.BUS.PRIN-012** When a call arrives through a self-hosted server session, the bus shall assign principal kind `script` with the authenticated server user recorded as `on_behalf_of`, and when it arrives with a server service credential, principal kind `service` (D-030 option B).
- **REQ-CORE.BUS.PRIN-013** The mapping from channel kind to principal kind shall be total and shall have no default case, so adding a channel kind fails to compile until it is mapped.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/bus/principal/channels.json` (one test channel per kind: gui, cli, python, control, mcp, server_mcp, service, device, internal, server_session, server_service), when `derive_principal` runs for each, then the kinds are human, script, script, script, agent, agent, service, device, system, script (with `on_behalf_of` = the server user), service.
- **AC-2** Given an MCP call whose params contain `{"principal": {"kind": "human"}}`, when derived, then the kind is `agent` and the params field has no effect.
- **AC-3** Given a CLI session run by an interactive user without an upgrade, when derived, then the kind is `script`.
- **AC-4** Given a CLI session and a fake `HumanPresence` that confirms for the same OS user, when the session upgrades and then invokes a W2 test command, then the Record Journal holds one `principal_upgrade` entry followed by the W2 entry with principal kind `human`, and a later session of the same process starts as `script` again.
- **AC-5** (error path) Given a fake `HumanPresence` that declines, times out, or confirms a different OS user, when the upgrade is requested, then the result is `upgrade_refused`, the session stays `script` and the Record Journal is unchanged.
- **AC-6** (error path) Given an MCP or server-session channel, when an upgrade is requested, then the result is `upgrade_refused` and no prompt is shown.
- **AC-7** (error path) Given a test channel that returns no identity, when a command is invoked, then the bus returns `unauthenticated` and journals nothing.
- **AC-8** Given an internal recompute triggered by a human, when derived, then the kind is `system` and `on_behalf_of` names the human.

## Design

- **Registry command(s):** none. The upgrade is a session operation of the bus, not a document command; its record is a journal entry of kind `principal_upgrade` defined in `CORE.JRN.ENTRY`.
- **Data structures:** `enum PrincipalKind { Human, Agent, Script, Service, Device, System }`; `enum ChannelKind { Gui, Cli, Python, Control, Mcp, ServerMcp, ServerSession, ServerService, Device, Internal }`; `Principal { kind, id: PrincipalId, on_behalf_of: Option<PrincipalId>, client_id: Option<ClientId>, self_asserted: Option<Untrusted<SelfAsserted>>, upgraded_session: Option<SessionId> }`; `trait Channel { fn kind(&self) -> ChannelKind; fn authenticated_identity(&self) -> Option<Identity>; fn session(&self) -> SessionId; }`; `trait HumanPresence { fn confirm(&self, req: &UpgradeRequest, deadline: Duration) -> PresenceOutcome; }` with `PresenceOutcome { Confirmed { os_user, method: GuiPrompt | OsPresence }, Declined, TimedOut }`; `SessionTable` holding upgraded sessions in memory only.
- **Algorithm:** derive: exhaustive `match` on `ChannelKind`; for `Cli` and `Python`, look the session up in `SessionTable` and return `human` if upgraded. Upgrade: check channel kind, call the provider with a 120 s deadline, compare OS users, build the `principal_upgrade` draft, append it, then insert the session. Session end (channel closed) removes the row; the table is never persisted, so a restart starts as `script`.
- **Errors:** `unauthenticated`, `upgrade_refused` (reasons `channel`, `declined`, `timeout`, `user_mismatch`).
- **Determinism:** D0 for derivation. The presence outcome is an observed input recorded in the journal entry, so replay reads it and never prompts.
- **Performance budget:** < 1 µs per derivation, excluding the human prompt [unverified until measured].
- **Crate:** `fid-bus` (module `principal`).

## Test fixtures

`fixtures/core/bus/principal/channels.json`: synthetic channel descriptors with expected kinds; a fake `HumanPresence` with scripted outcomes; CC-BY-4.0.

## Build prompt

> In crate `fid-bus`, add module `principal`. Define `PrincipalKind` (human, agent, script, service, device, system), `ChannelKind` (gui, cli, python, control, mcp, server_mcp, server_session, server_service, device, internal), `Principal { kind, id, on_behalf_of, client_id, self_asserted, upgraded_session }` with `self_asserted` wrapped in the project's `Untrusted<T>`, `trait Channel { kind(); authenticated_identity(); session(); }` and `trait HumanPresence { confirm(req, deadline) -> PresenceOutcome }`. Implement `derive_principal(channel) -> Result<Principal, BusError>` with an exhaustive match and no wildcard arm: gui -> human; cli and python -> script unless the session is upgraded, then human; control -> script; mcp and server_mcp -> agent; server_session -> script with `on_behalf_of` = server user; server_service -> service; device -> device; internal -> system with `on_behalf_of` = triggering principal. Never read principal data from request parameters; a channel with no identity yields `Unauthenticated`. Implement `upgrade_session(channel, presence) -> Result<(), BusError>`: only cli or python channels; 120 s deadline; refuse decline, timeout and a confirming OS user that differs from the session's; on success append a `principal_upgrade` entry (session ID, method, identity commitment, time) through the journal API before inserting the session into an in-memory table; the table is never persisted. Tests: AC-1..AC-8 with `fixtures/core/bus/principal/channels.json` and a fake presence provider. Constraints: CONSTITUTION §3 Principals (D-012 option C, D-030 option B), C6 (untrusted text), §8 (no panic, no unsafe, deny unwrap/expect).

## Open questions

- **Revise for D-035 (accepted 2026-11-17):** add the injected-input rule (GUI input is `human` only from local OS input devices; the UI host passes an origin token; injected input carries the channel's principal), and expose `HumanPresence` as the Lean-profile human assertion for W3 release and protected-branch promotion.
- **Applied (2026-11-17):** D-012 option C (script by default, per-session human confirmation) and D-030 option B (server callers are `script` with `on_behalf_of`, service credentials `service`). REQ-003 was narrowed to non-upgraded sessions; REQ-009..013 are new.
- D-030 option B reads "every server caller except a service credential is `script`". This leaf maps the server's MCP channel to `agent`, as for local MCP, because `agent` is the stricter kind and keeps the CONSTITUTION §3 agent bans; confirm with `ENT.IAM`.
- Whether a GUI call driven by an accessibility automation tool should count as `human`; this leaf treats every GUI-widget call in an interactive session as `human`.
- The confirmation prompt UI (which process, what it shows, how it resists spoofing) belongs to the desktop apps and `AUTO.CLI`; the OS presence integrations belong to `PLAT`. This leaf fixes only the `HumanPresence` trait they implement.

## Children

_Leaf: none._
