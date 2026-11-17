---
id: QUAL.EVID.LAYOUT
title: Evidence bundle layout and release-evidence statement
level: 3
parent: QUAL.EVID
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.EVID-001, REQ-QUAL.EVID-005, REQ-QUAL.EVID-006, REQ-QUAL.EVID-008]
covers: []
depends_on: []
uses_artifacts: [TRUST.SIGN.DSSE, ECO.REL.SIGN.CUSTODY]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.EVID.LAYOUT: Evidence bundle layout and release-evidence statement

## Purpose

Defines the public, versioned shape of the per-release evidence bundle: the folder layout, the closed list of member kinds, and the top-level release-evidence statement that lists every member with its SHA-256 and is the payload `ECO.REL.SIGN.CUSTODY` signs as a DSSE envelope after the human release approval. It is a small library plus a JSON Schema, so the assembler, IQ on the customer's machine, the matrix check and third parties all read the same contract.

## Scope

**In:**
- Bundle layout `evidence/<release>/` with one folder per member kind and `statement.json` at the root
- Member kinds (closed): `kernel_manifest`, `validation_report`, `ors_stats`, `replay_corpus`, `xart_report`, `rdl_release`, `sbom`, `vex`, `vuln_gate`, `change_impact`, `anomalies`
- Statement schema `qual-evidence-statement/1` (JSON Schema 2020-12): release version, channel, commit, members (kind, path, SHA-256, size, `published` flag, `omission_reason`)
- DSSE payload type `application/vnd.fid.release-evidence+json` (working name until D-002)
- Rule for restricted members (REQ-QUAL.EVID-008): `published = false`, digest and pass status kept, file not in the public bundle, reason stated
- Canonical serialisation (RFC 8785), members sorted by kind then path
- An offline verification guide section pointing to `ECO.REL.SIGN.GUIDE` steps

**Out** (and which node owns it instead):
- Collecting members and the completeness gate (owned by `QUAL.EVID.ASSEMBLE`)
- Signing (owned by `ECO.REL.SIGN.CUSTODY`)
- DSSE envelope format (owned by `TRUST.SIGN.DSSE`)

## Requirements

- **REQ-QUAL.EVID.LAYOUT-001** The statement shall hold the release version, channel, commit and, for every member, its kind, bundle path, SHA-256, size in bytes and `published` flag.
- **REQ-QUAL.EVID.LAYOUT-002** The serialiser shall emit the statement as RFC 8785 canonical JSON with members sorted by kind then path, so equal inputs give byte-identical statements.
- **REQ-QUAL.EVID.LAYOUT-003** If a member has `published = false`, then it shall carry a non-empty `omission_reason` and a `pass_status`, and the validator shall reject it otherwise.
- **REQ-QUAL.EVID.LAYOUT-004** The library shall validate a statement against the schema and reject unknown member kinds, duplicate paths and paths that leave the bundle folder.
- **REQ-QUAL.EVID.LAYOUT-005** The schema shall name the DSSE payload type under which `ECO.REL.SIGN.CUSTODY` signs the statement, and a golden envelope shall verify offline with the published release test key.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/evid/layout/members.json` (11 members, one restricted), when the serialiser runs twice with members in different orders, then both outputs are byte-identical and equal `statement.golden.json`.
- **AC-2** (error path) Given a restricted member without `omission_reason`, then validation fails with `EVID-RESTRICTED(path)`.
- **AC-3** (error path) Given a member path `../x`, then validation fails with `EVID-PATH`.
- **AC-4** Given `statement.golden.json` and the DSSE test envelope `statement.golden.dsse.json` signed with `fixtures/dsse/keys/ed25519-test.pem`, when verified with a DSSE v1.0 verifier offline, then it is valid.

## Design

- **Registry command(s):** none; library `qual::evid::layout` (R1, pure) and `schemas/qual-evidence-statement-1.json`.
- **Data structures:** `Statement { schema, release, channel, commit, members: Vec<Member> }`; `Member { kind: MemberKind, path: BundlePath, sha256, size, published, pass_status: Option<Pass|Fail>, omission_reason: Option<String> }`.
- **Algorithm:** validate, sort, canonicalise.
- **Errors:** `EVID-SCHEMA`, `EVID-KIND`, `EVID-DUP(path)`, `EVID-PATH(path)`, `EVID-RESTRICTED(path)`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-xtask` (module `qual::evid::layout`), plus the schema file.

## Test fixtures

`fixtures/qual/evid/layout/` synthetic members and the shared DSSE test key from `TRUST.SIGN.DSSE` fixtures; CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::evid::layout` and `schemas/qual-evidence-statement-1.json` (JSON Schema 2020-12). Types: Statement {schema "qual-evidence-statement/1", release, channel, commit, members}; Member {kind (closed: kernel_manifest, validation_report, ors_stats, replay_corpus, xart_report, rdl_release, sbom, vex, vuln_gate, change_impact, anomalies), path (relative, no `..`, inside the bundle), sha256, size, published, pass_status, omission_reason}. Restricted members (published false) need omission_reason and pass_status. Serialise as RFC 8785 canonical JSON, members sorted by kind then path. Document DSSE payload type `application/vnd.fid.release-evidence+json`; add a golden envelope signed with the DSSE Ed25519 test key and a test that verifies it offline. No panics, deny unwrap/expect. Tests: AC-1..AC-4 of spec node QUAL.EVID.LAYOUT.

## Open questions

- Build provenance (SLSA) and the MCP tool-definition manifest digest are candidate member kinds once `ECO.REL.REPRO` is specified; adding a kind is a minor schema version.

## Children

_Leaf: none._
