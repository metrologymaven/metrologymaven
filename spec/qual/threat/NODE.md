---
id: QUAL.THREAT
title: Threat model and security architecture review
level: 2
parent: QUAL
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-QUAL-003]
covers: []
depends_on: []
uses_artifacts: [ECO.REL.PROC.GATES, ECO.REL.PROC.NOTES, CORE.REG.MAN, ECO.CI.GATES, QUAL.VULN.ADVIS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.THREAT: Threat model and security architecture review

## Purpose

Keeps a written, versioned threat model for the whole platform and runs the security architecture review at every minor release. The model names assets, trust boundaries, threats and the controls that answer them, and links each control to the spec requirements that implement it, so a reviewer can see what is covered and what is not. It is a document and a review process, not code; it changes when the architecture changes, independently of how vulnerabilities are reported or how dependencies are scanned.

## Scope

**In:**
- Threat model document and a machine-readable threat register (threat ID, boundary, vector, controls, requirement IDs, residual risk, owner)
- Mandatory coverage areas: MCP servers and tool projection, the loopback control channel, plugins and WASM extensions, driver hosts, file and CAD import, motion and the approval path, signing keys, the build and supply chain, the journal and local admin tampering, export-controlled data
- Per-minor-release review: a review record listing changed areas, new threats, closed threats and the reviewer
- Trigger rules for an out-of-cycle review (new trust boundary, new network listener, new side-effect class use)
- CI check that every control in the register cites existing requirement IDs

**Out** (and which node owns it instead):
- Implementing the controls (owned by the domains named in each register row, for example `AUTO.MCP`, `AUTO.CTRL`, `HAL`, `TRUST`, `CORE.BUS`)
- Vulnerability reporting and advisories (owned by `QUAL.VULN`)
- Dependency scanning and SBOM (owned by `QUAL.SBOM`)
- Penetration testing services (not offered by the project)

## Requirements

- **REQ-QUAL.THREAT-001** The threat register shall contain at least one threat entry for each mandatory coverage area listed in this node's scope.
- **REQ-QUAL.THREAT-002** Every threat entry shall name its trust boundary, vector, at least one control, the spec requirement IDs implementing each control and a residual-risk rating of `low`, `medium` or `high`.
- **REQ-QUAL.THREAT-003** If a register entry cites a requirement ID that does not exist in the spec tree, then the register check shall fail and name the entry.
- **REQ-QUAL.THREAT-004** Before each minor release is tagged, a review record for that minor version shall exist, naming the reviewer, the areas changed since the previous review and each threat added, changed or closed.
- **REQ-QUAL.THREAT-005** When a PR adds a trust boundary, a network listener or a new command of class M1, S1, W3, X1 or A0, the review gate shall require a threat-register update in the same PR or a linked issue.
- **REQ-QUAL.THREAT-006** Every `high` residual-risk entry shall link an open issue with an owner, and the release notes shall list the number of `high` entries.
- **REQ-QUAL.THREAT-007** The published threat model shall omit exploit detail for unfixed vulnerabilities and shall reference `QUAL.VULN` advisories instead.

## Interfaces

- **Offers:** threat model document; threat register data file; per-minor review record; register check and review-trigger rule (run by `ECO.CI`).
- **Consumes (as artifacts):** the `qual-threat` gate and `--pre-tag` mode (`ECO.REL.PROC.GATES`), which read `threat/reviews/<MAJOR.MINOR>.toml`; release notes (`ECO.REL.PROC.NOTES`), which read the `high` count; the Registry Manifest (`CORE.REG.MAN`) for command classes; gate selection (`ECO.CI.GATES`); the published advisory feed (`QUAL.VULN.ADVIS`); the spec tree for requirement ID resolution.

## Decomposition hints

MODEL (document and register schema), REVIEW (per-minor review record and gate), TRIGGER (PR-level change triggers), CHECK (requirement-ID resolution).

## Split rationale

The split follows the three moments the threat model is touched: continuously as data with a CI check (REG), once per minor release as a signed-off review (REVIEW), and on any PR that changes the architecture (TRIGGER). Each has a different input (the register, two register versions, a PR diff) and a different consumer (CI, the release gate, PR review).

Alternatives considered:
- **Separate MODEL and CHECK leaves** (the hints). Rejected: the check is the schema's validator; splitting them would put one file format in two leaves.
- **Folding TRIGGER into REG.** Rejected: TRIGGER reads Registry Manifests and PR events, a different input set and failure mode.

## Open questions

- Resolved: the minor-release gate exists as `ECO.REL.PROC.GATES` gate `qual-threat` and runbook step `threat-review-minor` (`ECO.REL.PROC.RUNBOOK`); `QUAL.THREAT.REVIEW` adopts its proposed path.
- Resolved: the reviewer is a maintainer other than the author (D-003).
- The concept doc's threat table (01 §10.2) needs re-checking against the current spec before import [unverified currency].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| QUAL.THREAT.REG | Threat model document, register schema and register check | 001 002 003 006 007 | none |
| QUAL.THREAT.REVIEW | Per-minor-release threat-model review record | 004 | none |
| QUAL.THREAT.TRIGGER | PR-level threat-model review trigger | 005 | none |
