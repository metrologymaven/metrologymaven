---
id: GDT.RES.ROWFLOW.ADOPT
title: Adopt signed-off rows into a released table
level: 4
parent: GDT.RES.ROWFLOW
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.RES.ROWFLOW-004, REQ-GDT.RES.ROWFLOW-005]
covers: []
depends_on: [GDT.RES.ROWFLOW.STATE, GDT.RES.SCHEMA, GDT.RES.LINT, CORE.BUS, CORE.JRN.BR]
side_effect: W3
determinism: D0
cli: [fid gdt adopt-dmt-row]
mcp: [gdt_adopt_dmt_row]
journal: [gdt.adopt_dmt_row@1]
updated: 2026-11-17
---

# GDT.RES.ROWFLOW.ADOPT: Adopt signed-off rows into a released table

## Purpose

Promotes one or more `signed_off` row revisions into a new released `DmtTable` version, the only way a row reaches the table the resolver uses. It is `human_only` and W3 (a released table is a controlled record): the bus refuses agents and scripts, and MCP can only file a request. Before committing it re-checks every per-row invariant, runs the release-mode lint over the resulting table, and records the new table version and digest so ResultRecords and the support matrix can cite it.

## Scope

**In:** `gdt.adopt_dmt_row@1` (W3, `human_only: true`); params: base table version, list of revision IDs, new table version; per-row checks; release-mode lint over the whole resulting table; new table file and digest; provenance chain (agent draft → human author → reviewers → adopter) recorded per row.
**Out:** dialect release gates (inventory completeness, ORS green at the release commit, owned by `GDT.RES.ROWASME.GATE`); signing a release package (owned by `TRUST`); support-matrix regeneration (owned by `GDT.RES.MATRIX`).

## Requirements

- **REQ-GDT.RES.ROWFLOW.ADOPT-001** The command shall be registered W3 with `human_only: true`, and the bus shall refuse agent and script principals.
- **REQ-GDT.RES.ROWFLOW.ADOPT-002** If any listed revision is not `signed_off`, was authored by an agent principal without a human re-proposal, or lacks an attestation for a cited edition, then the command shall fail naming the first such revision and journal nothing.
- **REQ-GDT.RES.ROWFLOW.ADOPT-003** The command shall build the new table from the base version plus the listed revisions (replacing rows with the same key), run `GDT.RES.LINT` in release mode over it, and fail with the findings if any exist.
- **REQ-GDT.RES.ROWFLOW.ADOPT-004** The new table version shall be strictly greater than the base version, and each adopted row's review state shall be `signed_off{reviewers}` as STATE maps it.
- **REQ-GDT.RES.ROWFLOW.ADOPT-005** A committed adoption shall journal one `gdt.adopt_dmt_row@1` event with adopter principal, base and new versions, new table digest, and per row the key, revision, content digest, author, reviewers and UTC timestamp.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/rowflow/adopt_ok.json` (base table v1.0.0, two signed-off revisions), when a human session runs `fid gdt adopt-dmt-row --base 1.0.0 --rows <r1>,<r2> --version 1.1.0 --json`, then table v1.1.0 is written with both rows `signed_off`, its digest is returned and one journal event is appended.
- **AC-2** (error path) Given an MCP agent channel, then a pending human request is created and no table is written.
- **AC-3** (error path) Given one revision still `under_review`, then the command fails naming it and journals nothing.
- **AC-4** (error path) Given a resulting table where another row fails lint L5, then the findings are returned and nothing is written.
- **AC-5** (error path) Given `--version 1.0.0` equal to the base, then `version_not_increased`.
- **AC-6** (replay) Exact replay of AC-1 gives `IDENTICAL` and the same table digest.

## Design

- **Registry command(s):** `gdt.adopt_dmt_row@1`, W3, `human_only: true` (MCP request-only). Params: `base: TableVersion`, `revisions: [RevisionId]`, `version: SemVer`. Result: `{ version, table_digest }`. `--json`, `--dry-run` (checks and lint only).
- **Data structures:** `AdoptionRecord { key, revision, content_digest, author, derived_from, reviewers }`.
- **Algorithm:** load base → check each revision → merge (BTreeMap by key) → lint release mode → write table → commit.
- **Errors:** `not_signed_off{revision}`, `agent_authored_revision{revision}`, `attestation_missing{revision}`, lint findings, `version_not_increased`, `unknown_revision`.
- **Determinism:** D0.
- **Performance budget:** adopting 50 rows into a 5 000-row table < 300 ms [unverified].
- **Crate:** `fid-gdt` (module `dmt_flow::cmd`).

## Test fixtures

Synthetic base tables and signed-off revisions (CC0, in-repo).

## Build prompt

> In crate `fid-gdt`, module `dmt_flow::cmd`, implement registry command `gdt.adopt_dmt_row@1` (W3, `human_only: true`; CLI `fid gdt adopt-dmt-row`; MCP `gdt_adopt_dmt_row` projected request-only; one journal event; `--json`, `--dry-run`). Params: base table version, revision IDs, new version. Check each revision is `signed_off`, human-authored (agent drafts only via a human re-proposal) and fully attested; merge into the base table by key; run the `GDT.RES.LINT` release mode over the result and fail with findings; require version > base; write the new `DmtTable` with each row's `ReviewState::signed_off{reviewers}`; journal adopter, versions, table digest and per-row key, revision, content digest, author, reviewers, UTC time. Tests per the acceptance criteria. Constraints: CONSTITUTION §3 (W3 and human-only: agents never adopt, C5), §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`, never store standards text.

## Open questions

- Class W3 rests on the released table being a controlled record (parent open question).

## Children

_Leaf: none._
