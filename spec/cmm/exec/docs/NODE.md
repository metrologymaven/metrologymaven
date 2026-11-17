---
id: CMM.EXEC.DOCS
title: Motion Executive positioning, instructions for use and IQ text
level: 3
parent: CMM.EXEC
kind: task
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-CMM.EXEC-021]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CMM.EXEC.DOCS: Motion Executive positioning, instructions for use and IQ text

## Purpose

Writes and guards the documentation that positions the Motion Executive correctly: it is machine protection and process assurance, not a safety function, and no risk-reduction credit may be taken for it. The same statements go into the product documentation, the instructions for use and the installation-qualification (IQ) pack, together with the machine-side isolation checklist. A docs lint keeps the wording from drifting.

## Scope

**In:**
- Required statements (by ID) and where each must appear
- The machine-side isolation checklist in the IQ pack
- A docs-lint rule run in CI over the documentation sources

**Out** (and which node owns it instead):
- The documentation portal and build (owned by `OUTR.DOCS`)
- Retrofit and integration legal checklist wording (counsel review; tracked in D-008 follow-ups)

## Requirements

- **REQ-CMM.EXEC.DOCS-001** The product documentation, the instructions for use and the IQ pack shall each contain statement S1 (the Motion Executive is machine protection and process assurance, not a safety function) and statement S2 (no risk-reduction credit may be taken for it; the controller's own safety functions and the hardware emergency stop must be sufficient on their own).
- **REQ-CMM.EXEC.DOCS-002** The IQ pack shall contain the machine-side isolation checklist: dedicated machine-side host or separate service account; per-application firewall rule allowing only the signed Motion Executive binary to reach the controller port; controller on a dedicated NIC or VLAN; no MCP host with shell or code tools on the machine PC; a statement that I++ DME has no client authentication.
- **REQ-CMM.EXEC.DOCS-003** The docs lint shall fail if any documentation source describes the Motion Executive, setup cap, heartbeat or placement check as a safety function, safety feature or safety measure, except inside the negated statements S1 and S2.
- **REQ-CMM.EXEC.DOCS-004** The docs lint shall fail if statement S1 or S2 is missing from any of the three document sets, or if any checklist item from REQ-CMM.EXEC.DOCS-002 is missing from the IQ pack.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/exec/docs_lint/good/` (three document sets with S1, S2 and the full checklist), when the docs lint runs, then it passes with zero findings.
- **AC-2** Given fixture `fixtures/cmm/exec/docs_lint/missing_s2/`, when the lint runs, then it fails naming the instructions-for-use set and statement S2.
- **AC-3** Given fixture `fixtures/cmm/exec/docs_lint/safety_wording/` (a page calling the setup cap a "safety feature"), when the lint runs, then it fails naming the file and line.
- **AC-4** Given fixture `fixtures/cmm/exec/docs_lint/missing_vlan/` (IQ pack without the NIC or VLAN item), when the lint runs, then it fails naming the checklist item.
- **AC-5** Given AC-1 run twice, then the findings output is byte-identical.

## Design

- **Statements:** S1 and S2 are stored once as tagged snippets (`<!-- stmt:S1 -->`) and included in each document set, so wording stays identical.
- **Lint:** a small rule in the docs CI: scan Markdown sources; locate tagged statements; regex for "safety (function|feature|measure)" near the listed component names outside tagged statements; checklist items matched by tag IDs. Output sorted by file and line.
- **Side-effect class:** R0 (read-only check in CI); no registry command.
- **Determinism:** D0.
- **Crate:** `fid-docs-lint` rule module (or the docs tooling the project adopts; one crate).

## Test fixtures

Small Markdown trees under `fixtures/cmm/exec/docs_lint/` (`good`, `missing_s2`, `safety_wording`, `missing_vlan`), written for this leaf, CC-BY-4.0.

## Build prompt

> Add documentation and a docs-lint rule that positions the Motion Executive (ME) correctly.
> Write statements S1 ("the ME is machine protection and process assurance, not a safety function") and S2 ("no risk-reduction credit may be taken for it; the controller's safety functions and hardware e-stop must suffice on their own") once as tagged snippets and include them in the product docs, the instructions for use and the IQ pack. Add the IQ machine-side isolation checklist (dedicated host or service account; per-application firewall rule so only the signed ME binary reaches the controller port; controller on its own NIC or VLAN; no MCP host with shell or code tools on the machine PC; I++ DME has no client authentication), each item tagged.
> Implement the lint (one crate or the project's docs tooling): fail if S1 or S2 is missing from any set, if any checklist item is missing from the IQ pack, or if the ME, setup cap, heartbeat or placement check is called a safety function, feature or measure outside S1 and S2. Output sorted, deterministic.
> Acceptance tests: the five criteria in spec node `CMM.EXEC.DOCS`, fixtures in `fixtures/cmm/exec/docs_lint/`.
> Constraints: no panic, no `unsafe`, no `unwrap`/`expect`. Counsel review of the final wording is recommended before the first live-machine release (D-008).

## Open questions

- Final wording should be reviewed by counsel (D-008 recommendation); the lint guards structure, not legal sufficiency.

## Children

_Leaf: none._
