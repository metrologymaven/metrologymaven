---
id: CORE.BUS.PORT
title: In-process bus port: sessions, human upgrade and calls
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
depends_on: [CORE.BUS.API, CORE.BUS.PRIN, CORE.BUS.PIPE.STAGE]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-25
---

# CORE.BUS.PORT: In-process bus port: sessions, human upgrade and calls

## Purpose

The implementation of `BusPort` (`CORE.BUS.API`) over a composed `fid_bus::Bus`, inside `fid-bus`. One `LocalPort` is one authenticated connection: it owns its session key and, after a confirmed upgrade, its upgrade grant, and neither ever leaves `fid-bus`. A surface holding a port can ask for one call at a time, read the principal kind the bus derives, and ask for a human upgrade. It cannot choose who confirms that upgrade, present a grant, or change the identity its channel carries. Those inputs come from the `SessionHost`, which only code that links `fid-bus` can build (the composition root, `AUTO.SURF.COMPOSE`). This is how the CLI's `--confirm-human` reaches `HumanPresence` without `fid-cli` linking `fid-bus`, while a script still cannot make itself `human` (CORE.BUS.PRIN REQ-003 and REQ-009).

## Scope

**In:**
- `SessionHost`: the process's `SessionTable`, the OS user, the presence provider (or none) and the upgrade clock
- `LocalBus`: one composed `Bus`, its per-call parts (state view and precondition roots) and, for a bus over a project, its upgrade journal
- `LocalPort`: opening a session of kind `cli`, `python`, `control` or `mcp`; its channel; `principal_kind`; `call`; `confirm_human`; `attach` to another `LocalBus`; close
- Running PRIN's three upgrade steps with no session-table lock held while a person is asked, and holding a confirmed upgrade until a bus with an upgrade journal can record it
- Reporting each failure with the pipeline stage that raised it

**Out:**
- The call types and the trait (`CORE.BUS.API`)
- Principal derivation, the session table, grants and the `principal_upgrade` entry (`CORE.BUS.PRIN`)
- The pipeline and its stages (`CORE.BUS.PIPE.STAGE`)
- Building a `Bus` with handlers, choosing the OS user and the presence provider, opening projects (`AUTO.SURF.COMPOSE`)
- Turning replies into envelopes and transport events (`AUTO.SURF.INPROC`)
- GUI channels, which belong to the desktop apps and are never opened through a port; the control-channel server's binding of clients (`AUTO.CTRL.EXEC`)
- OS presence checks and the in-app prompt (`TRUST.SIGN.UV`, `PLAT`, desktop apps)

## Requirements

