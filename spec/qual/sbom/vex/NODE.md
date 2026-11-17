---
id: QUAL.SBOM.VEX
title: VEX statements, review and publication
level: 3
parent: QUAL.SBOM
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-QUAL.SBOM-005, REQ-QUAL.SBOM-006, REQ-QUAL.SBOM-007]
covers: []
depends_on: []
uses_artifacts: [ECO.REL.SIGN.SBOM]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.SBOM.VEX: VEX statements, review and publication

## Purpose

Records, for every dependency vulnerability finding, whether the shipped product is affected, in a machine-readable exploitability (VEX) document that is reviewed in PRs and published with each release. It also hands every `affected` finding to the vulnerability process so customers get one advisory stream. The scanner (`QUAL.SBOM.SCAN`) reads the statements this leaf defines.

## Scope

**In:**
- VEX source file `security/vex/statements.toml` (one statement per vulnerability ID and component) and its schema
- Statuses `not_affected`, `affected`, `fixed`, `under_investigation`; justification codes for `not_affected`; reviewer and date fields
- Published format: OpenVEX JSON [unverified version], one document per release, generated from the source file
- Check: every finding in a findings file has a statement; `not_affected` carries a justification code and a reviewer other than the PR author
- Hand-off: `sbom/affected.json` listing `affected` statements and their triage references; check that each `affected` statement older than 1 working day carries a triage reference
- The VEX library types used by `QUAL.SBOM.SCAN`

**Out** (and which node owns it instead):
- Finding vulnerabilities and the release gate (owned by `QUAL.SBOM.SCAN`)
- Triage, advisories and embargo (owned by `QUAL.VULN.TRIAGE` and `QUAL.VULN.ADVIS`)
- Signing the published document (owned by `ECO.REL.SIGN.CUSTODY` as a release artefact)

## Requirements

- **REQ-QUAL.SBOM.VEX-001** Each VEX statement shall hold vulnerability ID, component package URL, status, timestamp, author and, for `not_affected`, one justification code from a closed list.
- **REQ-QUAL.SBOM.VEX-002** If a finding in the supplied findings file has no VEX statement for its vulnerability ID and component, then the check shall fail and name the finding.
- **REQ-QUAL.SBOM.VEX-003** If a `not_affected` statement lacks a justification code or a reviewer, or its reviewer equals its author, then the check shall fail and name the statement.
- **REQ-QUAL.SBOM.VEX-004** When the publish step runs for a release, the tool shall write one VEX document covering every finding in that release's findings file, and the release workflow shall publish it next to the SBOMs.
- **REQ-QUAL.SBOM.VEX-005** If an `affected` statement is older than 1 working day at the check time and has no triage reference, then the check shall fail and name it [unverified target].
- **REQ-QUAL.SBOM.VEX-006** The tool shall write `sbom/affected.json` listing every `affected` statement with its vulnerability ID, component, first-affected time and triage reference.
- **REQ-QUAL.SBOM.VEX-007** Given the same source file, findings file and check time, the tool shall produce byte-identical outputs.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/sbom/vex/ok/` (`statements.toml` with one statement of each status, `findings.json` with 4 findings, check time `2027-03-02T09:00:00Z`), when `cargo xtask qual vex check --source statements.toml --findings findings.json --now 2027-03-02T09:00:00Z --out out/` runs, then it exits 0 and `out/sbom/affected.json` equals `expected-affected.json`.
- **AC-2** (error path) Given `uncovered/` (a finding with no statement), then it exits 1 naming the vulnerability ID and component.
- **AC-3** (error path) Given `self-review/` (`not_affected` whose reviewer equals its author), then it exits 1 naming the statement.
- **AC-4** (error path) Given `stale-affected/` (`affected` since Friday 10:00, check time Tuesday 11:00, no triage reference), then it exits 1; with check time Monday 09:00 it exits 0.
- **AC-5** Given AC-1 inputs, when `qual vex publish --release v1.4.0 ...` runs, then `out/vex/fid-v1.4.0.openvex.json` equals the golden file and validates against the pinned OpenVEX schema [unverified schema].
- **AC-6** (determinism) Given AC-1 and AC-5 run twice, then all outputs are byte-identical.

## Design

- **Registry command(s):** none; repository tools `cargo xtask qual vex check --source <toml> --findings <json> --now <rfc3339> --out <dir>` and `cargo xtask qual vex publish --release <tag> --source <toml> --findings <json> --out <dir>` (both W1, local output root).
- **Data structures:** `Statement { vuln_id, purl, status: NotAffected|Affected|Fixed|UnderInvestigation, justification: Option<Justification>, author, reviewer: Option<String>, timestamp, triage_ref: Option<String>, note: Untrusted<String> }`; `Justification` = the five OpenVEX justification codes [unverified list]; `Finding { vuln_id, purl, severity, source_db }` (shape shared with `QUAL.SBOM.SCAN`).
- **Algorithm:** parse; index statements by (vuln_id, purl); check coverage and review rules; working-day age counts Monday to Friday in UTC with no holiday calendar; publish by mapping to OpenVEX with the release tag as document ID suffix and the release commit time as document timestamp.
- **Errors:** `VEX-PARSE`, `VEX-UNCOVERED(vuln, purl)`, `VEX-REVIEW(statement)`, `VEX-TRIAGE-LATE(statement)`. Exit 0/1/2.
- **Determinism:** D0; the check time is an explicit argument, never the wall clock.
- **Performance budget:** under 1 s for 500 statements.
- **Crate:** `fid-xtask` (module `qual::vex`, also used as a library by `qual::vuln_scan`).

## Test fixtures

`fixtures/qual/sbom/vex/{ok,uncovered,self-review,stale-affected}/` with synthetic vulnerability IDs (`TEST-0001`…) and package URLs; golden OpenVEX output; CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::vex` with subcommands `qual vex check --source <statements.toml> --findings <findings.json> --now <rfc3339> --out <dir>` and `qual vex publish --release <tag> --source ... --findings ... --out <dir>`. Statement fields: vuln_id, purl, status (not_affected|affected|fixed|under_investigation), justification (closed OpenVEX list, required for not_affected), author, reviewer (required for not_affected, must differ from author), timestamp, triage_ref, note (untrusted text). Check: every finding (vuln_id, purl) has a statement; review rules; an `affected` statement older than 1 working day (Mon-Fri, UTC, no holidays) at `--now` must have triage_ref. Write `sbom/affected.json` (canonical JSON, sorted). Publish: one OpenVEX JSON document per release covering every finding, timestamp = release commit time passed in, never the wall clock. Expose the types as a library for `qual::vuln_scan`. Exit 0/1/2; no panics, deny unwrap/expect, BTreeMap only. Tests: AC-1..AC-6 of spec node QUAL.SBOM.VEX.

## Open questions

- OpenVEX versus CSAF VEX is a leaf-level choice made here as OpenVEX [unverified tool support]; `ECO.REL.SIGN.SBOM` lists both as candidates and should reference this leaf.
- The working-day rule ignores public holidays; maintainers may add a holiday file.

## Children

_Leaf: none._
