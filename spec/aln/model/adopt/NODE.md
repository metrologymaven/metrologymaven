---
id: ALN.MODEL.ADOPT
title: Agent provenance and human adoption of alignments
level: 3
parent: ALN.MODEL
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.MODEL-008, REQ-ALN.MODEL-009]
covers: []
depends_on: [ALN.MODEL.ELEM, CORE.BUS, CORE.DEP]
side_effect: W3
determinism: D0
cli: [fid align adopt-alignment]
mcp: [align_adopt_alignment]
journal: [align.adopt_alignment@1]
updated: 2026-11-16
---

# ALN.MODEL.ADOPT: Agent provenance and human adoption of alignments

## Purpose

Implements CONSTITUTION C5 for alignments: an alignment created or changed by an agent principal is `proposed`, results depending on it cannot be released, and only a human principal can adopt it. Adoption is classed W3 (it gates release of controlled records), so on MCP it is projected only as a human-only request and agents can never complete it.

## Scope

**In:**
- Provenance state machine: `human`, `proposed`, `adopted`
- Tagging on every `align.*` W2 command by principal kind
- `align.adopt_alignment@1` and its checks
- The `release_block` query that release commands (TRUST, RPT) call for a result

**Out** (and which node owns it instead):
- Principals, authentication and the MCP request queue for W3 (`CORE.BUS`, `AUTO.MCP`)
- The release command itself (`TRUST`, `RPT`)
- Other release blockers such as `not_converged` (set by solvers; reported through the same query)

## Requirements

- **REQ-ALN.MODEL.ADOPT-001** When an agent principal's command creates or modifies an alignment, the system shall set its provenance state to `proposed` with the agent principal ID and session ID.
- **REQ-ALN.MODEL.ADOPT-002** When a human principal's command creates an alignment, the state shall be `human`; when a human modifies a `proposed` alignment, the state shall stay `proposed`.
- **REQ-ALN.MODEL.ADOPT-003** When `release_block(result)` is queried, the system shall return every alignment in the result's dependency closure whose state is `proposed` or whose status is `not_converged`, `ambiguous` or `failed`, and an empty list only when none exists.
- **REQ-ALN.MODEL.ADOPT-004** When a human principal runs `align.adopt_alignment` with the alignment ID and the expected content digest, the system shall set state `adopted` and record adopter ID, time and adopted digest.
- **REQ-ALN.MODEL.ADOPT-005** If the issuing principal is an agent (including the authoring agent), then the adopt command shall fail with `adopter_not_human` and journal nothing.
- **REQ-ALN.MODEL.ADOPT-006** If the expected digest differs from the alignment's current digest, then adoption shall fail with `digest_mismatch` and journal nothing.
- **REQ-ALN.MODEL.ADOPT-007** When an agent modifies an `adopted` alignment, the state shall return to `proposed`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/model/chain3` and an agent principal `agent-1`, when `agent-1` runs a stub solver command creating A3, then A3's state is `proposed` and `release_block` of a result evaluated in A3 lists A3.
- **AC-2** Given AC-1, when human principal `qa-1` runs `fid align adopt-alignment A3 --expected-digest <d>`, then A3 is `adopted`, `release_block` is empty, and one `align.adopt_alignment@1` event records `qa-1`.
- **AC-3** (error path) Given AC-1, when `agent-1` (or any agent) issues the adopt command, then it fails with `adopter_not_human` and journals nothing.
- **AC-4** (error path) Given AC-1, when `qa-1` adopts with a stale digest, then it fails with `digest_mismatch`.
- **AC-5** Given AC-2, when `agent-1` edits A3's parameters, then A3 is `proposed` again.
- **AC-6** Given the MCP projection, then `align_adopt_alignment` called by an agent produces only a pending human request and never changes state.

## Design

- **Registry command:** `align.adopt_alignment@1` (W3; params `alignment: AlignmentId`, `expected_digest: Sha256`, optional `note`); result `{ state, adopted_digest }`. MCP projection is the W3 human-only request form (CONSTITUTION §3).
- **Data structures:** `Provenance { principal, kind, session, state: Human | Proposed | Adopted { by, at, digest } }` (stored on the element defined in `ALN.MODEL.ELEM`).
- **Algorithm:** a command-bus post-hook on all `align.*` W2 commands sets the state from the principal kind; `release_block` walks the CORE.DEP closure of the result in stable-ID order.
- **Errors:** `adopter_not_human`, `digest_mismatch`, `not_found`.
- **Determinism:** D0.
- **Performance budget:** `release_block` over a 10,000-element closure in under 20 ms [unverified until measured].
- **Crate:** `fid-align` (provisional name).

## Test fixtures

`fixtures/aln/model/chain3` (see `ALN.MODEL.ELEM`) plus two test principals: `agent-1` (kind agent) and `qa-1` (kind human).

## Build prompt

> In crate `fid-align`, implement alignment provenance and adoption: a command-bus post-hook on every `align.*` W2 command that sets provenance state (agent principal gives `proposed`; human gives `human`; a human edit of a proposed alignment keeps `proposed`; an agent edit of an adopted alignment gives `proposed`), the query `release_block(result) -> Vec<Blocker>` over the result's dependency closure (proposed state, or status not_converged, ambiguous or failed), and the command `align.adopt_alignment@1` (class W3; CLI `fid align adopt-alignment <id> --expected-digest <sha256> [--json]`; MCP `align_adopt_alignment` projected as a human-only request; journal `align.adopt_alignment@1`).
> Requirements: only human principals may adopt (`adopter_not_human` otherwise, nothing journaled); the expected digest must equal the current digest (`digest_mismatch`); adoption records adopter, time and digest.
> Acceptance: fixture `fixtures/aln/model/chain3` with principals `agent-1` and `qa-1`; tests for proposed tagging, adoption clearing the release block, agent adoption refused, stale digest refused, agent edit re-proposing, MCP call producing only a pending request.
> Constraints: CONSTITUTION C5 (humans adopt agent work; agents never adopt their own proposals) and §3 (agents never perform W3). No panic, no unwrap/expect, no unsafe; no std HashMap/HashSet.

## Open questions

- **Revise for D-036/D-037 (accepted 2026-11-17):** an adoption command is now an item of the H2 batched release-and-adoption queue (AGENT.GATEPREP), not a separate stop; keep it `human_only`.
- Classing adoption as W3 is this leaf's reading of CONSTITUTION §3 (adoption gates release of controlled records). The parent listed adopt without an MCP pattern; the W3 projection keeps it out of agents' reach while still letting an agent ask a human. Reviewers should confirm.
- This leaf takes the principal kind (human or agent) as CORE.BUS supplies it. How a human at the CLI or a Python script is classed is open in D-012; this leaf does not depend on the outcome, but its test principals must follow whatever D-012 decides.

## Children

_Leaf: none._
