---
id: CORE.BUS.POL
title: Side-effect, projection, file-handle and egress policy
level: 3
parent: CORE.BUS
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.BUS-004, REQ-CORE.BUS-005, REQ-CORE.BUS-011, REQ-CORE.BUS-012, REQ-CORE.BUS-013]
covers: []
depends_on: [CORE.REG, CORE.JRN, CORE.BUS.PRIN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.BUS.POL: Side-effect, projection, file-handle and egress policy

## Purpose

The authorise stage of the pipeline. POL decides, from the principal, the channel and the command's registry policy fields, whether a call may run now, must become a pending request for a human, or is refused. It enforces CONSTITUTION §3 and §3.1: the agent bans on M1, S1, W3 and A0; request-only projection; handles instead of raw paths; human approval before egress; and no network use outside X1, I1 and D1 commands.

## Scope

**In:**
- Class and principal checks (agent bans; human-only classes)
- MCP projection enforcement on MCP channels (`exposed`, `request_only`, `never`) as defence in depth behind the generated surfaces
- Pending-request records for `request_only` commands, and their approval or rejection by a human
- File handles: approved import and export roots, canonicalisation, fixed extensions, no overwrite; agents never pass raw paths
- X1 egress gating on a recorded human approval
- Network guard: no network use by the bus itself; network only inside executed X1, I1 or D1 commands
- Job-cancel authorisation per §3.1 (own jobs W1; another principal's job needs a human)

**Out** (and which node owns it instead):
- Approval Station and motion release rules (owned by `CMM`)
- Rendering pending requests in the Proposal Inbox (owned by `VIZ`)
- Configuring approved roots (an A0 setting declared by `CORE.STO`)
- Signing ceremonies (owned by `TRUST`)

## Requirements

- **REQ-CORE.BUS.POL-001** If an agent principal invokes an M1, S1, W3 or A0 command, then the bus shall return a typed `policy` error and commit nothing.
- **REQ-CORE.BUS.POL-002** If a command whose projection is `request_only` is invoked through an MCP channel, then the bus shall record a pending request in the Record Journal as a W1 entry, return its request ID, and not execute the command.
- **REQ-CORE.BUS.POL-003** If a command whose projection is `never` is invoked through an MCP channel, then the bus shall return a typed `policy` error.
- **REQ-CORE.BUS.POL-004** When a human principal approves a pending request, the bus shall execute the command with the human as principal and record the requesting principal and request ID in the entry.
- **REQ-CORE.BUS.POL-005** If an agent principal passes a raw file-system path in any parameter, then the bus shall return a typed `policy` error; only handles to approved roots shall be accepted.
- **REQ-CORE.BUS.POL-006** When a handle is resolved, the bus shall canonicalise the path, reject any result outside the approved root, reject extensions not allowed for the command, and reject overwriting an existing file for `write_root` effects.
- **REQ-CORE.BUS.POL-007** If an X1 command is invoked without a recorded human approval for that invocation, then the bus shall not execute it.
- **REQ-CORE.BUS.POL-008** The bus shall open no network connection except inside the execution of an X1, I1 or D1 command.
- **REQ-CORE.BUS.POL-009** If a principal other than the job's own principal or a human invokes job cancellation, then the bus shall return a typed `policy` error.

## Interfaces

- **Offers:** `Policy::authorise(principal, channel, def, params) -> Decision { Run | PendingRequest | Refuse(code) }`; `PendingRequest` record; `FileHandle` type and resolver; network-guard test harness.
- **Consumes:** `CORE.REG` (side_effect, mcp_projection, fs_effect), `CORE.JRN` (append pending-request entries), `CORE.BUS.PRIN` (principal).

## Decomposition hints

CLASS (class and principal matrix), PROJ (MCP projection and pending requests), FS (handles and roots), EGRESS (X1 approval and network guard).

## Open questions

- Should a pending request expire? The concept docs are silent; a default expiry would be a policy setting owned here.
- D-023 (open) asks whether device traffic to a driver host on the local network is X1; REQ-008 allows network use inside I1 and D1 commands and will need no change if D-023 keeps that traffic in D1, but the EGRESS child must follow its outcome.
- The network guard needs an OS-level test (for example a sandbox that denies sockets); the mechanism per platform is left to the FS/EGRESS leaves.

## Children

_None yet: status `open`._
