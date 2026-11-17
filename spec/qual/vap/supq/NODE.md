---
id: QUAL.VAP.SUPQ
title: Supplier questionnaire and GAMP categorisation rationale
level: 3
parent: QUAL.VAP
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.VAP-001, REQ-QUAL.VAP-006]
covers: []
depends_on: [QUAL.VAP.TPL]
uses_artifacts: [QUAL.EVID.LAYOUT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.VAP.SUPQ: Supplier questionnaire and GAMP categorisation rationale

## Purpose

The project's own answers to the supplier questionnaire a regulated customer sends a software vendor, plus a GAMP 5 categorisation rationale [unverified category]. Answers state gaps plainly (for example, the project holds no ISO 9001 certification), and an answer may say `met` only with a link to evidence. A CI check enforces both.

## Scope

**In:**
- `vap/templates/supplier-questionnaire.json` (`vap-template/1`) with answer rows: question ID, project-written question, status `met|partial|gap|not_applicable`, answer text, evidence links (evidence-bundle member kinds or published document paths), gap reason
- GAMP categorisation rationale section
- Check of answer rules

**Out** (and which node owns it instead):
- Evidence content (owned by `QUAL.EVID`)

## Requirements

- **REQ-QUAL.VAP.SUPQ-001** If an answer has status `met` or `partial` without at least one evidence link, then the check shall fail and name the question.
- **REQ-QUAL.VAP.SUPQ-002** If an answer has status `gap` without a gap reason, then the check shall fail and name the question.
- **REQ-QUAL.VAP.SUPQ-003** If an evidence link names a member kind outside `qual-evidence-statement/1` or a document path absent from the given tree, then the check shall fail and name the question.
- **REQ-QUAL.VAP.SUPQ-004** The questionnaire shall include a question on quality-management certification answered `gap` with the reason that the project holds none, until that changes.

## Acceptance criteria

- **AC-1** Given the shipped questionnaire and fixture tree `fixtures/qual/vap/supq/tree/`, when `cargo xtask qual vap check-supq --tree tree/` runs, then it exits 0.
- **AC-2** (error path) Given an answer `met` with no evidence, then it exits 1 naming it.
- **AC-3** (error path) Given a `gap` without reason, then it exits 1.
- **AC-4** (error path) Given an evidence link to member kind `pentest_report`, then it exits 1.

## Design

- **Registry command(s):** none; repository check `cargo xtask qual vap check-supq --tree <dir>` (R0).
- **Data structures:** `Answer { id, question, status, text: Untrusted<String>, evidence: Vec<EvidenceLink{ member_kind | path }>, gap_reason }`.
- **Algorithm:** rule checks; member kinds from the `qual::evid::layout` enum shipped as JSON Schema.
- **Errors:** `SUPQ-EVIDENCE(q)`, `SUPQ-GAP(q)`, `SUPQ-LINK(q)`.
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `qual::vap::supq`).

## Test fixtures

`fixtures/qual/vap/supq/` synthetic tree and answer variants; CC-BY-4.0.

## Build prompt

> Write `vap/templates/supplier-questionnaire.json` (`vap-template/1`): project-written questions with status met|partial|gap|not_applicable, answer text, evidence links (evidence-statement member kinds or document paths) and gap reasons; include a GAMP 5 categorisation rationale [unverified category] and a quality-certification question answered `gap` (the project holds none). In `fid-xtask`, add `qual vap check-supq --tree <dir>`: fail when met/partial lacks evidence, gap lacks a reason, or a link names an unknown member kind (from schema `qual-evidence-statement/1`) or a missing path. No panics, deny unwrap/expect. Tests: AC-1..AC-4 of spec node QUAL.VAP.SUPQ.

## Open questions

- Whether to answer an industry-standard questionnaire layout is a maintainer choice; the project writes its own questions to avoid copying third-party forms.

## Children

_Leaf: none._