- **REQ-CORE.BUS.PORT-001** The port shall offer ways to open sessions of the kinds `cli`, `python`, `control` and `mcp` only, and none to open a channel of kind `gui`, `internal`, `device`, `service`, `server_mcp`, `server_session` or `server_service`.
- **REQ-CORE.BUS.PORT-002** When `SessionHost::open` opens a port, the port shall take its session key from `SessionTable::open_session` and present, as its channel's authenticated identity, the subject and OS user given to `SessionHost::new`.
- **REQ-CORE.BUS.PORT-003** The public constructors of `SessionHost` and `LocalBus` shall take only `fid-bus` types for the OS user, presence provider, upgrade clock, bus, call parts and upgrade journal, so that only a crate that links `fid-bus` can build them.
- **REQ-CORE.BUS.PORT-004** The port shall keep its session key, its upgrade grant and any pending upgrade private to `fid-bus`, with no operation that returns, clones or accepts any of them.
- **REQ-CORE.BUS.PORT-005** When `principal_kind` is called, the port shall return the kind that `SessionTable::derive_principal` derives for the port's channel at that moment.
- **REQ-CORE.BUS.PORT-006** When `call` is called, the port shall run exactly one `Bus::execute` on its own channel with an invocation whose command, parameters and dry-run flag are the `BusCall`'s and whose other fields are empty.
- **REQ-CORE.BUS.PORT-007** When `confirm_human` is called, the port shall run `SessionTable::prepare_upgrade` (with its channel check) first, then check that the session host has a presence provider, then run `PendingUpgrade::confirm` with that provider and the host's clock while holding no session-table lock, and then `SessionTable::commit_upgrade` on its bus's upgrade journal.
- **REQ-CORE.BUS.PORT-008** If `prepare_upgrade` succeeds and the session host has no presence provider, then `confirm_human` shall return `upgrade_refused` with reason `declined` and detail `no_presence_provider` without asking anyone, leaving the session `script`.
- **REQ-CORE.BUS.PORT-009** While the port's bus has no upgrade journal, the port shall hold a confirmed upgrade as pending and uncommitted, so that `principal_kind` stays `script`.
- **REQ-CORE.BUS.PORT-010** When a port holding a pending upgrade is attached to a bus with an upgrade journal, the port shall commit the upgrade on that journal before `attach` returns.
- **REQ-CORE.BUS.PORT-011** If committing a pending upgrade fails, then the port shall drop the pending upgrade, return the commit error and leave the session `script`.
- **REQ-CORE.BUS.PORT-015** If a port whose upgrade was committed on one upgrade journal is attached to a bus whose upgrade journal is a different one, or none, then `attach` shall refuse with `precondition` reason `false` and detail `upgrade recorded on another journal`, keeping the port on its current bus, so that no call runs as `human` on a journal without its `principal_upgrade` entry (REQ-CORE.BUS.PRIN-011).
- **REQ-CORE.BUS.PORT-012** If a call or an upgrade fails, then the port shall return an `ApiError` holding the bus error's code, reason and detail and the stage that raised it, with stage `session` for an error raised outside the pipeline.
- **REQ-CORE.BUS.PORT-013** When a port is closed or dropped, the port shall end its session through `SessionTable::end_session`, so that its grant is revoked.
- **REQ-CORE.BUS.PORT-014** The port shall not read a principal kind, an upgrade state or an identity from environment variables, argv, files or call parameters.

## Acceptance criteria

