---
id: CMM.GEN.PROV
title: Generated-program provenance and agent-authored flag
level: 3
parent: CMM.GEN
kind: task
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-CMM.GEN-004]
covers: []
depends_on: [CORE.JRN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CMM.GEN.PROV: Generated-program provenance and agent-authored flag

## Purpose

Computes the provenance record attached to every generated or converted program: source kind, generator ID and version, input digests and the agent-authored flag. The flag decides, under D-008, whether a human other than the requester must release the program, so it must be computed from the journal's principal records rather than declared by the caller, and it must be sticky: once any step was issued or edited by an agent principal, no later human edit clears it.

## Scope

**In:**
- `GeneratedProgramProvenance { source_kind, generator: String@semver, input_digests: Vec<Digest> (sorted), agent_authored: bool, agent_principals: Vec<PrincipalId>, computed_at_entry: EntryHash }`
- `provenance_for(program_id, journal_view) -> Provenance`: a pure function over the journal entries that created or edited the program's steps
- Canonical digest of the provenance record

**Out** (and which node owns it instead):
- Principals and their kind (human or agent) (owned by `CORE`)
- Enforcing released-by-other at ticket issue (owned by `CMM.EXEC.TICKET`)
- The release lifecycle (owned by `AUTO.REC`)

## Requirements

- **REQ-CMM.GEN.PROV-001** The system shall attach to every generated or converted program a provenance record with source kind (`CAD`, `PMI`, `TEMPLATE`, `DMIS`, `AGENT`, `MANUAL`), generator ID and version, and the sorted list of input digests.
- **REQ-CMM.GEN.PROV-002** The system shall set `agent_authored` to true when any journal entry that created or edited any step of the program was issued by a principal of kind agent, and shall list those principals.
- **REQ-CMM.GEN.PROV-003** The system shall never clear `agent_authored` once set for a program lineage, including after later edits by humans, copies or re-generation from an agent-authored program.
- **REQ-CMM.GEN.PROV-004** The system shall compute the provenance only from journal entries and never from a caller-supplied flag.
- **REQ-CMM.GEN.PROV-005** The provenance digest shall be SHA-256 over RFC 8785 canonical JSON of the record.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/gen/journal_human_only.jsonl` (program created and edited by human principals), when `provenance_for` runs, then `agent_authored` is false and the digest equals the fixture's `expected_digest`.
- **AC-2** Given fixture `fixtures/cmm/gen/journal_agent_step.jsonl` (one step edited by an agent principal, later edits by humans), when `provenance_for` runs, then `agent_authored` is true and the agent principal is listed.
- **AC-3** Given fixture `fixtures/cmm/gen/journal_copy_of_agent.jsonl` (a human copies the AC-2 program), when `provenance_for` runs on the copy, then `agent_authored` is true.
- **AC-4** (error path) Given a journal view missing the program's creation entry, when `provenance_for` runs, then it fails with `PROVENANCE_INCOMPLETE` rather than returning `agent_authored: false`.
- **AC-5** (replay) Given AC-1 to AC-3, when run twice, then the provenance digests are bit-identical.

## Design

- **Interface:** `fn provenance_for(program: ProgramId, journal: &dyn JournalView) -> Result<GeneratedProgramProvenance, ProvenanceError>`; called by every `cmm.generate_*` and `cmm.convert_*` command before commit. Side-effect class R1: pure read and compute.
- **Algorithm:** walk the program lineage (creation entry, edits, copy-of links) in journal order; collect principal kinds; sort input digests and principals by byte order.
- **Errors:** `PROVENANCE_INCOMPLETE { missing }`, `PROGRAM_NOT_FOUND`.
- **Determinism:** D0.
- **Crate:** `fid-cmm-gen`.

## Test fixtures

Synthetic journal excerpts in `fixtures/cmm/gen/` (`journal_human_only.jsonl`, `journal_agent_step.jsonl`, `journal_copy_of_agent.jsonl`), hand-written against the CORE journal entry schema, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-cmm-gen`, implement `provenance_for(program, journal) -> GeneratedProgramProvenance` (pure, side-effect class R1), used by every program generation and conversion command.
> The record holds source kind (CAD, PMI, TEMPLATE, DMIS, AGENT, MANUAL), generator ID@semver, sorted input digests, `agent_authored`, the agent principals and the journal entry hash it was computed at. `agent_authored` is true if any entry that created or edited any step in the program's lineage (including copies) came from an agent principal; it is computed only from the journal, never from a caller flag, and is never cleared. A lineage with a missing creation entry fails with `PROVENANCE_INCOMPLETE`. Digest = SHA-256 over RFC 8785 JCS.
> Acceptance tests: the five criteria in spec node `CMM.GEN.PROV`, fixtures in `fixtures/cmm/gen/`.
> Constraints: this flag gates released-by-other under decision D-008, so fail closed; no panic, no `unsafe`, no `unwrap`/`expect`; determinism D0.

## Open questions

- None.

## Children

_Leaf: none._
