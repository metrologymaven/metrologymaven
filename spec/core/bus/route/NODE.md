---
id: CORE.BUS.ROUTE
title: Branch routing, optimistic concurrency and idempotency
level: 3
parent: CORE.BUS
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.BUS-006, REQ-CORE.BUS-009, REQ-CORE.BUS-010]
covers: []
depends_on: [CORE.JRN, CORE.BUS.PRIN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.BUS.ROUTE: Branch routing, optimistic concurrency and idempotency

## Purpose

The route stage of the pipeline. ROUTE decides which branch a commit lands on (agent work goes to the agent's own branch), checks the caller's `expected_head` against that branch's head, serialises writers so each branch has one writer at a time, and deduplicates retries by idempotency key. Branch storage and promotion stay in the journal; ROUTE only chooses and guards.

## Scope

**In:**
- Branch selection: human working branch, agent branch `agent/<client>/<n>`, opt-in co-editing
- `expected_head` check and typed conflict error
- One writer per branch (serialisation of concurrent commits)
- Idempotency keys: store, lookup, mismatch detection, retention

**Out** (and which node owns it instead):
- Creating, diffing and promoting branches (owned by `CORE.JRN.BR`)
- Principal derivation (owned by `CORE.BUS.PRIN`)

## Requirements

- **REQ-CORE.BUS.ROUTE-001** When an agent principal commits a W1 or W2 command, the bus shall route it to the agent branch `agent/<client>/<n>` for that agent session, creating the branch from the current working head on first use.
- **REQ-CORE.BUS.ROUTE-002** The bus shall not route an agent commit to a human's working branch unless a human has recorded an opt-in for that branch, and each such entry shall be flagged as agent co-edited.
- **REQ-CORE.BUS.ROUTE-003** If a command's `expected_head` differs from the target branch head, then the bus shall return a typed `conflict` error carrying the current head and commit nothing.
- **REQ-CORE.BUS.ROUTE-004** While a commit to a branch is in progress, the bus shall not start another commit to the same branch.
- **REQ-CORE.BUS.ROUTE-005** When a command carries an idempotency key already committed on the same branch with the same arguments digest, the bus shall return the original result without committing again.
- **REQ-CORE.BUS.ROUTE-006** If an idempotency key is reused on the same branch with a different arguments digest, then the bus shall return a typed `idempotency_mismatch` error.
- **REQ-CORE.BUS.ROUTE-007** The bus shall keep idempotency records for at least as long as the branch exists.

## Interfaces

- **Offers:** `Route::target(principal, channel, def) -> BranchRef`; `Route::guard(branch, expected_head)`; idempotency table (stored through `CORE.JRN` in the project database).
- **Consumes:** `CORE.JRN` (heads, branch creation), `CORE.BUS.PRIN`.

## Decomposition hints

TARGET (branch selection and opt-in), CONC (expected_head and writer lock), IDEM (idempotency table).

## Open questions

- How `<client>` and `<n>` are assigned for an agent session (per MCP connection, per client ID) needs agreement with `AUTO.MCP`.

## Children

_None yet: status `open`._
