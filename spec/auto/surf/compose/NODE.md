---
id: AUTO.SURF.COMPOSE
title: Composition root: handlers, project buses and the fid entry point
level: 3
parent: AUTO.SURF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-compose]
built_pr: 0
satisfies: [REQ-AUTO.SURF-008]
covers: []
depends_on: [CORE.BUS.PORT, AUTO.SURF.INPROC, CORE.REG.MAN, CORE.STO.LAYOUT, CORE.BUS.POL.FS, CORE.PRJ.TREE.QUERY, AUTO.CLI.DISPATCH]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-25
---

# AUTO.SURF.COMPOSE: Composition root: handlers, project buses and the fid entry point

## Purpose

The one place that links the bus, the storage layer, every domain's handlers and the surfaces, and wires them together. It builds a `Bus` for one open project, with the command table of the manifest in force and every handler the domains have, plus a `Bus` for a process with no project open. It opens the project with the storage lock and fixes the OS identity of the process's session. It installs the only presence provider the session can be confirmed with, and hands the surface a ready `InProcTransport` (`AUTO.SURF.INPROC`) over a `LocalPort` (`CORE.BUS.PORT`). It runs in the process that owns the project: the `fid` process for the headless and no-project targets, and the desktop app process for a project the app has open. Surfaces never compose a bus, so `fid-cli` and `fid-mcp` keep linking no CORE crate but the bus API (REQ-CORE.BUS.LINT-004). For that reason the `fid` executable is built from this crate, not from `fid-cli`.

## Scope

**In:**
- `HANDLER_SOURCES`: one entry per `fid_registry_build::SOURCES` crate that registers that crate's handlers for an open project, and optionally for no project
- `UNWIRED`: the pinned list of manifest command majors that have no handler yet, each with a reason, and their stub handlers; the coverage test
- Composing a project bus and the no-project bus as `LocalBus` values, including the no-project Record Journal that refuses every append
- Opening a project read-only or writable with the lock through `CORE.STO.LAYOUT`, resolving `root:` handles through `CORE.BUS.POL.FS`, the raw-path rule for opening, and releasing the lock
- The process's OS user, its `SessionHost`, `PRESENCE_PROVIDERS` and the check that no other shipped type implements `HumanPresence`
- `InProcessHost`: building a port, `confirm_human`, `open_project` and handing out the transport
- The `fid` executable's entry point (bin target `fid`), which moves here from `fid-cli`

**Out:**
- Sessions, upgrades and calls (`CORE.BUS.PORT`); transport events and envelopes (`AUTO.SURF.INPROC`)
- Choosing the target from `--project`, `--app` and `FID_PROJECT`, the exit codes for lock conflicts and failed confirmations, and the `fid-cli` seam that receives transports (`AUTO.CLI.TARGET.SELECT`, `AUTO.CLI.TARGET.HEADLESS`)
- The handlers themselves and the per-domain data they read (their domain nodes)
- OS presence checks and the in-app prompt (`TRUST.SIGN.UV`, `PLAT`, desktop apps); the control-channel server's sessions (`AUTO.CTRL.EXEC`)
- The desktop app's own entry point, which calls the same composition functions (desktop app nodes)
- The crate-graph check (`CORE.BUS.LINT`)

## Requirements

