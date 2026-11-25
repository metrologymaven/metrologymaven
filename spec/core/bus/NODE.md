---
id: CORE.BUS
title: Command bus: execution, principals and side-effect enforcement
level: 2
parent: CORE
kind: subsystem
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-CORE-002, REQ-CORE-004, REQ-CORE-009]
covers: []
depends_on: [CORE.REG, CORE.JRN, CORE.JOB]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.BUS: Command bus

## Purpose

The only path by which any surface changes state. The bus takes a command ID, parameters and a channel, derives the principal from the channel, validates parameters against the registry schema, checks permissions, side-effect class rules and preconditions, routes the commit to the right branch, runs the handler against a read-only state view, and hands the resulting events to the journal as one atomic commit. It is separate from the registry (which only describes commands) and the journal (which only records them) because it is where policy is enforced.

## Scope

**In:**
- `Bus::execute(command, params, channel)` pipeline: resolve → validate → authorise → check preconditions → route → run handler → commit → return result
- Principal derivation from the authenticated channel; principal kinds `human | agent | script | service | device | system`
- Side-effect class enforcement per CONSTITUTION §3, including the bans on agent M1, S1, W3 and A0
- Branch routing: agent-principal W1 and W2 commits go to that agent's branch
- File-system effect enforcement: agents pass handles to approved import and export roots, never raw paths
- Egress (X1) gating, so no data leaves the machine without a request the user approves
- Handler contract: handlers read an immutable state view and return events; they never write state
- Effects check: an event not declared by the command aborts the commit
- Idempotency keys and retry deduplication
- Optimistic concurrency: `expected_head` and typed conflict errors; one writer per branch
- Typed errors shared by all surfaces
- Unit normalisation of authored literals at the boundary (value and unit recorded as entered; one journaled rounding)
- Hand-off of long-running commands to the job system

**Out** (and which node owns it instead):
- Command definitions and the manifest (owned by `CORE.REG`)
- Hash chain, branches, undo and promotion (owned by `CORE.JRN`)
- Job scheduling, progress and cancellation (owned by `CORE.JOB`)
- Channel authentication protocols themselves: CLI, MCP, Python, control channel (owned by `AUTO`)
- Signature ceremonies and keys (owned by `TRUST`)
- Motion approval (owned by `CMM`)

## Requirements

- **REQ-CORE.BUS-001** When a command is invoked, the bus shall take the principal kind and identity from the authenticated channel and shall ignore any principal fields in the request parameters.
- **REQ-CORE.BUS-002** When a command is invoked through the CLI, Python or a headless caller, the bus shall assign principal kind `script`, not `human`.
- **REQ-CORE.BUS-003** If parameters fail validation against the command's registry schema (type, quantity kind, unit or reference kind), then the bus shall reject the call with a typed validation error and shall commit nothing.
- **REQ-CORE.BUS-004** If an agent principal invokes an M1, S1, W3 or A0 command, then the bus shall reject the call with a typed policy error and shall commit nothing.
- **REQ-CORE.BUS-005** If a command whose MCP projection is `request_only` is invoked through an MCP channel, then the bus shall create a pending request record for a human and shall not execute the command.
- **REQ-CORE.BUS-006** When an agent principal commits a W1 or W2 command, the bus shall route the commit to that agent's branch and never to a human's working branch unless the human has opted in for that branch.
- **REQ-CORE.BUS-007** If a command's precondition evaluates false, then the bus shall reject the call with a typed precondition error naming the failed predicate.
- **REQ-CORE.BUS-008** If a handler emits an event type not declared in the command's effects, then the bus shall abort the commit and record nothing.
- **REQ-CORE.BUS-009** When a command carries an idempotency key already committed on the same branch, the bus shall return the original result without committing again.
- **REQ-CORE.BUS-010** If a command's `expected_head` differs from the branch head, then the bus shall reject it with a typed conflict error and shall commit nothing.
- **REQ-CORE.BUS-011** If an agent principal passes a raw file-system path, then the bus shall reject the call; only handles to approved roots shall be accepted.
- **REQ-CORE.BUS-012** If an X1 command is invoked by any principal without a recorded human approval, then the bus shall not execute it.
- **REQ-CORE.BUS-013** The bus shall not open any network connection on its own initiative; network use shall happen only inside an executed X1, I1 or D1 command.
- **REQ-CORE.BUS-014** When an authored literal with a unit is normalised to SI, the bus shall record the literal as entered and perform exactly one rounding.
- **REQ-CORE.BUS-015** The bus shall add no more than 1 ms median overhead per R0 command on reference hardware [unverified until measured].
- **REQ-CORE.BUS-016** Every state-changing path in the product shall pass through the bus; a crate-graph check shall fail the build if any crate other than the bus and storage crates can reach mutable project state.

