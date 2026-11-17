---
id: QUAL.CRA
title: EU Cyber Resilience Act and export-control documentation
level: 2
parent: QUAL
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-QUAL-006]
covers: []
depends_on: []
uses_artifacts: [QUAL.VULN.TRIAGE, QUAL.VULN.POLICY, QUAL.SBOM.VEX, ECO.REL.SIGN.SBOM, QUAL.THREAT.REG, ECO.REL.PROC.GATES, ECO.REL.LTS, ECO.REL.PROC.CHANNEL, ECO.REL.PKG.PLAN, ECO.APPS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.CRA: EU Cyber Resilience Act and export-control documentation

## Purpose

The product-regulation paperwork that must exist before the software is distributed in the EU or abroad: the EU Cyber Resilience Act support period and security-update policy, the technical documentation and conformity material the Act asks of whoever places the product on the market, the reporting procedure for actively exploited vulnerabilities, and export-control classification notes plus the data-tag vocabulary for export-controlled customer data. Every legal classification here needs counsel; this node specifies what must be written and checked, not the legal conclusions.

## Scope

**In:**
- Support-period statement per product line and the security-update policy (how long, which versions, how delivered)
- A distribution gate: no EU distribution channel goes live before the statement exists and has counsel sign-off
- CRA technical documentation index: product description, risk assessment (from `QUAL.THREAT`), SBOM and VEX (from `QUAL.SBOM`), vulnerability handling (from `QUAL.VULN`), support period
- Product classification record (default, important or critical class; open-source steward versus manufacturer role) with counsel review status [unverified categories]
- Reporting procedure for actively exploited vulnerabilities and severe incidents, triggered by the `actively_exploited` flag
- Export-control classification note for the software (for example EAR and Wassenaar self-assessment) with counsel review status
- Data-tag vocabulary for export-controlled customer projects (for example `itar`, `ear`, `cui`) and the default-deny projection policy statement

**Out** (and which node owns it instead):
- Vulnerability triage and advisories (owned by `QUAL.VULN`)
- SBOM and VEX content (owned by `QUAL.SBOM`)
- Threat analysis (owned by `QUAL.THREAT`)
- Enforcing data tags in MCP projection (owned by `AUTO.MCP.PROJ`; see Open questions)
- Legal conclusions themselves (owned by counsel, outside the project's spec)

## Requirements

- **REQ-QUAL.CRA-001** The project shall publish, for each product line, a support period in months and a security-update policy naming which versions receive fixes and how fixes are delivered.
- **REQ-QUAL.CRA-002** If the support-period statement or the classification record lacks a recorded counsel review, then the release process shall refuse to enable any EU distribution channel.
- **REQ-QUAL.CRA-003** The project shall keep a CRA technical-documentation index whose every entry points to a published artefact by path and digest, and a CI check shall fail when an entry's target is missing.
- **REQ-QUAL.CRA-004** The classification record shall state the product class and the project's role under the Act, each with a review status of `draft` or `counsel_reviewed`, and shall not be marked final while any field is `draft`.
- **REQ-QUAL.CRA-005** When a vulnerability record's `actively_exploited` flag is set, the reporting procedure shall start a timer and produce a draft notification within the deadlines named in the procedure [unverified deadlines].
- **REQ-QUAL.CRA-006** The project shall publish an export-control classification note for each released binary with a review status, and shall state that customers remain responsible for their own export decisions.
- **REQ-QUAL.CRA-007** The project shall define a closed data-tag vocabulary for export-controlled projects, and the policy shall state that a tagged project's geometry, PMI, points and journals are not projected to MCP unless an A0 administrator enables an export-controlled profile.
- **REQ-QUAL.CRA-008** All documents in this node shall cite legal acts by designator and article number only, without quoted legal text beyond article titles.

## Interfaces

- **Offers:** support-period statement; security-update policy; CRA technical-documentation index; classification record; exploited-vulnerability reporting procedure; export-control notes; data-tag vocabulary and projection policy (to `AUTO.MCP.PROJ`, `CORE.PRJ`).
- **Consumes (as artifacts):** `vuln/exploited.json` (`QUAL.VULN.TRIAGE`); `SECURITY.md` (`QUAL.VULN.POLICY`); SBOMs and VEX (`ECO.REL.SIGN.SBOM`, `QUAL.SBOM.VEX`); the threat register (`QUAL.THREAT.REG`); LTS lines, channels, package plan and app list (`ECO.REL.LTS`, `ECO.REL.PROC.CHANNEL`, `ECO.REL.PKG.PLAN`, `ECO.APPS`). `ECO.REL.PROC.GATES` gate `qual-cra` reads `cra/status.json`.

## Decomposition hints

SUPPORT (support period and update policy), TECHDOC (documentation index and check), CLASS (classification record and review), REPORT (exploited-vulnerability reporting procedure), EXPORT (export-control notes), TAGS (data-tag vocabulary and policy).

## Split rationale

The split follows the separate documents the Act and export rules ask for, grouped by who reads them: the support statement (customers), the classification record and gate status (the release gate), the technical-documentation index (market surveillance), the exploited-vulnerability reporting tracker (the security team under a deadline), and export notes with the data-tag vocabulary (customers and the enforcing domains). SUPPORT and CLASS stay apart because the support statement is public product information while the classification is a legal position; CLASS writes the gate file from both.

Alternatives considered:
- **TAGS separate from EXPORT** (the hints). Rejected: the vocabulary is one small file that shares the counsel gate and readers with the export notes.
- **One STATUS leaf for support and classification.** Rejected: they are reviewed by different people and change on different schedules.

## Open questions

- Counsel review is needed for CRA classification, the project's role (manufacturer versus open-source steward) and export control (EAR, Wassenaar) before EU or foreign distribution. These are people's decisions; a decision record should be opened when counsel input exists.
- Cross-domain, unowned: no node enforces export-control data tags; `AUTO.MCP.PROJ` projects by side-effect class only, and `CORE.PRJ.META` stores free-form tags only. `QUAL.CRA.EXPORT` publishes the vocabulary for them.
- The parent's export-control scope has no numbered requirement in QUAL; this node carries it under REQ-QUAL-006. Maintainers may wish to add a parent requirement.
- Resolved: the EU distribution gate is `ECO.REL.PROC.GATES` gate `qual-cra` with runbook step `eu-venue-gate`; `QUAL.CRA.CLASS` writes the status file it reads.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| QUAL.CRA.SUPPORT | Support-period statement and security-update policy | 001 | none |
| QUAL.CRA.CLASS | CRA classification record and distribution-gate status | 002 004 | none |
| QUAL.CRA.TECHDOC | CRA technical-documentation index and citation check | 003 008 | none |
| QUAL.CRA.REPORT | Exploited-vulnerability reporting procedure and deadline tracker | 005 | none |
| QUAL.CRA.EXPORT | Export-control notes and data-tag vocabulary | 006 007 | none |
