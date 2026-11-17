---
id: GDT.PMI.MAP.CMD
title: Propose characteristics from PMI command
level: 4
parent: GDT.PMI.MAP
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.PMI.MAP-001, REQ-GDT.PMI.MAP-003]
covers: [GOM.GDT.16, PCD.CAD.02, SA.GDT.01]
depends_on: [GDT.PMI.MAP.ITEM, GDT.CHAR.SPEC, CORE.JRN, CORE.BUS]
uses_artifacts: [IO.CAD.PMI]
side_effect: W1
determinism: D0
cli: [fid gdt propose-characteristics-from-pmi]
mcp: [gdt_propose_characteristics_from_pmi]
journal: [gdt.propose_characteristics_from_pmi@1]
updated: 2026-11-17
---

# GDT.PMI.MAP.CMD: Propose characteristics from PMI command

## Purpose

Owns `gdt.propose_characteristics_from_pmi@1`: for an imported model (given by handle), propose one governing spec per entity from the PMI's declared standard, falling back to the plan default marked `inherited_from: plan`, map every import-report entry to a proposal item, and write the resulting `ProposalSet` to a draft or agent branch. Nothing enters the working plan; adoption is `GDT.PMI.REVIEW`'s.

## Scope

**In:** `gdt.propose_characteristics_from_pmi@1` (W1); input by model handle only (no raw paths, CONSTITUTION §3); governing-spec proposal: declared standard designator and edition mapped to dialect and edition, defaults profile taken from the plan's default for that dialect and edition, else `not_evaluable{no_defaults_profile}`; when nothing is declared, the plan default with `inherited_from: plan`; the `ProposalSet { id, source_model_digest, items, created_by }` on the branch; the journal event.
**Out:** per-entity mapping (owned by `GDT.PMI.MAP.ITEM`); governing-spec validation rules (owned by `GDT.CHAR.SPEC`); adoption (owned by `GDT.PMI.REVIEW`); re-import diffs (owned by `GDT.PMI.DIFF`); CMM program creation from PMI (owned by `CMM.GEN`).

## Requirements

- **REQ-GDT.PMI.MAP.CMD-001** When the command runs on an imported model handle, it shall create exactly one proposal item per import-report entry in a new `ProposalSet` on a draft or agent branch and shall not modify the working plan.
- **REQ-GDT.PMI.MAP.CMD-002** When the PMI declares a standard, the item's governing spec shall name that dialect and edition with the plan's defaults profile for them; if the plan has none, then the item shall be `not_evaluable{no_defaults_profile}`.
- **REQ-GDT.PMI.MAP.CMD-003** When the PMI declares no standard, the item's governing spec shall be the plan default marked `inherited_from: plan`.
- **REQ-GDT.PMI.MAP.CMD-004** If the caller passes a raw file path instead of a model handle, or the handle is outside the caller's approved roots, then the command shall fail with `handle_required` or `handle_not_approved` and journal nothing.
- **REQ-GDT.PMI.MAP.CMD-005** The journal event shall record the source model digest, the proposal set ID, the item count by status and the principal.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/pmi/model_mixed.handle` (an imported model whose report matches `report_mixed.json`) and a plan with an ASME 2018 default, when `fid gdt propose-characteristics-from-pmi --model M --plan P --json` runs, then a proposal set with 5 items exists on a draft branch, the working plan is unchanged, and the journal counts 1 evaluable and 4 not evaluable.
- **AC-2** Given PMI declaring no standard, then each item's governing spec equals the plan default with `inherited_from: plan`.
- **AC-3** Given PMI declaring an ISO edition for which the plan has no defaults profile, then the items are `not_evaluable{no_defaults_profile}`.
- **AC-4** (error path) Given a raw path, then `handle_required` and nothing journaled.
- **AC-5** Given an MCP agent principal, then the set is written to that agent's branch.
- **AC-6** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command(s):** `gdt.propose_characteristics_from_pmi@1`, W1; params `model: ModelHandle`, `plan: PlanId`; result `ProposalSet` summary.
- **Data structures:** `ProposalSet`; `DeclaredStandard { designator, edition }` read from the import report.
- **Algorithm:** resolve handle; read report; per entry compute governing spec then call `map_entity`; write the set to the branch; journal.
- **Errors:** `handle_required`, `handle_not_approved`, `model_not_found`, `plan_not_found`, `unsupported_report_schema`.
- **Determinism:** D0.
- **Performance budget:** 5 000 entities < 1 s excluding I/O [unverified].
- **Crate:** `fid-gdt` (module `pmi::propose`).

## Test fixtures

Synthetic imported-model handles and reports (CC0).

## Build prompt

> In crate `fid-gdt`, module `pmi::propose`, implement registry command `gdt.propose_characteristics_from_pmi@1` (W1, `--json`; CLI `fid gdt propose-characteristics-from-pmi`; MCP `gdt_propose_characteristics_from_pmi`; one journal event). Accept only a model handle inside the caller's approved roots (`handle_required`, `handle_not_approved`). For each import-report entry: governing spec from the declared standard and edition with the plan's defaults profile for them (none gives `not_evaluable{no_defaults_profile}`), or the plan default marked `inherited_from: plan` when nothing is declared; then `map_entity`. Write a `ProposalSet { id, source_model_digest, items, created_by }` to a draft branch (agents: their agent branch) and never touch the working plan. Journal source digest, set ID, counts by status and principal. Tests per acceptance criteria. Constraints: CONSTITUTION C5 (proposals are adopted by humans elsewhere), C6 (PMI text is untrusted data), §3 (handles, no raw paths for agents and scripts), §8, no std `HashMap`/`HashSet`.

## Open questions

- Inherited from `GDT.PMI`: whether an MCP agent may run `propose` on a document it did not import itself; this leaf assumes handles to approved roots only, which covers both cases safely.

## Children

_Leaf: none._