## Interfaces

- **Offers:** `Bus::execute(command_id@major, params, channel) -> Result<CommandResult, BusError>`; `BusError` (validation, policy, precondition, conflict, not_found, cancelled); the principal model; the pending-request record for request-only commands.
- **Consumes:** `CORE.REG` (definitions and schemas), `CORE.JRN` (atomic append, head lookup, branch resolution), `CORE.JOB` (submit long-running commands), `CORE.STO` via JRN only.
- **Commands:** none of its own; it executes every other domain's commands.

## Decomposition hints

PIPE (execute pipeline and typed errors), PRIN (principal model and channel binding), POL (class, projection, fs-effect and egress policy), ROUTE (branch routing and concurrency), IDEM (idempotency), UNIT (literal normalisation), LINT (no-side-doors crate-graph check).

## Open questions

- Should the bus also write the local Activity Log for R0/R1 calls (concept doc 03 §4.1), or does `CORE.JRN` own that stream? This split gives the stream to `CORE.JRN`; the bus only calls it.
- Pending-request records for `request_only` commands overlap with the VIZ Proposal Inbox; VIZ renders them, BUS creates them.

## Split rationale

The dividing principle is **one child per pipeline stage concern**, with the pipeline itself as the integrating child: PIPE owns stage order, validation, preconditions, handler purity, effects and errors; PRIN owns *who* (principal derivation); POL owns *may they* (class, projection, file handles, egress, network); ROUTE owns *where and whether again* (branch, concurrency, idempotency); UNIT owns literal normalisation; LINT owns the structural build-time proof that no other path to state exists. UNIT and LINT are small and fully specified, so they are leaves. PRIN is written to leaf depth but is blocked on D-012 (principal kind of a human at the CLI), raised concurrently by AUTO.

Alternatives considered:
- **IDEM as its own child** (as the hints suggest). Rejected: idempotency lookup is keyed by branch and shares the writer lock with concurrency checks; splitting it would force two children to own one table.
- **Validation inside POL.** Rejected: schema validation is mechanical and belongs with the pipeline; POL stays purely about authorisation.
- **Pending requests in JRN.** Rejected: the decision to create one is policy; JRN only stores the resulting W1 entry.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.BUS.PIPE | Execute pipeline, handler contract and typed errors | 003 007 008 015 | none |
| CORE.BUS.PRIN | Principal model and channel binding (decision D-012) | 001 002 | none |
| CORE.BUS.POL | Side-effect, projection, file-handle and egress policy | 004 005 011 012 013 | none |
| CORE.BUS.ROUTE | Branch routing, optimistic concurrency and idempotency | 006 009 010 | none |
| CORE.BUS.UNIT | Authored literal normalisation to SI (leaf) | 014 | none |
| CORE.BUS.LINT | No-side-doors crate-graph check (leaf) | 016 | none |
| CORE.BUS.API | Bus API crate: the call types and port trait surfaces see (leaf) | 001 016 | none |
| CORE.BUS.PORT | In-process bus port: sessions, human upgrade and calls (leaf) | 001 002 | none |
