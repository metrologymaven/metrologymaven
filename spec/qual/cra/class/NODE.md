---
id: QUAL.CRA.CLASS
title: CRA classification record and distribution-gate status
level: 3
parent: QUAL.CRA
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.CRA-002, REQ-QUAL.CRA-004]
covers: []
depends_on: []
uses_artifacts: [QUAL.CRA.SUPPORT, ECO.REL.PROC.GATES]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.CRA.CLASS: CRA classification record and distribution-gate status

## Purpose

Records the project's position under the EU Cyber Resilience Act (product class and the project's role, for example manufacturer or open-source steward) as fields that each carry a review status, and writes `cra/status.json`, the file `ECO.REL.PROC.GATES` gate `qual-cra` reads to disable EU distribution venues until both this record and the support statement are counsel-reviewed. It stores counsel's conclusions; it never draws them.

## Scope

**In:**
- `compliance/cra/classification.toml` schema `qual-cra-class/1`: fields `product_class` and `role`, each with value, review status `draft|counsel_reviewed`, reviewer reference and date; record-level `final` flag
- Rule: `final = true` only when every field is `counsel_reviewed`
- Status file `cra/status.json` (path proposed by `ECO.REL.PROC.GATES`, adopted here) holding support-statement and classification review statuses and digests

**Out** (and which node owns it instead):
- Support statement (owned by `QUAL.CRA.SUPPORT`)
- Disabling venues (owned by `ECO.REL.PROC.GATES`)
- The legal classification itself (counsel)

## Requirements

- **REQ-QUAL.CRA.CLASS-001** The classification record shall hold `product_class` and `role`, each with a value, a review status of `draft` or `counsel_reviewed`, and for `counsel_reviewed` a reviewer reference and date.
- **REQ-QUAL.CRA.CLASS-002** If the record has `final = true` while any field is `draft`, then the check shall fail.
- **REQ-QUAL.CRA.CLASS-003** The tool shall write `cra/status.json` with `support_status` and `classification_status`, each `counsel_reviewed` only when every entry of the respective file is `counsel_reviewed` (and, for classification, `final = true`), otherwise `draft`, plus the SHA-256 of both files.
- **REQ-QUAL.CRA.CLASS-004** If either input file is missing or unparsable, then the tool shall still write `cra/status.json` with that status `draft` and a reason, so the gate disables EU venues rather than failing open.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/cra/class/reviewed/` (both files fully reviewed, final), when `cargo xtask qual cra status --support support.toml --class classification.toml --out out/` runs, then `out/cra/status.json` has both statuses `counsel_reviewed` and equals the golden file.
- **AC-2** Given `draft/` (role still `draft`), then `classification_status` is `draft`.
- **AC-3** (error path) Given `bad-final/` (`final = true`, one field draft), then `qual cra check-class` exits 1.
- **AC-4** Given a missing `classification.toml`, then the status file has `classification_status = draft` with reason `missing`.
- **AC-5** (joint) Given AC-2's status file and the `ECO.REL.PROC.GATES` fixture `cra-draft/`, then the gate lists every `eu_distribution` venue as disabled and does not block.

## Design

- **Registry command(s):** none; repository tools `cargo xtask qual cra check-class --class <toml>` (R0) and `cargo xtask qual cra status --support <toml> --class <toml> --out <dir>` (W1).
- **Data structures:** `Field { value, status, reviewer_ref, reviewed_on }`; `Classification { product_class: Field, role: Field, final }`; `Status { schema: "qual-cra-status/1", support_status, classification_status, reasons, support_sha256, class_sha256 }`.
- **Algorithm:** parse both files (support with the `qual::cra::support` types); fold statuses; write canonical JSON.
- **Errors:** `CRA-CLASS-PARSE`, `CRA-CLASS-FINAL`. The status command never exits non-zero on content; it exits 2 only on output failure.
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `qual::cra::class`).

## Test fixtures

`fixtures/qual/cra/class/{reviewed,draft,bad-final}/`; synthetic values such as `class = "placeholder-a"`, never a real legal conclusion; CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::cra::class` with `qual cra check-class --class <toml>` and `qual cra status --support <toml> --class <toml> --out <dir>`. Classification schema `qual-cra-class/1`: product_class and role, each {value, status draft|counsel_reviewed, reviewer_ref, reviewed_on}, plus final. check-class fails when final is true with any draft field, or counsel_reviewed lacks reviewer_ref. status writes `cra/status.json` (schema `qual-cra-status/1`, canonical JSON): support_status and classification_status (counsel_reviewed only when every entry is, and classification is final), reasons, SHA-256 of both inputs; a missing or unparsable input yields `draft` with a reason, never an error exit (fail closed for EU venues). Reuse the support types of `qual::cra::support`. No panics, deny unwrap/expect. Tests: AC-1..AC-5 of spec node QUAL.CRA.CLASS.

## Open questions

- Counsel review is needed for the product class and the project's role before EU distribution; when counsel input exists, maintainers should record it here and, if a choice remains, open a decision.

## Children

_Leaf: none._
