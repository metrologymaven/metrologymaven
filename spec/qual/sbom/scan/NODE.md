---
id: QUAL.SBOM.SCAN
title: Dependency vulnerability scan and release gate
level: 3
parent: QUAL.SBOM
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-QUAL.SBOM-003, REQ-QUAL.SBOM-004, REQ-QUAL.SBOM-008]
covers: []
depends_on: [QUAL.SBOM.VEX]
uses_artifacts: [ECO.REL.SIGN.SBOM, ECO.CI.WF.CALL, ECO.REL.PROC.GATES]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.SBOM.SCAN: Dependency vulnerability scan and release gate

## Purpose

Matches every SBOM component against an offline mirror of a public advisory database on PRs that change a lockfile, nightly and on every release candidate, and writes the vulnerability gate file that `ECO.REL.PROC.GATES` reads. The gate fails a release with a critical finding not cleared by VEX, and fails rather than passes when the database snapshot is stale.

## Scope

**In:**
- Matcher from SPDX package URLs and versions to advisory affected ranges in an OSV-format database snapshot [unverified format coverage]
- Database snapshot metadata: source, snapshot time, digest
- Modes `pr`, `nightly`, `release` and their triggers, as a workflow file calling ECO's reusable workflows
- Severity banding: critical when CVSS base score is 9.0 or higher, or the database's own critical band when no score is given
- Gate file `sbom/vuln-gate.json` and findings file `sbom/findings.json`
- Freshness rule: release mode fails when the snapshot is older than 24 hours at the given check time or cannot be loaded

**Out** (and which node owns it instead):
- VEX statements and their review (owned by `QUAL.SBOM.VEX`)
- SBOM completeness (owned by `QUAL.SBOM.COMPLETE`)
- Folding the gate into the release verdict (owned by `ECO.REL.PROC.GATES`)
- Mirroring the database into CI (CI infrastructure in `ECO.CI.WF`; this leaf only reads the snapshot directory)

## Requirements

