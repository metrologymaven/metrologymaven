---
id: AUTO.RUN.EXEC.APPR
title: Motion approval hand-off inside a run
level: 4
parent: AUTO.RUN.EXEC
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.RUN.EXEC-005]
covers: []
depends_on: [AUTO.RUN.EXEC.CMD, CORE.BUS]
uses_artifacts: [CMM.EXEC.REQ, CMM.EXEC.TICKET, CMM.EXEC.RECORD]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.RUN.EXEC.APPR: Motion approval hand-off inside a run

## Purpose

Lets a Command Program run contain CMM motion without the run ever approving it. A program's motion step is an invocation of `cmm.request_run@1` (M1), which only files an inert run request (`CMM.EXEC.REQ`). This node's `StepGate` allows that step for permitted principals, then holds the run in state `awaiting_approval` at the next step boundary (or at run end) until the CMM side reports the outcome: the motion run completed, or the request was denied, expired or its ticket voided. It reads CMM state only through R0 commands and events; the approval itself happens at the Approval Station (`CMM.EXEC.STATION`).

## Scope

**In:**
- `ApprovalGate` (implements `StepGate`, registered into `execute_run`'s gate chain when the CMM domain is present)
- Allowing a `cmm.request_run@1` step only for principals the CMM rules allow; an agent principal's run stops with `motion_forbidden` before the step (D-008)
- Holding at the next boundary while a request filed by this run is outstanding; `held` run event with reason `awaiting_approval`
- Watching request, ticket and CMM run state through `cmm.list_run_requests@1`, `cmm.get_ticket@1` and `cmm.get_run@1` (all R0), by event subscription with a polling fallback
- Mapping the CMM outcome to continue or stop

**Out** (and which node owns it instead):
- Filing, queueing and rate-limiting run requests (`CMM.EXEC.REQ`); tickets and voiding (`CMM.EXEC.TICKET`); approval UI (`CMM.EXEC.STATION`); motion execution (`CMM.EXEC.DRIVE`); CMM run records (`CMM.EXEC.RECORD`)
- The gate chain, hold mechanics and cancellation (`AUTO.RUN.EXEC.CMD`)

## Requirements

- **REQ-AUTO.RUN.EXEC.APPR-001** If a run whose principal is an agent reaches a step invoking `cmm.request_run@1`, then the gate shall stop the run before that step with `motion_forbidden`.
- **REQ-AUTO.RUN.EXEC.APPR-002** When a step invoking `cmm.request_run@1` has committed and returned a request ID, the gate shall hold the run at the next step boundary, or before finalisation if it was the last step, and emit a `held` event with reason `awaiting_approval` and the request ID.
- **REQ-AUTO.RUN.EXEC.APPR-003** When the CMM run record for that request reaches final state `completed`, the gate shall release the hold with `Allow` and record the CMM run record's journal entry hash in the run's event stream.
- **REQ-AUTO.RUN.EXEC.APPR-004** If the request is rejected or expires, or its ticket is voided, or the CMM run ends aborted or faulted, then the gate shall stop the run with `approval_denied`, `approval_expired`, `ticket_voided` or `motion_failed` respectively, naming the request ID.
- **REQ-AUTO.RUN.EXEC.APPR-005** While holding, the gate shall issue no command other than R0 reads and shall release within 2 s [unverified] of the CMM outcome becoming readable.
- **REQ-AUTO.RUN.EXEC.APPR-006** If the run is cancelled while holding, then the gate shall stop waiting at once and shall not withdraw or alter the CMM request, leaving it to the CMM request lifecycle.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/program/motion/m1-step5.json` (step 5 invokes `cmm.request_run@1`) run by a human principal against the CMM simulator test harness, when the station harness approves and the simulated motion run completes, then the run holds before step 6 with `awaiting_approval`, continues after completion, and ends `succeeded`.
- **AC-2** Given the same program and the station harness rejecting the request, then the run stops with `approval_denied` naming the request ID and the Run entity's outcome is `failed`.
- **AC-3** Given the request left unanswered past its expiry in the harness, then `approval_expired`.
- **AC-4** (error path) Given the same program started by an agent principal, then the run stops before step 5 with `motion_forbidden` and no run request exists.
- **AC-5** Given a held run cancelled with `fid job cancel-job`, then the run ends `cancelled` within 1 s and the CMM request is still listed by `cmm.list_run_requests@1`.
- **AC-6** Given program `fixtures/program/motion/m1-last.json` whose last step requests motion, then the run is held before finalisation and ends `succeeded` only after the simulated motion run completes.

## Design

- **Registry command(s):** none of its own; reads `cmm.list_run_requests@1`, `cmm.get_ticket@1`, `cmm.get_run@1` (R0).
- **Data structures:** `ApprovalGate { outstanding: Option<RunRequestId>, watch: Box<dyn CmmWatch> }`; `trait CmmWatch { fn wait(&self, request: RunRequestId, cancel: &CancelToken) -> CmmOutcome }`; `CmmOutcome { Completed(EntryHash) | Rejected | Expired | Voided | Aborted | Faulted }`.
- **Algorithm:** before each step: if the principal is an agent and the step's command is `cmm.request_run@1`, `Stop(motion_forbidden)`; if `outstanding` is set, `Hold` and call `watch.wait` (event subscription to ticket and run events; fallback poll every 500 ms [unverified]); map the outcome; clear `outstanding`. After a `cmm.request_run@1` step commits, read its request ID from the step result and set `outstanding`. Evaluated again with the end-of-run key before finalisation.
- **Errors:** `motion_forbidden`, `approval_denied`, `approval_expired`, `ticket_voided`, `motion_failed`.
- **Determinism:** N: depends on human approval and device timing.
- **Performance budget:** release within 2 s of outcome [unverified until measured].
- **Crate:** `fid-run` (module `exec::approval`, behind cargo feature `cmm`)

## Test fixtures

`fixtures/program/motion/m1-step5.json`, `fixtures/program/motion/m1-last.json` (project-authored); CMM simulator and approval-station test harnesses from `CMM.SIM` and `CMM.EXEC` (P2).

## Build prompt

> In crate `fid-run`, behind cargo feature `cmm`, add module `exec::approval` with `ApprovalGate` implementing `StepGate` (re-exported by `exec::cmd`). Register it into the `execute_run` gate chain when the feature is on. Rules: a run by an agent principal stops with `motion_forbidden` before any step invoking `cmm.request_run@1` (D-008: agents never file run requests). For other principals, allow that step; after it commits, take the returned request ID and, at the next step boundary or before finalisation, return `Hold` with reason `awaiting_approval`. Wait through a `CmmWatch` that subscribes to ticket and CMM run events and falls back to polling `cmm.list_run_requests@1`, `cmm.get_ticket@1` and `cmm.get_run@1` (R0 only). Map outcomes: completed → `Allow` (record the CMM run record's entry hash in the run events); rejected → `approval_denied`; expired → `approval_expired`; voided → `ticket_voided`; aborted or faulted → `motion_failed`. On cancellation stop waiting at once and leave the CMM request untouched. Acceptance: AC-1..AC-6 in the spec with the CMM simulator and station harnesses. Constraints: CONSTITUTION §8 (no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`); never issue an M1, W3 or approval command from this module; no MCP surface.

## Open questions

- CMM is reached only through registry commands invoked by ID on the bus and the record formats those commands return, so CMM nodes appear in `uses_artifacts`, not `depends_on`. A code edge would also close a cycle, because `CMM.EXEC` declares `depends_on: [AUTO]`, which D-034 forbids (an L2 domain may not depend on L3); flagged for CMM, not edited from here.
- Assumes `cmm.request_run@1` returns its request ID as the step result and that `CMM.EXEC.TICKET`/`CMM.EXEC.RECORD` publish ticket and run events keyed by request ID; both CMM nodes are open, so the event names are to be confirmed there (cross-domain).
- Assumes a non-agent `script` principal (CLI, Python) may file run requests, since `CMM.EXEC.REQ` forbids only agents; the approval at the station is a separate human act in every case.

## Children

_Leaf: none._
