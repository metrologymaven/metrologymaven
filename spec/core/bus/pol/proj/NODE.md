---
id: CORE.BUS.POL.PROJ
title: MCP projection, pending requests and approval
level: 4
parent: CORE.BUS.POL
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-bus]
built_pr: 328
satisfies: [REQ-CORE.BUS.POL-002, REQ-CORE.BUS.POL-003, REQ-CORE.BUS.POL-004]
covers: []
depends_on: [CORE.REG.DEF, CORE.BUS.PRIN, CORE.BUS.PIPE.ERR, CORE.BUS.PIPE.HANDLER, CORE.JRN.ENTRY]
side_effect: [R0, W1, W1]
determinism: D0
cli: [fid request list-requests, fid request withdraw-request, fid request reject-request]
mcp: [request_list_requests, request_withdraw_request]
journal: [request.file_request@1, request.withdraw_request@1, request.reject_request@1]
updated: 2026-11-21
---

# CORE.BUS.POL.PROJ: MCP projection, pending requests and approval

## Purpose

Enforces each command's MCP projection at the bus, behind the generated MCP surface: `exposed` runs, `request_only` becomes a pending request for a human, `never` is refused. A pending request is a W1 Record Journal entry holding the command, its parameters digest and the requester. A human approves by invoking the same command with `approval_of` set; the command then runs with the human as principal and records the requester and request ID. Requesters may withdraw and humans may reject. The VIZ Proposal Inbox renders these records.

## Scope

**In:** projection check on MCP channels (local and server); `request_only` for every `human_only` command (D-031) and for X1 commands filed by agents and scripts; the pending-request record and its states; approval checks; `request.list_requests@1`, `request.withdraw_request@1`, `request.reject_request@1`; the internal `request.file_request@1` entry.
**Out:** rendering requests (`VIZ.INBOX`); motion run requests and the approval station (`CMM`); executing the approved command (`CORE.BUS.PIPE.STAGE`); X1-specific approval content (`CORE.BUS.POL.EGRESS`).

## Requirements