- **REQ-QUAL.SBOM.SCAN-001** The workflow file shall run the scan in `pr` mode on every PR that changes a `Cargo.lock` or `native-components.toml`, in `nightly` mode once a day on the default branch, and in `release` mode on every release candidate.
- **REQ-QUAL.SBOM.SCAN-002** When the scan runs, the tool shall report every component whose version falls in an advisory's affected range, with vulnerability ID, aliases, component package URL, severity band and score.
- **REQ-QUAL.SBOM.SCAN-003** If, in `release` mode, any finding has band `critical` and no VEX statement with status `not_affected` or `fixed`, then the gate file shall have verdict `block` and name each such finding.
- **REQ-QUAL.SBOM.SCAN-004** If, in `release` mode, the database snapshot cannot be loaded or its snapshot time is more than 24 hours before the check time, then the gate verdict shall be `block` with reason `db_stale` or `db_unavailable`.
- **REQ-QUAL.SBOM.SCAN-005** In `pr` and `nightly` modes, the tool shall exit non-zero on a new critical finding without a clearing VEX statement, and shall never write a release gate file.
- **REQ-QUAL.SBOM.SCAN-006** The gate file shall record the release candidate, the database source, snapshot time and SHA-256, the SBOM digests scanned and the VEX source digest.
- **REQ-QUAL.SBOM.SCAN-007** The tool shall make no network request; it shall read the database only from the given snapshot directory.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/sbom/scan/clean/` (2 SBOMs, a 3-advisory synthetic OSV snapshot taken 2027-03-02T06:00Z, VEX file), when `cargo xtask qual vuln-scan --mode release --rc v1.4.0-rc.1 --inputs clean/ --now 2027-03-02T09:00:00Z --out out/` runs, then it exits 0 and `out/sbom/vuln-gate.json` has verdict `pass` and equals the golden file.
- **AC-2** (error path) Given `critical-open/` (component in the range of a CVSS 9.8 advisory, VEX `under_investigation`), then verdict is `block` naming the finding; after changing the statement to `not_affected` with justification and reviewer, verdict is `pass`.
- **AC-3** (error path) Given `clean/` with `--now 2027-03-03T07:00:00Z` (25 h after snapshot), then verdict is `block` with reason `db_stale`.
- **AC-4** (error path) Given an empty snapshot directory, then verdict is `block` with reason `db_unavailable`.
- **AC-5** Given `critical-open/` in `--mode pr`, then the tool exits 1 and no `vuln-gate.json` is written.
- **AC-6** Given the `ECO.REL.PROC.GATES` fixture adapter for `qual-sbom`, when it reads AC-1's and AC-2's gate files, then it reports `pass` and `block` respectively.
- **AC-7** (offline) Given AC-1 run in a sandbox without network, then it passes identically.

## Design

- **Registry command(s):** none; repository tool `cargo xtask qual vuln-scan --mode pr|nightly|release [--rc <tag>] --inputs <dir> --now <rfc3339> --out <dir> [--json]` (W1, local output root). Workflow `ci/qual/vuln-scan.yml` calls the reusable workflows of `ECO.CI.WF.CALL`.
- **Data structures:** `SnapshotMeta { source, snapshot_time, sha256 }` from `db/meta.json`; `Finding` (shared with `qual::vex`); `GateFile { schema: "qual-sbom-gate/1", rc, verdict: pass|block, reasons, findings, db: SnapshotMeta, sbom_sha256: Vec, vex_sha256 }`.
- **Algorithm:** load SBOM packages (purl, version); load advisories, index by ecosystem and package name; evaluate semver and ecosystem ranges per OSV rules [unverified range semantics]; band severity; join with VEX; apply mode rules; write canonical JSON sorted by (vuln_id, purl).
- **Errors:** `SCAN-SBOM-PARSE`, `SCAN-DB-UNAVAILABLE`, `SCAN-DB-STALE`, `SCAN-RANGE(advisory)` (unparsable range counts as affected and is reported). Exit 0 pass, 1 block or new critical, 2 bad config.
- **Determinism:** D0; check time explicit.
- **Performance budget:** under 30 s for 5,000 components against a full-size snapshot [unverified].
- **Crate:** `fid-xtask` (module `qual::vuln_scan`).

## Test fixtures

`fixtures/qual/sbom/scan/{clean,critical-open}/` with synthetic OSV advisories (`TEST-` IDs, invented package names) and SPDX documents; CC-BY-4.0. No real advisory data is committed.

## Build prompt

> In `fid-xtask`, add module `qual::vuln_scan`: `qual vuln-scan --mode pr|nightly|release [--rc <tag>] --inputs <dir> --now <rfc3339> --out <dir>`. Inputs: SPDX 2.3 SBOMs, an OSV-format advisory snapshot directory with `db/meta.json` (source, snapshot_time, sha256), and the VEX source (`qual::vex` types). Match components by purl ecosystem, name and version ranges; band severity (critical = CVSS base ≥ 9.0, else the database's critical label). Release mode writes `sbom/vuln-gate.json` (canonical JSON, schema `qual-sbom-gate/1`) with verdict block when any critical finding lacks VEX `not_affected` or `fixed`, or when the snapshot is missing or older than 24 h at `--now`; also write `sbom/findings.json`. PR and nightly modes exit 1 on uncleared critical findings and never write the gate file. No network at all. Add `ci/qual/vuln-scan.yml` triggering on lockfile or native-component changes, nightly, and release candidates, using ECO's reusable workflows. No panics, deny unwrap/expect, BTreeMap only. Tests: AC-1..AC-7 of spec node QUAL.SBOM.SCAN.

## Open questions

- The advisory database (for example the OSV data for crates.io) and how CI mirrors it for air-gapped builds are unverified; mirroring is CI infrastructure (`ECO.CI.WF`).
- `ECO.REL.PROC.GATES` proposes `sbom/vuln-gate.json`; this leaf adopts that path.

## Children

_Leaf: none._
