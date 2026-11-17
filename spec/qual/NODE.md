---
id: QUAL
title: Customer validation, compliance and security
level: 1
parent: FID
kind: domain
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-027]
covers: []
depends_on: [TRUST, UNC, ECO, KRN]
cli: [fid validate *]
mcp: [validate_*]
journal: [validate.*]
updated: 2026-11-16
---

# QUAL: Customer validation, compliance and security

## Purpose

Lets regulated and accredited customers adopt the software:
- validation evidence packs (IQ/OQ/PQ) for aerospace, medical, automotive and ISO/IEC 17025 labs;
- the mapping from our evidence (TraCIM results, ORS suites, replay) to what auditors ask for;
- the project's **security and product-compliance** posture: threat model, vulnerability handling, SBOM, EU Cyber Resilience Act duties, export-control screening.

## Scope

**In:**
- Validation Accelerator Pack: IQ/OQ/PQ templates, automated installation and operational qualification runs, an evidence bundle per release
- A compliance matrix that maps evidence to customer frameworks (AS9100, IATF 16949, ISO 13485 / FDA QMSR, ISO/IEC 17025, 21 CFR Part 11), clause IDs only
- Software verification evidence per release (fit validation, ORS results, determinism)
- Threat model and security architecture review
- Security policy and vulnerability disclosure (SECURITY.md, CVE handling, response times)
- SBOM per release (SPDX or CycloneDX) and dependency vulnerability scanning
- EU Cyber Resilience Act duties: support period, security updates, conformity documentation
- Export-control classification notes and data tags
- Vocabulary discipline: no unqualified "compliant", "certified" or "validated" claims

**Out:**
- Signing and verifier mechanics (TRUST)
- Fit validation datasets (UNC)
- CI implementation (ECO.CI runs QUAL's checks)

## Requirements

- **REQ-QUAL-001** Each release shall ship a Validation Accelerator Pack with IQ/OQ/PQ templates and an automated IQ/OQ run that produces a signed evidence bundle on the customer's machine.
- **REQ-QUAL-002** The project shall publish a compliance matrix mapping its evidence to customer frameworks by clause ID only. It shall state that compliance decisions belong to the customer.
- **REQ-QUAL-003** The project shall maintain a written threat model covering MCP, the control channel, plugins, driver hosts, file import and motion. It shall be reviewed at every minor release.
- **REQ-QUAL-004** The project shall publish a security policy with a private reporting channel and target response times, and shall issue advisories for confirmed vulnerabilities.
- **REQ-QUAL-005** Every release shall include an SBOM and pass dependency vulnerability scanning with no unresolved critical findings.
- **REQ-QUAL-006** The project shall document its EU Cyber Resilience Act support period and security-update policy before any EU distribution.
- **REQ-QUAL-007** Product UI, docs and marketing shall pass the vocabulary lint for regulated terms (ECO.RULES and OUTR.BRAND).

## Interfaces

- **Offers:** `validate.*` commands (IQ/OQ runs), evidence bundles, compliance matrix, security policy, SBOMs.
- **Consumes:** TRUST (signing), UNC/KRN (validation evidence), ECO (CI, releases).

## Decomposition hints

VAP (IQ/OQ/PQ pack and runner), MATRIX (compliance matrix), EVID (per-release verification evidence bundle), THREAT (threat model), VULN (security policy, disclosure, advisories), SBOM (SBOM and scanning), CRA (CRA and export-control documentation).

## Split rationale

The split follows **who produces each kind of assurance and when**. Customer-side validation is divided into the executable runs inside the product (IQOQ) and the documents a customer fills in (VAP), because one is registry commands with journal records and the other is templates that change with frameworks, not code. The project's own per-release verification evidence (EVID) is separate from both, because it is assembled in CI from other domains' reports and is what IQ checks against. MATRIX maps that evidence to customer frameworks and also owns the regulated-claim wording, since both decide what the project may say about compliance. The security side is divided by process: the architecture-level threat model reviewed per minor release (THREAT), the incident-driven disclosure process (VULN), the continuous dependency gate (SBOM), and the pre-distribution legal paperwork (CRA, including export control).

Dependencies run one way to avoid cycles: EVID depends on SBOM; IQOQ on EVID; VAP on IQOQ and EVID; MATRIX on EVID and VAP; VULN on THREAT and SBOM; CRA on VULN, SBOM and THREAT. External dependencies name specific level-3 nodes (`KRN.MAN.*`, `UNC.RDL.*`, `UNC.ORS.RUN`, `TRUST.*`, `ECO.REL.*`, `ECO.CI.*`, `ECO.RULES.VOCAB`, `OUTR.BRAND.LINT`, `PLAT.DOCTOR.REPORT`), never whole domains.

Alternatives considered:
- **One VAP child for templates and runner** (the decomposition hint). Rejected: the runner is product code with commands, journal events and fixtures, the templates are documents; they have different reviewers and change for different reasons.
- **A separate CLAIM child for vocabulary discipline.** Rejected: both lints already exist in ECO and OUTR; what remains for QUAL is a term list and a consistency test, too small for a subsystem, and it shares its review audience with the matrix.
- **Folding SBOM into ECO.REL.SIGN.** Rejected at this level: ECO.REL.SIGN generates the files, but the content policy, vulnerability gate and VEX are QUAL's commitments under REQ-QUAL-005.
- **Separate EXPORT child.** Rejected: export control has no numbered parent requirement and shares the counsel-review gate with the CRA work.

## Open questions

- Counsel review is needed for CRA classification and export control (EAR/Wassenaar) before EU or foreign distribution.
- Cross-domain: three release-runbook steps (evidence completeness, threat-model review per minor, EU distribution gate) need `ECO.REL.PROC` to add them.
- Cross-domain: SBOM format is unverified in `ECO.REL.SIGN`; `QUAL.SBOM` accepts SPDX or CycloneDX until one is chosen jointly.
- Cross-domain: no node enforces export-control data tags in MCP projection (`AUTO.MCP.PROJ`) or stores them on projects (`CORE.PRJ`).
- Cross-domain: the product-string lint (`ECO.RULES.VOCAB`) and the outreach lint (`OUTR.BRAND.LINT`) list different regulated terms; `QUAL.MATRIX` adds a consistency test.
- Cross-domain: IQ/OQ evidence bundles assume the `TRUST.PKG` container can carry qualification records.
- The concept places VAP v1 in P1b; all children stay P1a because REQ-QUAL-001 applies to every release. A phase move would need a decision.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| QUAL.VAP | Validation Accelerator Pack documents and templates | 001 | none |
| QUAL.IQOQ | Automated IQ and OQ runs and qualification evidence bundle | 001 | none |
| QUAL.EVID | Per-release software verification evidence bundle | 001 | none |
| QUAL.MATRIX | Compliance matrix and regulated-claim wording | 002 007 | none |
| QUAL.THREAT | Threat model and security architecture review | 003 | none |
| QUAL.VULN | Security policy, vulnerability disclosure and advisories | 004 | none |
| QUAL.SBOM | SBOM policy, dependency vulnerability scanning and VEX | 005 | none |
| QUAL.CRA | EU Cyber Resilience Act and export-control documentation | 006 | none |
