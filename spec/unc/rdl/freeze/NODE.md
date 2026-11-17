---
id: UNC.RDL.FREEZE
title: Frozen RDL release sets and suite digest
level: 3
parent: UNC.RDL
kind: feature
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL-005]
covers: []
depends_on: [UNC.RDL.FORMAT, CORE]
side_effect: W3
determinism: D0
cli: [fid rdl freeze-release, fid rdl show-release]
mcp: [rdl_freeze_release, rdl_show_release]
journal: [rdl.freeze_release@1, rdl.show_release@1]
updated: 2026-11-16
---

# UNC.RDL.FREEZE: Frozen RDL release sets and suite digest

## Purpose

Freezes a named, versioned set of RDL datasets into an immutable release whose digest (`rdl_suite_digest`) enters the kernel manifest. The frozen set defines what "bit-identical on the reference set" means for kernel patch releases and for the native-versus-WASM gate. Freezing is a controlled-record action done by a human maintainer.

## Scope

**In:**
- `RdlRelease { name: rdl-release-<semver>, datasets: [(id, digest)] sorted by ID, created_by, created_at, suite_digest }`
- `rdl.freeze_release@1` (W3): from a selection file to an immutable release
- `rdl.show_release@1` (R0): list members and digest; emit the digest for `KRN.MAN`

**Out:**
- Recording the digest in the kernel manifest (owned by `KRN.MAN`)
- Running the release set (owned by `UNC.RDL.AST`)
- Signing the release (owned by `TRUST`)

## Requirements

- **REQ-UNC.RDL.FREEZE-001** When `rdl.freeze_release@1` runs with a release name and a list of dataset IDs, the system shall resolve each ID to its current digest, sort members by ID, compute `suite_digest` as SHA-256 over the RFC 8785 canonical member list and name, and store the release immutably.
- **REQ-UNC.RDL.FREEZE-002** If a release with the same name exists, then freezing shall fail with `release_exists`, and no release shall ever be modified after it is stored.
- **REQ-UNC.RDL.FREEZE-003** If any listed dataset is missing, fails validation, or is tier B, then freezing shall fail with a typed error naming it and store nothing.
- **REQ-UNC.RDL.FREEZE-004** If the principal is an agent, then `rdl.freeze_release@1` shall not execute and shall at most file a human-approval request, as for every W3 command.
- **REQ-UNC.RDL.FREEZE-005** The `suite_digest` shall be identical on every platform for the same release.

## Acceptance criteria

- **AC-1** Given the 5 records in `fixtures/rdl/freeze/members/` and selection file `fixtures/rdl/freeze/sel_v0.txt`, when `fid rdl freeze-release --name rdl-release-0.1.0 --select <handle> --json` runs as a human maintainer, then the release is stored and `suite_digest` equals `fixtures/rdl/freeze/expected_digest.txt` on wasm32, x86-64 and ARM64.
- **AC-2** (order) Given the same selection file with lines shuffled, when frozen under a new name, then the member list (excluding name) is identical and so is the digest of the member list.
- **AC-3** (error path) Given AC-1 again with the same name, then it fails with `release_exists` and the stored release is unchanged.
- **AC-4** (error path) Given a selection containing a tier B ID or an unknown ID, then it fails naming that ID and stores nothing.
- **AC-5** (agent) Given an MCP call from an agent principal, then no release is created and the result is a pending human-approval request.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `rdl.freeze_release@1` (W3; `name: String` matching `rdl-release-<semver>`, `select: ImportHandle`; returns `{ name, suite_digest, count }`), `rdl.show_release@1` (R0; `name`; returns the release).
- **Data structures:** as in Scope; stored append-only in the RDL store.
- **Algorithm:** read selection (bounded, one ID per line, ≤ 100,000 lines), dedupe, resolve, validate, sort, canonicalize, digest, store.
- **Errors:** `release_exists`, `dataset_not_found{id}`, `blind_not_allowed{id}`, `invalid_name`, `selection_too_large`.
- **Determinism:** D0.
- **Performance budget:** freeze 10,000 members in under 1 s [unverified until measured].
- **Crate:** `fid-rdl`.

## Test fixtures

`fixtures/rdl/freeze/`: 5 small synthetic records, a selection file, a shuffled copy and the expected digest (CC-BY-4.0).

## Build prompt

> In crate `fid-rdl`, implement `rdl.freeze_release@1` (side-effect class W3: human-only; on MCP an agent call only files an approval request; CLI `fid rdl freeze-release`) and `rdl.show_release@1` (R0; `fid rdl show-release`), both journaled with command ID and result digest, with `--json`.
> Freeze: read the selection file through an import-root handle (≤ 100,000 IDs), dedupe, resolve each ID to its current dataset digest, reject missing (`dataset_not_found`), invalid or tier B (`blind_not_allowed`) entries, sort by ID, compute `suite_digest` = SHA-256 over RFC 8785 JSON of `{name, members}`; store immutably; fail `release_exists` if the name exists. Name must match `rdl-release-<semver>`. Show returns members and digest so `KRN.MAN` can record `rdl_suite_digest`.
> Tests with `fixtures/rdl/freeze/`: expected digest on wasm32, x86-64, ARM64; shuffled selection gives the same member list; duplicate name refused with no change; tier B and unknown IDs refused; agent call yields only an approval request; exact replay IDENTICAL. No panics, no `unwrap`.

## Open questions

- None.

## Children

_Leaf: none._