- **AC-1** Given a `SessionHost` with OS user `user-a` and no presence provider, and a test bus built with the `fid-bus` `test-support` feature, when one port of each `PortKind` is opened, then `principal_kind` is `script` for `cli`, `python` and `control` and `agent` for `mcp`; and a test that matches `PortKind` exhaustively has exactly those four arms, so no port kind can derive `human` without an upgrade.
- **AC-2** Given a `cli` port on a bus whose upgrade journal is an in-memory `JournalAppend`, and a fake presence provider that confirms `user-a`, when `confirm_human` runs and then a W2 test command is called, then the journal holds one `principal_upgrade` entry followed by the W2 entry with principal kind `human`; a second port opened on the same host stays `script`; and a new `SessionHost` over the same journal opens ports as `script`.
- **AC-3** (error path) Given the scripted outcomes of `fixtures/core/bus/principal/presence.json` (decline, timeout, a confirmation naming `user-b`), when `confirm_human` runs against each, then the result is `upgrade_refused` with reason `declined`, `timeout` and `user_mismatch` and stage `session`, `principal_kind` stays `script` and the journal is unchanged. Given no presence provider, then the result is `upgrade_refused/declined` with detail `no_presence_provider` and a counting provider is never called.
- **AC-4** (error path) Given a test command whose handler returns the principal kind it ran as, when a `cli` port calls it with parameters `{"actor":"user-a","on_behalf_of":"user-a","principal":{"kind":"human"},"session":"s1"}` (its schema allows these properties) and with the process environment holding `FID_PRINCIPAL=human`, then the handler reports `script`.
- **AC-5** Given a fake presence provider that blocks for 2 s inside `confirm` on port A, when port B of the same host calls an R0 test command and port C is opened during that time, then both complete before the provider returns.
- **AC-6** Given a `cli` port on a bus without an upgrade journal and a provider that confirms `user-a`, when `confirm_human` runs, then it returns `Ok`, `principal_kind` is `script` and nothing is appended anywhere; when the port is then attached to a bus with an upgrade journal, then the journal holds one `principal_upgrade` entry before `attach` returns and `principal_kind` is `human`. Given instead an upgrade journal whose `append` fails, then `attach` returns that error, `principal_kind` stays `script` and a second attach to a working journal appends nothing.
- **AC-7** Given the test bus, when a port calls an unknown command, a known command with a schema-invalid parameter document, a W3 command from an `mcp` port, and a command whose handler fails with `not_found/unknown_entity`, then each `ApiError` has the bus's code and reason unchanged and stage `resolve`, `validate`, `authorise` and `handler` respectively.
- **AC-8** Given an open `cli` port holding a grant, when it is dropped, then the session table no longer holds its session, and a port opened afterwards with the same label derives `script`.
- **AC-9** (error path) Given a `cli` port upgraded on project bus A (one `principal_upgrade` entry in A's journal), when it is attached to project bus B, whose upgrade journal has a different `JournalId`, and to the no-project bus, then each `attach` fails with `precondition/false` and detail `upgrade recorded on another journal`, B's journal is unchanged, and the port still calls on bus A as `human`. Given a port with no grant and no pending upgrade, then attaching it to A, B and the no-project bus in turn succeeds each time.
- **AC-10** (error path) Given an `mcp` port and a `control` port on a session host with a provider that would confirm, and a second host with no provider, when `confirm_human` runs on each, then every result is `upgrade_refused` with reason `channel` and stage `session`, the provider's call count is 0, and both ports keep their kinds (`agent`, `script`).

## Design

- **Registry command(s):** none.
- **Data structures (module `fid_bus::port`):**
  - `SessionHost` holds `RwLock<SessionTable>`, `OsUser`, `Option<Arc<dyn HumanPresence + Send + Sync>>` and `Mutex<Box<dyn UpgradeClock + Send>>`. `SessionHost::new(os_user, presence, clock)`; `SessionHost::open(&Arc<Self>, kind: PortKind, label: SessionId, bus: Arc<LocalBus>) -> Result<LocalPort, ApiError>`.
  - `LocalBus::new(bus: Bus, parts: Arc<dyn CallParts>, upgrades: Option<UpgradeJournal>)`, where `trait CallParts { fn inputs(&self, channel: &dyn Channel) -> Result<(Box<dyn StateView>, Box<dyn PredContext>), BusError>; }` supplies each call's state view and precondition roots, and `UpgradeJournal { id: JournalId, append: Mutex<Box<dyn JournalAppend + Send>>, ctx: UpgradeContext }` is where `principal_upgrade` entries go; `id` names the Record Journal (the composer gives one per project's record stream), and a port remembers the `id` its grant was recorded on.
  - `LocalPort { host: Arc<SessionHost>, bus: Arc<LocalBus>, channel: LocalChannel, pending: Option<ConfirmedUpgrade> }`, not `Clone`. `LocalChannel` implements `fid_bus::principal::Channel`: its kind maps `PortKind` to `ChannelKind::{Cli, Python, Control, Mcp}`, its identity is `Identity { subject: os_user, os_user: Some(os_user) }`, and it holds the `SessionKey` and the optional `UpgradeGrant`.
  - `LocalPort` implements `fid_bus_api::BusPort`, plus `attach(&mut self, bus: Arc<LocalBus>) -> Result<(), ApiError>`, `has_pending_upgrade(&self) -> bool` (read by the composition root's raw-path rule) and `close(self)`.
- **Locks.** `open`, `commit_upgrade` and `end_session` take the write lock. `derive_principal`, `prepare_upgrade` and `Bus::execute` take the read lock. `PendingUpgrade::confirm`, which asks a person, runs with no lock held (CORE.BUS.PRIN). A long call delays opening a new session in the same process; that is accepted for v1.
- **Upgrade order.** `confirm_human`: `prepare_upgrade` under the read lock first, so a `control` or `mcp` port gets PRIN-010's `upgrade_refused/channel` whatever the host's provider; then, if there is no provider, refuse (REQ-008); otherwise `confirm` with no lock, then, if the bus has an upgrade journal, `commit_upgrade` under the write lock and keep the grant in the channel; if it has none, keep the `ConfirmedUpgrade` as pending (REQ-009). `attach` commits a pending upgrade first (REQ-010) and switches buses only after that commit succeeds. A port that already holds a grant moves only to a bus with the same upgrade journal `id` (REQ-015); a grant is never carried to another journal, and the person must confirm again in a new session there. PRIN's generation check still applies at commit, so an upgrade that raced with another one, or a session that ended meanwhile, fails with `conflict/head_moved`.
- **Stage report.** `fid-bus` adds a crate-private run of the pipeline that returns the `Stage` it failed in, with `Bus::execute` unchanged. Errors raised by derivation or by the upgrade steps report `session`.
- **Why no `gui` port.** A `gui` channel derives `human` with no upgrade (REQ-CORE.BUS.PRIN-005). Making it impossible to open one through a port means that no code which receives a port, and no composer, can hand a script a session that starts as `human`.
- **Errors:** every bus error unchanged; `upgrade_refused/declined` with detail `no_presence_provider`; `invalid_api_error` cannot occur, because every bus code and reason fits `ApiError`, and a test checks this over `bus-errors.json`.
- **Determinism:** N, because of presence prompts, clock reads and locks. Calls keep the determinism of their command.
- **Performance budget:** < 20 µs per call over `Bus::execute` [unverified until measured].
- **Crate:** `fid-bus` (module `port`), which gains a dependency on `fid-bus-api` (layer `model`, below `engine`).

## Test fixtures

Reuses `fixtures/core/bus/principal/presence.json` (scripted presence outcomes) and the `fid-bus` `test-support` command set; the echo-principal and failing-handler test commands are defined in the test file. Fake presence providers, a blocking provider and failing journals are test code under `crates/fid-bus/tests/`.

## Build prompt

> In crate `fid-bus`, add module `port` and a dependency on `fid-bus-api`. Implement `SessionHost::new(os_user: OsUser, presence: Option<Arc<dyn HumanPresence + Send + Sync>>, clock: Box<dyn UpgradeClock + Send>)` holding the `SessionTable` behind an `RwLock`; `LocalBus::new(bus: Bus, parts: Arc<dyn CallParts>, upgrades: Option<UpgradeJournal>)`; and `LocalPort`, opened by `SessionHost::open(kind: PortKind, label, bus)` for `PortKind::{Cli, Python, Control, Mcp}` only (there is no way to open `gui`, `internal`, `device`, `service` or server channels). The port's channel takes its session key from `open_session`, carries `Identity { subject: os_user, os_user }` from the host, and holds its `UpgradeGrant` privately; nothing returns, clones or accepts the key, the grant or a pending upgrade. Implement `fid_bus_api::BusPort`: `channel()`; `principal_kind()` = `derive_principal` now; `call()` = one `Bus::execute` with command, params and dry run from the `BusCall` and every other invocation field empty; `confirm_human()` = `prepare_upgrade` (read lock; its channel check refuses `control` and `mcp` ports with `upgrade_refused/channel`), then refuse `upgrade_refused/declined` with detail `no_presence_provider` when the host has no provider, otherwise `confirm` (no lock) and `commit_upgrade` (write lock) on the bus's upgrade journal, or keep the confirmation pending when the bus has none. `attach(bus)` commits a pending upgrade first; on failure drop it, return the error and stay `script`; a port holding a grant attaches only to a bus with the same `UpgradeJournal::id`, otherwise `precondition/false` (`upgrade recorded on another journal`). Every failure becomes an `ApiError` with the bus code, reason and detail and the failing `Stage` (add a crate-private pipeline run that reports it), or `session` outside the pipeline. Drop and `close` call `end_session`. Read nothing from the environment, argv, files or params. Tests: AC-1..AC-10 of spec node CORE.BUS.PORT with `fixtures/core/bus/principal/presence.json`, plus a test that every code and reason of `bus-errors.json` builds an `ApiError`. Constraints: CONSTITUTION §3 (principals; D-012 and D-035 by ID), §8 (no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`), `BTreeMap` only.

## Open questions

- Cancelling a call that is running inside `Bus::execute` (Ctrl+C from `AUTO.CLI.JOB`) needs a cancel handle into the handler's `HandlerCtx`. The port does not offer one yet; long-running commands go to `CORE.JOB`, which has its own cancel.
- A control-channel server could open `control` ports for its clients, but REQ-CORE.BUS.PRIN-010 refuses upgrades on `control` channels, while `AUTO.CLI.TARGET.APP` expects the app to confirm a human for a CLI client. Which channel kind an app-side CLI session uses is for `AUTO.CTRL.EXEC` and `CORE.BUS.PRIN`.
- The write lock taken by `open` waits for running calls. If a process serves many sessions (the app's control-channel server), the session table may need finer locking.

## Children

_Leaf: none._
