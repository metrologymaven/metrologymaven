---
id: GDT.PMI.REVIEW
title: Proposal review and human adoption
level: 3
parent: GDT.PMI
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.PMI-002]
covers: [GOM.GDT.16]
depends_on: [GDT.PMI.LINK, CORE.JRN, CORE.BUS]
side_effect: W2
determinism: D0
cli: [fid gdt adopt-characteristics]
mcp: [gdt_adopt_characteristics]
journal: [gdt.adopt_characteristics@1]
updated: 2026-11-16
---

# GDT.PMI.REVIEW: Proposal review and human adoption

## Purpose

Proposed characteristics count toward the inspection plan only after a human adopts them (CONSTITUTION C5, REQ-GDT-006). This leaf owns `gdt.adopt_characteristics@1`: adopt selected items or a whole proposal set into the working plan, refuse `not_evaluable` items, record the adopting human, and project the command to MCP as a request that a human must confirm, never as an agent action.

## Scope

**In:** adoption of selected items or a set; validation (item exists, still pending, evaluable, governing spec complete); journal event with adopting principal; MCP projection as an adoption **request** that waits for human confirmation; rejecting proposal items explicitly (`--reject`), recorded with a reason.
**Out:** creating proposals (owned by `GDT.PMI.MAP`); link preservation (owned by `GDT.PMI.LINK`); review UI (owned by `VIZ` and the apps).

## Requirements

- **REQ-GDT.PMI.REVIEW-001** When a human principal runs `gdt.adopt_characteristics@1` on pending evaluable items, the system shall copy them into the working plan as characteristics and journal one event listing item IDs, new characteristic IDs and the adopting principal.
- **REQ-GDT.PMI.REVIEW-002** If the invoking principal is an agent, then the command shall not adopt; it shall create a pending adoption request that takes effect only when a human confirms it, and the confirmation shall be journaled under the human principal.
- **REQ-GDT.PMI.REVIEW-003** If any selected item is `not_evaluable`, already adopted or rejected, or lacks a complete governing spec, then the command shall fail with a typed error naming the items and adopt none of the selection.
- **REQ-GDT.PMI.REVIEW-004** When an item is rejected with `--reject`, the system shall record the rejecting principal and reason, and the item shall no longer be adoptable.
- **REQ-GDT.PMI.REVIEW-005** Adoption shall preserve each item's `PmiLink` and governing spec, including the `inherited_from` marker.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/pmi/proposal_set_5.json` (five pending items, one `not_evaluable`), when a human runs `fid gdt adopt-characteristics --items i1,i2,i3 --json`, then three characteristics exist in the working plan with their links and one event names the human.
- **AC-2** (error path) Given `--items i1,i5` where i5 is `not_evaluable`, then the command fails with `items_not_adoptable{i5}` and adopts nothing.
- **AC-3** (agent) Given principal `agent` calling MCP `gdt_adopt_characteristics` on i4, then no characteristic is created, a pending request exists, and after a human confirms it, i4 is adopted with the human as adopting principal.
- **AC-4** Given `--reject i4 --reason duplicate`, then i4 is rejected and a later adoption of i4 fails with `items_not_adoptable{i4}`.
- **AC-5** (replay) Exact replay of AC-1 and AC-4 gives `IDENTICAL`.

## Design

- **Registry command(s):** `gdt.adopt_characteristics@1`, W2. Params: `set: ProposalSetId`, `items: [ItemId]` or `all: bool`, `reject: Option<Reason>`. Result: adopted characteristic IDs or a pending request ID. MCP projection: request-only (the bus queues it for human confirmation), consistent with C5.
- **Data structures:** `ItemState { Pending, Adopted{char_id, by}, Rejected{by, reason} }`; `AdoptionRequest { items, requested_by, confirmed_by: Option }`.
- **Algorithm:** validate all, then apply atomically in one journal event; agents create a request instead.
- **Errors:** `items_not_adoptable{ids}`, `set_unknown`, `request_not_found`.
- **Determinism:** D0.
- **Performance budget:** 1 000 items adopted in < 100 ms [unverified].
- **Crate:** `fid-gdt`.

## Test fixtures

Synthetic proposal sets (CC0, in-repo); principal fixtures for human and agent from the command bus test harness.

## Build prompt

> In crate `fid-gdt`, implement `gdt.adopt_characteristics@1` (W2; CLI `fid gdt adopt-characteristics`; MCP `gdt_adopt_characteristics` projected as a request-only tool). Human principal: validate every selected item (pending, evaluable, complete governing spec; otherwise `items_not_adoptable{ids}` and adopt none), then copy items into the working plan as characteristics, preserving `PmiLink` and governing spec (including `inherited_from`), and journal one event with item IDs, new characteristic IDs and the adopting principal. Agent principal: never adopt; create an `AdoptionRequest` that a human confirms, journaled under the human. `--reject <reason>` marks items rejected with principal and reason. Tests per the acceptance criteria, including exact replay. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), C5 (agents never adopt), no std `HashMap`/`HashSet`.

## Open questions

- **Revise for D-036/D-037 (accepted 2026-11-17):** an adoption command is now an item of the H2 batched release-and-adoption queue (AGENT.GATEPREP), not a separate stop; keep it `human_only`.
- The request-and-confirm projection for W2 adoption follows the C5 pattern; if `CORE.BUS` standardises a generic "agent requests human confirmation" mechanism, this leaf should use it.

## Children

_Leaf: none._
