---
id: VIZ.INBOX.ACTIONS.REQS
title: Pending request review, approval and rejection
level: 4
parent: VIZ.INBOX.ACTIONS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX.ACTIONS-001, REQ-VIZ.INBOX.ACTIONS-002]
covers: []
depends_on: [CORE.BUS.POL.PROJ, CORE.REG.QRY, VIZ.SHELL.DOCK]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.INBOX.ACTIONS.REQS: Pending request review, approval and rejection

## Purpose

Shows the pending requests agents and scripts file for human-only and request-only commands (for example `capture.start_recording@1`, `egress.send@1`, `journal.request_promote_branch@1`, `view.reject_proposal@1`) in the Inbox, with the requested command, its class, its arguments and the requester. A human approves by invoking the same command with `approval_of` (`CORE.BUS.POL.PROJ`) or rejects with `request.reject_request@1`. An approval re-sends exactly the requested arguments; the reviewer cannot change them in this panel.

## Scope

**In:** the requests tab (from `request.list_requests@1`, state `pending`); argument display from the request payload with untrusted strings quoted; approve (invoke with `approval_of`); reject (`request.reject_request@1` with a reason); special display for `egress.send@1` (artifact digest, size and destination ID).
**Out:** filing, approval checks and states (`CORE.BUS.POL.PROJ`); proposals on agent branches (`VIZ.INBOX.ACTIONS.CHIPS`); egress execution (`CORE.BUS.POL.EGRESS`).

## Requirements

- **REQ-VIZ.INBOX.ACTIONS.REQS-001** The requests tab shall list every pending request with command ID, side-effect class, requester, filing head and arguments, sorted by filing sequence.
- **REQ-VIZ.INBOX.ACTIONS.REQS-002** Approve shall invoke the requested command with `approval_of` and the request's recorded arguments unchanged, under the GUI session's human principal, and shall offer no way to edit them.
- **REQ-VIZ.INBOX.ACTIONS.REQS-003** Each request row shall show its side-effect class and, for `egress.send@1`, the artifact digest, size and destination before approval.
- **REQ-VIZ.INBOX.ACTIONS.REQS-004** Reject shall invoke `request.reject_request@1` with the reviewer's reason.
- **REQ-VIZ.INBOX.ACTIONS.REQS-005** Untrusted arguments and notes shall be rendered as quoted plain text.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/inbox-requests` (an agent's pending `capture.start_recording@1` and `egress.send@1`), when the tab opens, then both rows show class, requester and arguments; the egress row shows digest, size and destination.
- **AC-2** Given approve on the recording request, then the bus records one approval with the request ID and the recording starts; the request state is `approved`.
- **AC-3** Given reject with a reason, then the request is `rejected` and nothing ran.
- **AC-4** Given a note with markup, then it renders literally.

## Design

- **Registry command(s):** none owned; invokes `request.list_requests@1`, `request.reject_request@1` and the requested commands with `approval_of`.
- **Data structures:** `RequestRow { id, command, class, requester, filed_at_head, args: JsonWithUntrusted }`.
- **Algorithm:** list; read argument payloads; render with the generated read-only form view; on approve, re-submit the stored arguments with `approval_of`.
- **Errors:** passed through (`bad_approval`, `policy`).
- **Determinism:** D0.
- **Performance budget:** 1,000 pending requests listed < 100 ms [unverified].
- **Crate:** `fid-inbox` (module `actions::reqs`).

## Test fixtures

`fixtures/viz/inbox-requests`: generated journal with scripted agent requests; CC-BY-4.0.

## Build prompt

> In crate `fid-inbox`, add module `actions::reqs`: an Inbox tab listing pending requests from `request.list_requests@1` (command ID, class, requester, filing head, arguments from the payload, untrusted strings quoted; `egress.send@1` rows show digest, size and destination), with Approve (invoke the requested command with `approval_of` and the recorded arguments unchanged, as the GUI session's human principal) and Reject (`request.reject_request@1` with a reason). No argument editing. Tests AC-1..AC-4. Rules: humans adopt agent work (C5); untrusted text is data (C6); data leaves the machine only via `egress.send@1` (CONSTITUTION §3.1); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Added under ACTIONS because `CORE.BUS.POL.PROJ` and `VIZ.CAPTURE.REC.SESSION` route agent requests to the Inbox, while the parent scope names only branch proposals.

## Children

_Leaf: none._
