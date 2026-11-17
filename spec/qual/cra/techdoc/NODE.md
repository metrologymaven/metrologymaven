---
id: QUAL.CRA.TECHDOC
title: CRA technical-documentation index and citation check
level: 3
parent: QUAL.CRA
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.CRA-003, REQ-QUAL.CRA-008]
covers: []
depends_on: []
uses_artifacts: [QUAL.THREAT.REG, QUAL.SBOM.VEX, ECO.REL.SIGN.SBOM, QUAL.VULN.POLICY, QUAL.CRA.SUPPORT, QUAL.CRA.CLASS, QUAL.CRA.REPORT, QUAL.CRA.EXPORT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.CRA.TECHDOC: CRA technical-documentation index and citation check

## Purpose

Keeps the index of the technical documentation the Act asks for, as a data file whose every entry points to a published artefact by path and SHA-256, and fails CI when a target is missing or changed without the index being updated. It also runs the citation rule over every document in `compliance/cra/` and `compliance/export/`: legal acts are cited by designator and article number, and no legal text is quoted beyond article titles.

## Scope

**In:**
- `compliance/cra/techdoc-index.toml` schema `qual-cra-techdoc/1`: entries with topic (product description, risk assessment, SBOM, VEX, vulnerability handling, support period, classification, reporting procedure, export notes), path, SHA-256
- Index check: path exists in the release tree, digest matches, every required topic present
- Citation check over Markdown in the two folders: quoted passages (block quotes or text in quotation marks) longer than 12 words fail; references to legal acts must match the designator pattern list in `compliance/cra/designators.toml`

**Out** (and which node owns it instead):
- The documents themselves (owned by the nodes in `uses_artifacts`)
- Advisory and threat content rules (owned by `QUAL.VULN.ADVIS`, `QUAL.THREAT.REG`)

## Requirements

- **REQ-QUAL.CRA.TECHDOC-001** The index shall contain at least one entry for each required topic, each with a repository-relative path and SHA-256.
- **REQ-QUAL.CRA.TECHDOC-002** If an entry's path does not exist in the given release tree, or the file's SHA-256 differs from the entry, then the check shall fail and name the entry.
- **REQ-QUAL.CRA.TECHDOC-003** If a required topic has no entry, then the check shall fail and name the topic.
- **REQ-QUAL.CRA.TECHDOC-004** If a Markdown file in `compliance/cra/` or `compliance/export/` contains a block quote or a quotation-marked passage longer than 12 words, then the citation check shall fail and name the file and line.
- **REQ-QUAL.CRA.TECHDOC-005** If such a file names a legal act in a form that matches none of the patterns in `designators.toml`, then the citation check shall warn and name the file and line.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/cra/techdoc/ok/` (release tree and index with 9 topics), when `cargo xtask qual cra techdoc --index techdoc-index.toml --tree tree/` runs, then it exits 0.
- **AC-2** (error path) Given `stale/` (the VEX file changed after indexing), then it exits 1 naming the VEX entry.
- **AC-3** (error path) Given `missing-topic/` (no `reporting_procedure` entry), then it exits 1 naming the topic.
- **AC-4** (error path) Given `quote/` (a 20-word quoted passage in `SUPPORT.md`), when `qual cra cite-check --dirs compliance/cra compliance/export` runs, then it exits 1 naming the file and line.

## Design

- **Registry command(s):** none; repository tools `cargo xtask qual cra techdoc --index <toml> --tree <dir>` and `cargo xtask qual cra cite-check --dirs <dir>...` (both R0).
- **Data structures:** `Entry { topic: Topic, path, sha256 }`; closed `Topic` enum; `designators.toml` regex list.
- **Algorithm:** stream-hash each target; quotation detection counts words between matching straight or typographic quotes and inside `>` blocks.
- **Errors:** `TECHDOC-MISSING(entry)`, `TECHDOC-DIGEST(entry)`, `TECHDOC-TOPIC(topic)`, `CITE-QUOTE(file, line)`.
- **Determinism:** D0.
- **Performance budget:** under 2 s.
- **Crate:** `fid-xtask` (module `qual::cra::techdoc`).

## Test fixtures

`fixtures/qual/cra/techdoc/{ok,stale,missing-topic,quote}/`; synthetic documents, CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::cra::techdoc` with `qual cra techdoc --index <toml> --tree <dir>` and `qual cra cite-check --dirs <dir>...`. Index schema `qual-cra-techdoc/1`: entries {topic, path, sha256}; topics (closed): product_description, risk_assessment, sbom, vex, vulnerability_handling, support_period, classification, reporting_procedure, export_notes. Fail on missing topic, missing path, digest mismatch. Citation check over Markdown: fail on block quotes or quotation-marked passages over 12 words; warn when a legal-act reference matches no pattern in `designators.toml`. No panics, deny unwrap/expect, bounded file reads. Tests: AC-1..AC-4 of spec node QUAL.CRA.TECHDOC.

## Open questions

- Which documents the Act requires in the technical documentation, and their mapping to these topics, need counsel review [unverified topic list].

## Children

_Leaf: none._