- **REQ-AUTO.SURF.COMPOSE-001** The composition root shall hold exactly one `HANDLER_SOURCES` entry for each entry of `fid_registry_build::SOURCES`, naming the same crate, in the same order.
- **REQ-AUTO.SURF.COMPOSE-002** When a project bus is composed, the composition root shall build it from the command table of the manifest in force over `fid_registry_build::SOURCES` and every handler that the `HANDLER_SOURCES` entries register for that project.
- **REQ-AUTO.SURF.COMPOSE-003** The composition root shall register, for every command major in `UNWIRED`, a stub handler that fails with bus error `internal` reason `invariant_violated` and detail `handler not wired: <name@major>`.
- **REQ-AUTO.SURF.COMPOSE-004** If a command major of the manifest in force has neither a registered handler nor an `UNWIRED` entry, or an `UNWIRED` entry names a major that has a handler or is not in the manifest, then the coverage test shall fail naming that major.
- **REQ-AUTO.SURF.COMPOSE-005** When the no-project bus is composed, the composition root shall register the handlers that `HANDLER_SOURCES` entries register without a project and a stub for every other manifest major that fails with `precondition` reason `false` and detail `no project open`.
- **REQ-AUTO.SURF.COMPOSE-006** The composition root shall give the no-project bus a Record Journal that refuses every append with `precondition` reason `false`, and no upgrade journal, so that nothing commits and no upgrade is recorded while no project is open.
- **REQ-AUTO.SURF.COMPOSE-007** The composition root shall take the OS user of every session from the process's OS account (the user of the Windows process token, or the account of the POSIX real user ID) and never from environment variables, argv, files or parameters.
- **REQ-AUTO.SURF.COMPOSE-008** The composition root shall give `SessionHost::new` only a presence provider listed in `PRESENCE_PROVIDERS` for the running platform, and no provider when the list has no entry for it.
- **REQ-AUTO.SURF.COMPOSE-009** If a type not listed in `PRESENCE_PROVIDERS` implements `HumanPresence` in shipped code of any workspace crate (anything outside `tests/`, `benches/`, `examples/` and `#[cfg(test)]` items), then the presence-provider test shall fail naming the type and the file.
- **REQ-AUTO.SURF.COMPOSE-010** When asked to open a project by `root:` handle, the composition root shall resolve the handle through the approved roots of `CORE.BUS.POL.FS` and open nothing if it does not resolve.
- **REQ-AUTO.SURF.COMPOSE-011** If asked to open a project by raw path while the port's principal kind is not `human` and the port holds no pending person-confirmed upgrade, then the composition root shall refuse with `raw_path_refused` and open nothing.
- **REQ-AUTO.SURF.COMPOSE-012** The composition root shall open a project in the mode its caller names: read-only through `CORE.STO.LAYOUT` without the write lock, or writable with it.
- **REQ-AUTO.SURF.COMPOSE-013** When the transport that holds an open project is dropped, the composition root shall close the port first and then release the project and its lock.
- **REQ-AUTO.SURF.COMPOSE-014** When a project bus is composed for a port, the composition root shall attach the port to it before handing out the transport, so that a pending upgrade is committed before the first call (REQ-CORE.BUS.PORT-010).
- **REQ-AUTO.SURF.COMPOSE-015** The `fid` executable shall be built only as the bin target `fid` of `fid-compose`, never as a bin target of a crate listed under `[surfaces]` in `lint-allowlist.toml`; other bins of surface crates (such as `fid-mcp`'s `fid-mcp`) are not affected.
- **REQ-AUTO.SURF.COMPOSE-016** When the `fid` process starts, the composition root shall open exactly one port, of kind `mcp` for `fid mcp` and of kind `cli` otherwise, labelled `fid/<process id>`.

## Acceptance criteria

- **AC-1** Given the workspace, when the source test runs, then `HANDLER_SOURCES` and `fid_registry_build::SOURCES` name the same crates in the same order.
- **AC-2** Given the workspace manifest, when the coverage test runs, then every manifest command major has exactly one handler or one `UNWIRED` entry and no `UNWIRED` entry has a handler. (error path) Given a test-only source that adds `demo.unwired@1` with no handler and no `UNWIRED` entry, then the test fails naming `demo.unwired@1`; given an `UNWIRED` entry for `project.list_elements@1`, which has a handler, then it fails naming that major.
- **AC-3** Given a project created by `CORE.STO.LAYOUT` in a temporary directory and opened read-only, and a `cli` port, when the `fid_surface` tracker sends `project.list_elements@1` with the smallest parameter document its schema accepts through the `InProcTransport`, then the outcome is `computed` with a result that validates against `schema/element-list.schema.json`. When it sends a command in `UNWIRED`, then the outcome is `failed` with `internal_error`, `bus_code` `internal`, `bus_reason` `invariant_violated` and `bus_origin` `handler`. When it sends `demo.no_such@1`, then `not_found` with `bus_origin` `pipeline`.
- **AC-4** (error path) Given the no-project bus and a `cli` port, when every manifest major that no `HANDLER_SOURCES` entry registers without a project and that reaches the handler stage is sent with the smallest parameter document its schema accepts, then each finishes `failed` with `precondition_failed` and detail `no project open` (the no-project stub's result), and the temporary directory the process runs in is unchanged. Every other major (script-forbidden classes, lifecycle-hidden, removed) fails earlier, at the bus pipeline, none succeeds, and nothing is written.
- **AC-5** Given the process environment holding `USER`, `USERNAME` and `LOGNAME` set to `spoofed-user` and `FID_PRINCIPAL=human`, when a port is built, then its channel identity equals the identity built with those variables removed, is not `spoofed-user`, and its principal kind is `script`.
- **AC-6** Given `PRESENCE_PROVIDERS` with no entry for the CI platform, when `confirm_human` runs on a port from the shipped composition, then it fails with `upgrade_refused/declined` and detail `no_presence_provider`, and the principal kind stays `script`. Given the workspace, the presence-provider test passes. (error path) Given fixture tree `fixtures/auto/surf/compose/presence_scan/shipped/` holding `src/auto.rs` with `impl HumanPresence for AutoYes`, then the test fails naming `AutoYes` and that file; given `fixtures/auto/surf/compose/presence_scan/tests_only/`, where the same impl sits in `tests/` and inside a `#[cfg(test)]` module, then it passes.
- **AC-7** (error path) Given a `script` port and a raw-path project reference, when `open_project` runs, then it fails with `raw_path_refused`, the project's lock holder record is not set and nothing is opened. Given a session host built in a test with a fake provider that confirms the session's OS user, when `confirm_human` runs on the no-project bus and then `open_project` opens a raw-path project writable, then the project's Record Journal holds one `principal_upgrade` entry before the first command entry, and a W2 command then commits with principal kind `human`.
- **AC-8** Given a project opened writable, then the lock's holder record names this process's PID while the transport lives and is cleared after the transport is dropped; given it opened read-only, then the holder record stays empty. The lock file is a fixed, store-managed entry that is never deleted (REQ-CORE.STO.LAYOUT-001, REQ-CORE.STO.LAYOUT-003).
- **AC-9** Given `cargo metadata --format-version 1`, then the bin target `fid` belongs to `fid-compose` and to no other package (in particular to no `[surfaces]` crate), `layers.toml` maps `fid-compose` to `app`, and `cargo xtask layers` and `cargo xtask lint-crate-graph` exit 0. Given the moved binary, then `fid --version`, `fid --help --json` and `fid canon` produce the same bytes and exit codes as before the move (the moved `CARGO_BIN_EXE_fid` tests pass unchanged, and `fid-cli`'s remaining tests, including the source scan of `tests/tree.rs`, pass in `fid-cli`).

## Design

- **Registry command(s):** none of its own.
- **Data structures:**
  - `HandlerSource { krate: &'static str, project: fn(&mut HandlerSet, &ProjectParts) -> Result<(), BusError>, free: Option<fn(&mut HandlerSet, &FreeParts) -> Result<(), BusError>> }`; `HANDLER_SOURCES: [HandlerSource; N]` in `SOURCES` order. `ProjectParts` gives an entry what its crate's `register` function needs from one open project: the namespace, the journal, branch and tree sources, settings and file roots. Each entry builds its crate's source adapters (for example `BranchSource` for `fid-journal-cmd`) from them.
  - `UNWIRED: &[(&str, &str)]`: `(name@major, reason)` sorted by command, where the reason is `no handler in <crate>` or `no <source> adapter yet`.
  - `PRESENCE_PROVIDERS: &[(Platform, &str)]`: the shipped `HumanPresence` implementations by platform, empty until an OS presence provider exists.
  - `InProcessHost { host: Arc<SessionHost>, port: LocalPort, project: Option<ProjectGuard> }` with `for_process(kind: PortKind) -> Result<Self, HostError>`, `confirm_human(&mut self) -> Result<(), ApiError>`, `open_project(&mut self, project: ProjectRef, mode: OpenMode) -> Result<(), HostError>` and `into_transport(self) -> HostTransport`. `HostTransport` owns the `InProcTransport` and the `ProjectGuard`, and drops them in that order. `ProjectRef { Handle(String), RawPath(PathBuf) }`.
- **Where it runs:**
  - In the `fid` process, `main` calls `InProcessHost::for_process` once. For a project target it then calls `open_project`, and for `--confirm-human` it calls `confirm_human` first: before opening, the confirmation stays pending on the no-project bus and is committed by the attach of REQ-014. The resulting transport is handed to `fid-cli` through `HeadlessConnector` (`AUTO.CLI.TARGET.HEADLESS`), which the `fid` entry point implements over `InProcessHost`. The app target never composes a bus in the `fid` process: the CLI sends over the control channel to the app, whose own process runs this composition.
  - In the desktop app, the app's entry point calls the same `compose_project` with the app's presence provider, which is listed in `PRESENCE_PROVIDERS` when it is built.
- **Algorithm (project bus):** (1) `fid_registry_build::manifest_in_force()` gives the command table; (2) a `HandlerSet` gets every entry's `project` registration, then the `UNWIRED` stubs; (3) `BusParts` gets the project's journal, router (`CORE.BUS.ROUTE` `SessionRouter`), streams, environment and manifest digests; (4) `LocalBus::new` gets the bus, the per-call parts and the project's upgrade journal on the session's working branch; (5) `port.attach(bus)`; if that fails, the project is closed and the error returned.
- **First wired handlers:** at least `project.list_elements@1` and `project.describe_element@1` (`fid_project_cmd::query::register`, which needs only the project namespace). Every other major is wired where its crate already has a handler and the adapter is small, and is otherwise listed in `UNWIRED`. Removing an `UNWIRED` entry is part of the PR that adds the handler.
- **OS user:** read through a dependency admitted by `deny.toml`, or a `PLAT` probe, with no `unsafe` in this crate. A lookup failure is `unauthenticated` (REQ-CORE.BUS.PRIN-008), and the process runs nothing.
- **Presence scan:** a test walks `crates/*/src/**/*.rs`, finds `impl ... HumanPresence for <Type>` (with or without a path prefix), skips items under a `#[cfg(test)]` attribute by matching braces, and fails for any type not in `PRESENCE_PROVIDERS`. Tests may implement fake providers freely, because a test is never shipped. **Limit:** the scan reads source text, so an impl generated by a macro (`macro_rules!` or a proc macro) whose expansion contains `HumanPresence` but whose source does not is not seen. The scan therefore also fails on any macro definition in shipped code whose body names `HumanPresence`, and review covers proc macros, which are listed in `layers.toml` and few.
- **Why the binary moves:** `CORE.BUS.LINT` refuses any path from a `[surfaces]` crate to a CORE crate other than `fid-bus-api`, and this crate reaches `fid-bus` and `fid-store` directly. So no surface crate may link it, and the process entry that links both must sit here. `fid-cli` keeps all argv, help and rendering logic as a library. The `main` here only composes and hands over.
- **Errors:** `raw_path_refused`, `bad_handle`, `unauthenticated`, storage errors from `CORE.STO.LAYOUT` (including `locked` with its holder, which `AUTO.CLI.TARGET.HEADLESS` maps to `conflict`), and every bus error unchanged.
- **Determinism:** N (OS identity, locks, presence). Composition from the same workspace and project is deterministic in content.
- **Performance budget:** composing a project bus for the workspace manifest < 50 ms on reference hardware [unverified until measured].
- **Crate:** `fid-compose` (new, layer `app`), with bin target `fid`.

## Test fixtures

A project created by `CORE.STO.LAYOUT` in a temporary directory per test (no checked-in project). `fixtures/auto/surf/compose/presence_scan/shipped/` and `.../tests_only/`: two tiny source trees for the scanner (not Cargo packages), project-authored, CC-BY-4.0. Fake presence providers are test code in `crates/fid-compose/tests/`.

## Build prompt

> Create crate `crates/fid-compose` (layer `app` in `layers.toml`) linking `fid-bus`, `fid-bus-api`, `fid-store`, `fid-journal`, `fid-registry-build`, `fid-surface`, `fid-cli` and the handler crates. Add `HANDLER_SOURCES`, one entry per `fid_registry_build::SOURCES` crate in the same order, each registering that crate's handlers for an open project (and optionally for no project) from `ProjectParts`/`FreeParts`; wire at least `fid_project_cmd::query::register`; list every other manifest major without a handler in the sorted `UNWIRED` with a reason, register a stub for each that fails `internal/invariant_violated` (`handler not wired: <id>`), and add the coverage test of AC-2. Compose the no-project bus with only no-project handlers, stubs failing `precondition/false` (`no project open`) and a Record Journal refusing every append, with no upgrade journal. Implement `InProcessHost::for_process(kind)` (OS user from the OS account, never the environment; `SessionHost::new` with the provider listed in `PRESENCE_PROVIDERS` for the platform or none; one port labelled `fid/<pid>`), `confirm_human`, `open_project(ProjectRef, OpenMode)` (handles through `CORE.BUS.POL.FS`; raw paths only for a `human` port or one with a pending confirmed upgrade, else `raw_path_refused`; `ProjectLocation::open` read-only or with the lock; compose the project bus and `attach` the port) and `into_transport`, whose value drops the port before the project guard. Add the presence-provider source scan. Move `crates/fid-cli/src/main.rs` to `crates/fid-compose/src/bin/fid.rs` and remove `fid-cli`'s `[[bin]]`, keeping behaviour byte-identical. Move only the tests that run `env!("CARGO_BIN_EXE_fid")`: `tests/bin.rs`, `tests/ctrl.rs` (AUTO.CTRL.CLIENT AC-1, AC-2) and the `run_fid` tests of `tests/tree.rs` (AUTO.CLI.TREE AC-1..AC-4); the source scan (`ac5_no_hardcoded_registry_cli_paths_in_fid_cli_src`) and `embedded_tree_roundtrip_and_malformed_refusal` stay in `fid-cli/tests/tree.rs`. Every reference that breaks when the binary moves: `.github/workflows/tracim-public.yml:65` (`cargo build --release --locked -p fid-cli`) and `.github/workflows/rust.yml:305, 307, 310, 436, 519` (`cargo run -q -p fid-cli -- canon`) move to `--bin fid` in a separate CI PR that lands first (it works before and after the move); `README.md:27` (`cargo run -q -p fid-cli -- canon`) becomes `--bin fid` in this build PR; `crates/fid-ctrl/tests/client.rs:3` points at the moved `crates/fid-compose/tests/ctrl.rs`; and `spec/gdt/char/record/NODE.md:81` ("The `fid` binary (`fid-cli`)") is a spec follow-up for `GDT.CHAR.RECORD`, not edited by the build. Tests: AC-1..AC-9 of spec node AUTO.SURF.COMPOSE. Constraints: CONSTITUTION §2.1 (L3), §3 (principals; D-012 and D-035 by ID), §8 (no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`), `BTreeMap` only.

## Open questions

- Several domain crates in `SOURCES` declare commands but have no bus `Handler` yet. They expose pure functions instead (for example `fid_mig::run_list_sources`). Their CLI forms run end to end only once each has a handler, or an adapter in its `HANDLER_SOURCES` entry, and leaves `UNWIRED`. That work belongs to the domain nodes that own those commands.
- `AUTO.CLI.TARGET.HEADLESS` now defines `HeadlessConnector` for the headless target. The no-project target (REQ-AUTO.CLI.TARGET-005) needs the same kind of seam over the no-project bus, which is for `AUTO.CLI.TARGET.SELECT`. Until `fid-cli` calls these seams, the moved `main` keeps today's behaviour.
- `PRESENCE_PROVIDERS` is empty, so a real `--confirm-human` is refused everywhere until an adapter from the `TRUST.SIGN.UV` user-verification providers (or a `PLAT` presence check) to `HumanPresence` is built and listed. This fails closed.
- Only listed crates should be able to link both a `[surfaces]` crate and a CORE crate other than the bus API. Today that holds only because this is the one such crate. `CORE.BUS.LINT` could add a `[composition]` list to enforce it.

- **Build status (PR 633, with errata PR 636; PR PRNUM sets `built_in`).** Built in `fid-compose` (layer `app`): `HANDLER_SOURCES` (one entry per `SOURCES` crate; `fid-project-cmd` wires the two tree queries and the four container commands), the sorted `UNWIRED` list with stubs, the coverage check, the project bus and the no-project bus over `LocalBus`, `InProcessHost` (`for_process`, `with_host`, `confirm_human`, `open_project`, `into_transport`), `PRESENCE_PROVIDERS` (empty) and the presence scan, and the `fid` bin target moved from `fid-cli` (behaviour unchanged; CI and README call `--bin fid`). Covered: AC-1 to AC-9, plus D-035 and D-045 through the composed bus. `tests/host.rs` also checks that a second writable open is `locked` and then succeeds.
- **Raw paths and pending upgrades (review of #624).** REQ-011 is applied more strictly than written: a pending person-confirmed upgrade only lets the composition root open the raw-path project and attach the port, which records the upgrade first. The project is handed to the port only if the port is then `human` (its grant is on that project's journal); otherwise the project and lock are released and the open fails with `raw_path_refused`: a read-only open has no upgrade journal (the port moves back to the no-project bus), and a failed append (for example `time_regressed`) leaves the port on the no-project bus, with the journal's error kept in the detail. If the port could not move back, it would be closed rather than left attached.
- **OS user.** The OS user is the process account's identifier, not its name: `uid:<real uid>` on POSIX (rustix, no `unsafe`) and the token user's SID on Windows (`fid-ctrl-acl`, the D-048 exception). A `PLAT` probe for account names can replace it; presence providers must then report the same spelling.
- **Project state.** The call view folds `ContainerCreated` events of the working branch (at open from the journal, then through a commit hook) over an element tree with no elements. A project whose working branch holds element-tree or stage events is refused at open (replay not built), so the stage and tree-group commands stay in `UNWIRED`. The namespace is the identity record's when the branch holds `ProjectIdentified`, else the `CORE.STO.LAYOUT` UUID.
- **Kernel manifest digest.** Until a kernel manifest is in force, entries record `NO_KERNEL_MANIFEST`, the SHA-256 of the empty string. `CORE.JRN` must reserve this value as "no kernel attested", so no reader takes it for an attested kernel (spec change pending, owned by `CORE.JRN`).
- **Presence scan and renames.** Besides `impl ... HumanPresence for T` and macros naming the trait, the scan reports every `HumanPresence as <alias>` (import or re-export) in shipped code, since `impl <alias> for T` would otherwise pass unseen.
- **REQ-016 in `main`.** `for_process`, `port_kind_for` and `process_label` implement it, but the moved `main` does not open a port yet: it keeps today's behaviour until `AUTO.CLI.TARGET.SELECT` and `AUTO.CLI.TARGET.HEADLESS` give `fid-cli` a transport seam (see above).
- **CI references.** The `--bin fid` workflow edits planned for a separate CI PR are in the build PR, since `-p fid-cli` stops building a binary in the same change. `fid-cli`'s AUTO.CLI.TREE AC-3 stays in `fid-cli` (it does not run the binary); AC-1, AC-2 and AC-4 moved.

## Change log

- 2026-11-25 (maintainer decision, errata from PR 633): AC-4 is narrowed to the majors that reach the handler stage, which return the no-project stub; every other major (script-forbidden classes, hidden, removed) fails at the bus pipeline and nothing is written. AC-8 and AC-7 no longer say a lock file is created or deleted: the lock is a fixed, store-managed entry that is never deleted (REQ-CORE.STO.LAYOUT-001, -003), and its holder record names this PID while the transport lives and is cleared after, and stays empty for a read-only open. No REQ changed.

## Children

_Leaf: none._
