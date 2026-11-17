---
id: CORE.RPL.RETGT.RUN
title: Re-targeted replay to a new branch
level: 4
parent: CORE.RPL.RETGT
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL.RETGT-001, REQ-CORE.RPL.RETGT-002]
covers: []
depends_on: [CORE.RPL.EXEC.LOOP, CORE.JRN.BR, CORE.BUS.PIPE]
side_effect: W2
determinism: D0
cli: [fid journal replay-retargeted]
mcp: [journal_replay_retargeted]
journal: [journal.replay_retargeted@1]
updated: 2026-11-17
---

# CORE.RPL.RETGT.RUN: Re-targeted replay to a new branch

## Purpose

"Do the same thing to the next part." The command re-runs a recorded journal span with chosen recorded inputs (a measured mesh, a point cloud, an imported CAD element) bound to new entities, and commits every resulting command to a newly created branch. The source branch is never touched. Bindings are type-checked before anything runs.

## Scope

**In:** the binding model (which recorded inputs may be re-bound), the type check, branch creation for the output, execution through LOOP with commits to the new branch, the command `journal.replay_retargeted@1`.
**Out:** Command Programs and named parameters (`AUTO.PROG`); throwaway execution (`CORE.RPL.RETGT.DRY`); promotion of the new branch (`CORE.JRN.BR`).

## Requirements

- **REQ-CORE.RPL.RETGT.RUN-001** A recorded input shall be re-bindable when it is an entity reference argument (`x-ref`) of a W-class entry in the span and its entity was created outside the span.
- **REQ-CORE.RPL.RETGT.RUN-002** If a binding names an input that is not re-bindable, or a new entity that does not exist on the base head, then the command shall fail with `binding_invalid` before executing any step.
- **REQ-CORE.RPL.RETGT.RUN-003** If a binding's new entity type differs from the recorded input's entity type, then the command shall fail with `binding_type` naming the input before executing any step.
- **REQ-CORE.RPL.RETGT.RUN-004** When the bindings are valid, the command shall create a new branch from the chosen base head, commit one `journal.replay_retargeted@1` entry recording the span, base head and bindings, and then commit each re-executed command as its own entry on that branch.
- **REQ-CORE.RPL.RETGT.RUN-005** Re-executed commands shall take fresh class-N observations (clock, salts) but reuse recorded arguments other than the bound inputs, and if a step needs an I1, D1 or M1 observation not covered by a binding, then the command shall stop with `step_needs_device` and leave the new branch with the entries committed so far.
- **REQ-CORE.RPL.RETGT.RUN-006** The command shall never append to the source branch, and when called by an agent principal the new branch shall be an agent branch.
- **REQ-CORE.RPL.RETGT.RUN-007** Entities created inside the span shall receive derived UUIDv5 IDs from the new branch's replay entry and the recorded ID, so two re-targeted runs with the same inputs give the same IDs.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/core/rpl/retgt/run/part_a` (a span that imports mesh M1, aligns it and evaluates 5 characteristics) and a second mesh M2 in the project, when `fid journal replay-retargeted --branch main --from 3 --to 12 --bind M1=M2 --json` runs, then a new branch exists with 1 + 10 entries, 5 characteristic results computed from M2, and `main`'s head is unchanged.
- **AC-2** (error path) Given `--bind M1=<a CAD body ID>`, then the command fails with `binding_type` and no branch is created.
- **AC-3** (error path) Given a binding for an input created inside the span, then the command fails with `binding_invalid`.
- **AC-4** Given AC-1 run twice into two branches, then the derived entity IDs on both branches are equal.
- **AC-5** (agent) Given an MCP agent session, when it calls `journal_replay_retargeted`, then the new branch is an agent branch and the agent cannot promote it.

## Design

- **Registry command(s):** `journal.replay_retargeted@1`: W2, projection `exposed` (bus routes agent commits to an agent branch), D0 orchestration, `undo: head_move`, `fs_effect: none`, `network: none`, `web: unavailable` (note "needs project storage"); params `{ branch: string (x-ref branch), from_seq: integer, to_seq: integer, base?: string (entry hash; default source head), bindings: [{ input: string (x-ref entity), entity: string (x-ref entity) }], new_branch?: string }`; result `{ branch, entries: integer, steps: [{ seq, outcome, new_entry }] }`.
- **Data structures:** `Binding { input: EntityId, entity: EntityId }`; `RebindableInput { entity, entity_type, first_seq }`.
- **Algorithm:** scan span for `x-ref` arguments → set of external inputs → check bindings (existence on base, type equality) → create branch → commit replay entry → LOOP with a sink that commits to the new branch, argument rewriter for bound IDs and UUIDv5 derivation for created IDs.
- **Errors:** `binding_invalid`, `binding_type`, `step_needs_device`, `branch_exists`, `span_invalid`.
- **Determinism:** D0 orchestration; step outputs per their class.
- **Performance budget:** overhead < 2 ms per step beyond handler time [unverified until measured].
- **Crate:** `fid-replay` (module `retarget`).

## Test fixtures

`fixtures/core/rpl/retgt/run/` synthetic projects with two small meshes (project-generated, seed 11); CC-BY-4.0.

## Build prompt

> In crate `fid-replay`, add module `retarget` and register `journal.replay_retargeted@1` (W2, exposed, undo head_move; params branch, from_seq, to_seq, optional base, bindings of input entity to new entity, optional new_branch). Re-bindable inputs are `x-ref` arguments of W entries whose entities were created outside the span. Validate every binding (exists on base: `binding_invalid`; same entity type: `binding_type`) before any step. Create the new branch, commit one replay entry recording span, base and bindings, then run the span through `run::replay_span` with a sink committing to the new branch, bound IDs rewritten, created IDs derived as UUIDv5 from the replay entry and recorded ID, fresh clock and salts, and stop with `step_needs_device` on unbound I1/D1/M1 observations. Never touch the source branch; agent callers get an agent branch. Tests AC-1..AC-5. Constraints: CONSTITUTION §3 (agents write to agent branches), §5 (UUIDv5 derived IDs), C5, §8.

## Open questions

- Programs (`AUTO.PROG`) will call this with named parameters instead of raw entity IDs; the mapping is AUTO's.

## Children

_Leaf: none._
