---
id: CORE.BUS.ROUTE
title: Branch routing, optimistic concurrency and idempotency
level: 3
parent: CORE.BUS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.BUS-006, REQ-CORE.BUS-009, REQ-CORE.BUS-010]
covers: []
depends_on: [CORE.JRN.CHAIN, CORE.JRN.BR, CORE.BUS.PRIN, CORE.BUS.PIPE.ERR, CORE.STO.SQL]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.BUS.ROUTE: Branch routing, optimistic concurrency and idempotency

## Purpose

The route stage of the pipeline. ROUTE decides which branch a commit lands on (agent work goes to the agent's own branch), checks the caller's `expected_head` against that branch's head, serialises writers so each branch has one writer at a time, and deduplicates retries by idempotency key. Branch storage and promotion stay in the journal; ROUTE only chooses and guards.

## Scope

**In:** branch selection (session working branch, agent branch `agent/<client>/<n>`, recorded co-editing opt-in); `expected_head` check; one writer per branch; the idempotency table (store, lookup, mismatch, retention).
**Out:** creating, diffing and promoting branches (`CORE.JRN.BR.NAME`, `CORE.JRN.BR.PROMO`); principal derivation and session upgrade (`CORE.BUS.PRIN`); the MCP session that names the agent (`AUTO.MCP.SESS`); the database tables (`CORE.STO.SQL`).

## Requirements

- **REQ-CORE.BUS.ROUTE-001** When an `agent` principal commits a W1 or W2 command, the bus shall route it to the agent branch `agent/<client>/<n>` of that agent session, where `<client>` is the session's configured agent name reduced to `[a-z0-9-]{1,32}` and `<n>` the next unused positive integer for that client, creating the branch from the session's working head on first use.
- **REQ-CORE.BUS.ROUTE-002** The bus shall not route an agent commit to a non-agent branch unless a `human` has recorded an opt-in for that branch and agent session, and each such entry shall carry the flag `agent_coedited`.
- **REQ-CORE.BUS.ROUTE-003** When a non-agent principal commits, the bus shall route the commit to the branch the session has open.
- **REQ-CORE.BUS.ROUTE-004** If a command's `expected_head` differs from the target branch head, then the bus shall return `conflict` carrying the current head and commit nothing.
- **REQ-CORE.BUS.ROUTE-005** While a commit to a branch is in progress, the bus shall not start another commit to the same branch, and waiting writers shall be served in arrival order.
- **REQ-CORE.BUS.ROUTE-006** When a command carries an idempotency key already committed on the same branch with the same arguments digest, the bus shall return the original result and entry hash without committing again.
- **REQ-CORE.BUS.ROUTE-007** If an idempotency key is reused on the same branch with a different arguments digest, then the bus shall return `idempotency_mismatch` and commit nothing.
- **REQ-CORE.BUS.ROUTE-008** The bus shall keep idempotency records for as long as their branch exists, and the record shall be written in the same transaction as the entry it refers to.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/bus/route/project` (working branch `main` with 3 entries) and an MCP agent session named `Claude Code`, when the agent commits a W2 test command, then branch `agent/claude-code/1` exists, forked from `main`'s head, and holds the entry; `main` is unchanged. A second session of the same agent gets `agent/claude-code/2`.
- **AC-2** Given a human opt-in recorded for `main` and that agent session, when the agent commits, then the entry is on `main` with `agent_coedited: true`.
- **AC-3** Given a CLI session upgraded to human (D-012), when it commits, then the entry lands on the branch the session has open.
- **AC-4** (error path) Given `expected_head` set to an older hash, then `conflict` with the current head, and nothing is journaled.
- **AC-5** Given 8 threads committing 1,000 W2 test commands each to one branch, then the branch holds exactly 8,000 new entries with gap-free sequence numbers and no interleaving error.
- **AC-6** Given a command with key `k1` committed once, when it is retried with the same arguments, then the original entry hash is returned and the entry count is unchanged; (error path) with different arguments, then `idempotency_mismatch`.
- **AC-7** (error path) Given a crash injected after the entry append but before return, when the project reopens and the retry arrives, then the original result is returned (record and entry were atomic).

## Design

- **Registry command(s):** none. The co-editing opt-in is recorded by `journal.set_branch_coediting@1` (W2, `human_only`), owned by `CORE.JRN.BR.NAME`; ROUTE only reads it.
- **Data structures:** `BranchRef`; `RouteTarget { branch, coedited: bool }`; table `idempotency(branch, key, args_digest, entry_hash, result_digest)` with primary key `(branch, key)`; per-branch `Mutex` with a FIFO wait queue.
- **Algorithm:** `target(principal, session)`: agent → look up the session's agent branch or allocate `<client>/<n>` (max existing n + 1) and create it through the branch API; otherwise the session's branch; check opt-in for agent-to-non-agent. `guard(branch, expected_head)`: lock, compare head, look up idempotency. Insert the idempotency row in the append transaction.
- **Errors:** `conflict`, `idempotency_mismatch`.
- **Determinism:** D0; branch numbering derives from journal state only.
- **Performance budget:** lock plus lookup < 20 µs uncontended [unverified until measured].
- **Crate:** `fid-bus` (module `route`).

## Test fixtures

`fixtures/core/bus/route/project`: a synthetic project with a 3-entry `main` branch, built by a committed generator script; CC-BY-4.0.

## Build prompt

> In crate `fid-bus`, add module `route` with `target(principal, session) -> RouteTarget` and `guard(target, expected_head, idem_key, args_digest) -> Result<Guard, BusError>`. Agent W1/W2 commits go to `agent/<client>/<n>`: `<client>` is the session's configured agent name lower-cased and reduced to `[a-z0-9-]{1,32}`, `<n>` is the next unused integer for that client; create the branch from the session's working head through the journal branch API on first use. Agents reach a non-agent branch only with a recorded human opt-in, and such entries carry `agent_coedited`. Other principals commit to the session's open branch. Hold one FIFO-fair writer lock per branch for the whole commit; compare `expected_head` (`conflict` with the current head); look up `(branch, key)` in the idempotency table: same args digest returns the original result, different gives `idempotency_mismatch`. Write the idempotency row in the same database transaction as the entry. Tests: AC-1..AC-7 with `fixtures/core/bus/route/project`, including a crash-injection test. Constraints: CONSTITUTION §3 (agents write to an agent branch), C5, §8 (no panic, no unsafe, deny unwrap/expect).

## Open questions

- **Resolved here:** `<client>` comes from the agent name bound by `AUTO.MCP.SESS` (REQ-AUTO.MCP.SESS-001), not from self-asserted client data, and `<n>` is per client. Confirm with `AUTO.MCP.SESS`.
- **Part 2 unblocked (2026-11-21, `CORE.JRN.ENTRY` REQ-013/REQ-014, maintainer decision).** The entry header now records `agent_coedited` (REQ-002, AC-2) and the stage that added each event (`command.event_sources`, so a `BranchCreated` the route stage adds is recorded `bus_route` and folds through `Branches::apply`). The pipeline passes the real source of every event (`handler`, and `bus_approval` for an approval's `RequestApproved`) through `CommandDraft.event_sources`; the route stage sets `EntryDraft.agent_coedited` and appends `bus_route` for the events it adds. Both are written only as schema version 2, so ordinary entries stay version 1.
- Whether an agent's undo on its own branch should be W1 rather than W2 (raised by `CORE.JRN.UNDO`) does not change routing: both classes go to the agent branch.
- **Build status (2026-11-21, part 1 of 2; `built_in` not set).** The maintainer split this leaf. Part 1 is built in `fid-bus` module `route` and wired into the pipeline's route stage (`crates/fid-bus/src/route.rs`, `crates/fid-bus/src/pipeline/mod.rs`); tests in `crates/fid-bus/tests/route.rs`, outcomes hand-labelled from this spec, commands from the CORE.BUS.PIPE.STAGE probe registry.
  - **Covered:** AC-3 (REQ-003: `SessionRouter` routes every non-agent principal to the branch its session has open, the working branch until the session opens another; sessions are keyed by their bus-issued key, never by client data), AC-4 (REQ-004: `check_expected_head` returns a `HeadConflict` carrying the current head; as a bus error it is `conflict/expected_head_mismatch` whose detail names the current head; nothing is journaled), AC-5 (REQ-005: one FIFO ticket lock per branch, `BranchLocks`, held from the route stage through the handler and the commit; 8 threads x 1,000 W2 commits give 8,000 entries with a verifying, gap-free chain).
  - **Lock order (as built):** branch ticket, then the bus-wide commit lock (kept bus-wide around the compare-and-swap and the hook install, because hooks are bus-wide), then hook and journal locks. At most one ticket per call; no ticket is taken under the commit lock; the internal mutexes of `BranchLocks` and `SessionRouter` are leaf locks. Tested: a held branch does not block another branch's commit, a second writer of the held branch does not reach its handler, and writers on several branches never deadlock.
  - **Agents fail closed:** `SessionRouter` refuses an agent's commit-class call with `policy/agent_write_forbidden`; the pipeline's own `agent/` check stays as a backstop. An idempotency key is still refused with `validation/schema`.
  - **Open ACs (part 2, waiting on upstream changes):** AC-1 (REQ-001: agent branches `agent/<client>/<n>` and the event source that creates them through the branch API), AC-2 (REQ-002: the co-editing opt-in and `agent_coedited`; `RouteTarget::coedited` is always `false`), AC-6 and AC-7 (REQ-006..008: the idempotency table, which needs the v2 store schema and a side row written in the same transaction as the entry), and the agent name bound by `AUTO.MCP.SESS` for `<client>`. The fixture `fixtures/core/bus/route/project` comes with AC-1 in part 2.
  - **Deviation:** the bus error envelope is `{code, reason, detail}` only, so at the `Bus::execute` boundary the current head travels in the detail text (`... current head sha256:...`), which surfaces must not parse. A structured `current_head` field on the conflict error is a catalogue change for `CORE.BUS.PIPE.ERR`.

## Children

_Leaf: none._