- **REQ-CORE.BUS.POL.PROJ-001** If a command whose projection is `request_only`, or whose definition has `human_only: true`, is invoked through an MCP channel, then the bus shall return `PendingRequest`, and the pipeline shall append one W1 `request.file_request@1` entry holding the command ID and major, the parameters commitment, the requester principal and the branch head at filing, and return the request ID.
- **REQ-CORE.BUS.POL.PROJ-002** If a command whose projection is `never` is invoked through an MCP channel, then the bus shall refuse it exactly as a command it does not hold: `not_found` with reason `unknown_command` and the same detail (`"<id>"@<major>: unknown command`), byte for byte, so the caller cannot tell that the command exists (D-035), and commit nothing.
- **REQ-CORE.BUS.POL.PROJ-003** When a `human` principal invokes a command with `approval_of` naming a pending request whose command and parameters digest match the invocation, the bus shall allow it, mark the request `approved` in the same commit, and record the requester and request ID in the entry.
- **REQ-CORE.BUS.POL.PROJ-004** If `approval_of` names a request that is not pending, whose parameters digest differs, or the approver is not `human`, then the bus shall return `policy` with reason `bad_approval` and commit nothing.
- **REQ-CORE.BUS.POL.PROJ-005** When `request.withdraw_request@1` is invoked by the requester of a pending request, the bus shall mark it `withdrawn`; any other principal shall get `policy` with reason `not_requester`.
- **REQ-CORE.BUS.POL.PROJ-006** When a `human` principal invokes `request.reject_request@1` with a reason, the bus shall mark the pending request `rejected` and store the reason as an untrusted text commitment.
- **REQ-CORE.BUS.POL.PROJ-007** When `request.list_requests@1` runs, the bus shall return the caller's own requests, or all requests for a `human` caller, sorted by filing sequence, filterable by state.
- **REQ-CORE.BUS.POL.PROJ-008** If approval executes the command and any later pipeline stage fails, then the request shall stay `pending` and nothing shall be committed.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/bus/pol/proj/registry.json` (commands with projections exposed, request_only and never, and one `human_only` W2 command), when an MCP agent invokes the `request_only` and the `human_only` command, then each returns a request ID, the Record Journal gains one `request.file_request@1` entry each, and no handler runs.
- **AC-2** (error path) Given the `never` command over MCP, then `not_found` / `unknown_command`, byte for byte equal (RFC 8785 JSON of code, reason and detail) to what the command table returns for the same `id@major` when it does not hold it, and nothing is journaled.
- **AC-3** Given a pending request R, when a GUI human invokes the same command and parameters with `approval_of: R`, then one entry is appended with principal `human`, `requested_by` = the agent and `request_id` = R, and `fid request list-requests --json` shows R as `approved`.
- **AC-4** (error path) Given R and an approval whose parameters differ by one field, then `policy` / `bad_approval`; given an agent setting `approval_of`, then `policy` / `bad_approval`.
- **AC-5** Given R, when the agent withdraws it, then R is `withdrawn`; when another agent withdraws it, then `policy` / `not_requester`.
- **AC-6** Given R, when a human runs `fid request reject-request R --reason "wrong datum" --json`, then R is `rejected` and the reason is stored as untrusted text.
- **AC-7** (replay) Given the journal from AC-1 and AC-3, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):**
  - `request.list_requests@1` R0; params `{ state?: RequestState }`; result `{ requests: [RequestSummary] }`.
  - `request.withdraw_request@1` W1; params `{ request: RequestId }`; projection `exposed`.
  - `request.reject_request@1` W1; params `{ request: RequestId, reason: Untrusted<String> (≤2,000) }`; `human_only: true`; projection `never` (rejecting is a human act in a desktop app or upgraded CLI).
  - `request.file_request@1` W1; exposure `internal` (written only by the pipeline, never invoked by a surface).
- **Data structures:** `PendingRequest { id: RequestId (UUIDv7), command: CommandRef, params_commitment, requester: Principal, filed_at_head: EntryHash, state: pending|approved|rejected|withdrawn }`; `fn authorise_projection(channel, def) -> Decision`.
- **Algorithm:** on MCP channels: `never` → refuse as unknown (`fid_bus::validate::unknown_command`, the function the command table's resolve also uses); `request_only` or `human_only` → PendingRequest; else Run. For `approval_of`: load the request, check state, principal and parameters digest, then let the pipeline continue with the human principal and add a `RequestApproved` event to the same commit.
- **Errors:** `policy` (reasons `bad_approval`, `not_requester`, `human_only`), `not_found` (`unknown_command` for a `never` command over MCP, `unknown_request`). The catalogue keeps the reason `policy/not_on_mcp` (reasons are never removed from the bus error catalogue), but this check no longer returns it.
- **Determinism:** D0; request IDs are UUIDv7 recorded as observed inputs.
- **Performance budget:** listing 10,000 requests in < 50 ms [unverified until measured].
- **Crate:** `fid-bus` (module `policy::projection`).

## Test fixtures

`fixtures/core/bus/pol/proj/registry.json`, a synthetic test registry; CC-BY-4.0.

## Build prompt

> In crate `fid-bus`, add module `policy::projection`. `authorise_projection(channel, def)`: on MCP channels, projection `never` → refused exactly as an unknown command (`not_found`/`unknown_command`, D-035); projection `request_only` or `human_only: true` → `Decision::PendingRequest`, which the pipeline commits as one W1 `request.file_request@1` entry (command ref, parameters commitment, requester, head at filing) and returns the request ID. Approval: a human invocation with `approval_of: RequestId` is allowed only if the request is pending and the command and parameters digest match; the commit carries `RequestApproved` plus the requester and request ID; any failure leaves the request pending (`bad_approval` for mismatches or non-human approvers). Register `request.list_requests@1` (R0; own requests, all for humans; sorted by filing sequence), `request.withdraw_request@1` (W1; requester only, else `not_requester`) and `request.reject_request@1` (W1, `human_only`, projection never; untrusted reason ≤2,000 chars). CLI `fid request list-requests | withdraw-request | reject-request`; MCP `request_list_requests`, `request_withdraw_request`. Tests: AC-1..AC-7 with `fixtures/core/bus/pol/proj/registry.json`. Constraints: CONSTITUTION §3 (projections, human-only D-031, agents never adopt their own proposals), C5, C6, §8.

## Open questions

- Pending requests do not expire; an expiry setting may be added later. A request filed against an old head is still re-validated (preconditions, conflicts) when approved.
- The surface for approving on the CLI is an upgraded human session (D-012) passing `--approve-request <id>`; the flag name belongs to `AUTO.CLI`.
- **Built in `fid-bus` (`policy::projection`); what is left for other leaves.** AC-1, AC-2, AC-4 and AC-5 are built and tested. AC-3 is built up to the decision: the approval check, the `RequestApproved` event (requester, request ID and record digest) and the `approved` state in `request.list_requests@1`; appending the approved command's entry is `CORE.BUS.PIPE.STAGE` and `fid request list-requests --json` is `AUTO.CLI.TREE`. AC-6 is built as the `request.reject_request@1` handler; its CLI form is `AUTO.CLI.TREE`. AC-7 (exact-mode replay) needs `CORE.RPL.EXEC.LOOP`; a determinism test covers the parts here. REQ-008's "a later stage fails" is the pipeline's: this module changes no request until the approving entry is committed and folded.
- **Check order (maintainer decision on #328).** For MCP channels and `agent` principals the projection check and filing come before the class check (`CORE.BUS.POL.CLASS`), so an agent's call to a `human_only` or W3 command becomes a pending request, not `human_only` or `class_forbidden`; calls the projection lets run then pass the class check; an approval runs the class check on the approving human. `policy::projection::authorise` is the one entry point and applies them in this order.
- **Races.** `Approval` and `Filing` carry the head they were decided at (`decided_at_head`). Two approvals of one request decided at the same head can both be produced; `CORE.BUS.PIPE.STAGE` must re-check at append time, by compare-and-swap on the branch head or under the append lock, so only one commits. The fold already refuses the second.
- **The fold trusts no event body for who and what.** It takes each entry's principal and command: `RequestFiled` only in `request.file_request@1` entries of the requester, `RequestWithdrawn` only in `request.withdraw_request@1` entries of the requester, `RequestRejected` only in `request.reject_request@1` entries of the human rejecter, `RequestApproved` only in the approved command's entry of the human approver, with matching record digest and parameters commitment, and the approving entry's own parameters commitment must equal the record's. Registry validation reserves these effect names (`reserved_effect`), so no other command can declare them.
- A rejection reason may not hold Unicode 16.0 Cc, Cf, Zl or Zp code points, noncharacters (U+FDD0..U+FDEF and every U+xFFFE/U+xFFFF) or tag characters (U+E0000..U+E007F); the table is `REASON_FORBIDDEN_RANGES`.
- Parameters reach `authorise` as `ValidParams` from the validation stage (`CORE.BUS.PIPE.VALID`); filing re-checks them against the structural keywords of the target's `params_schema` and a 64 KiB bound as defence in depth. A non-requester's withdrawal stays `policy/not_requester`, as REQ-005 and AC-5 require, although it tells a caller that the request ID exists.
- Pending-request records reach handlers as entities of kind `request.pending_request` through the handler `StateView`; the project's state view must include them when `CORE.BUS.PIPE.STAGE` wires the pipeline. Filing an X1 command by a `script` outside MCP is `CORE.BUS.POL.EGRESS`.
- **Other ways a hidden command can still show (D-035), for other leaves.** (a) `CommandTable::resolve` (`CORE.BUS.PIPE.VALID`) does not know the audience: for `id@N` whose name the table holds under other majors it says `not_found/unknown_major`, so an agent asking for a missing major of a hidden name learns the name exists, and a hidden major of a name with a visible major is `unknown_command` here where a missing major of that name is `unknown_major`. (b) When `CORE.BUS.PIPE.STAGE` wires the pipeline, resolve and parameter validation run before `authorise`; a validation error against a hidden command's `params_schema` would reveal it, so for MCP and `agent` callers the projection check (or an audience-aware resolve) must come before validation.

## Change log

- 2026-11-21 (security fix for D-035, maintainer decision): a `never`-projected command over MCP was refused as `policy/not_on_mcp` with detail "`<id>@<major>` is not available over MCP", which told an agent that the hidden command exists. It is now refused exactly as an unknown command (`not_found/unknown_command`, same detail shape, built by the same function as the command table's refusal). REQ-002, AC-2, Algorithm and Errors are revised; the remaining paths are listed under Open questions. No REQ ID was renumbered.

## Children

_Leaf: none._
