---
id: QUAL.SBOM
title: SBOM policy, dependency vulnerability scanning and VEX
level: 2
parent: QUAL
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-QUAL-005]
covers: []
depends_on: [ECO.REL.SIGN, ECO.CI.WF, ECO.CI.LIC]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# QUAL.SBOM: SBOM policy, dependency vulnerability scanning and VEX

## Purpose

Sets what the per-release SBOM must contain and proves it is complete, scans every dependency for known vulnerabilities on every PR, nightly and before release, and records an exploitability (VEX) statement for every finding. It holds the release gate of REQ-QUAL-005: no release with an unresolved critical finding. SBOM generation itself is done by the release pipeline in `ECO.REL.SIGN`; this node owns the policy and the checks around it.

## Scope

**In:**
- SBOM content policy: format (SPDX or CycloneDX), minimum fields per component (name, version, supplier or source, licence expression, package URL, digest), one SBOM per shipped artefact
- Completeness check: every resolved dependency in each lockfile and every vendored or bundled native component appears in the SBOM
- Vulnerability scanning against at least one public advisory database on PRs, nightly and at release
- Severity thresholds and the release gate
- VEX statements per finding: `not_affected` (with justification), `affected`, `fixed`, `under_investigation`
- Hand-off of `affected` findings to `QUAL.VULN`

**Out** (and which node owns it instead):
- Producing and signing SBOM files (owned by `ECO.REL.SIGN`)
- Licence allowlist checks (owned by `ECO.CI.LIC`)
- Advisories for the project's own vulnerabilities (owned by `QUAL.VULN`)
- Dependency admission review for new core dependencies (owned by `ECO.CI` when specified; see Open questions)

## Requirements

- **REQ-QUAL.SBOM-001** Each shipped artefact's SBOM shall be a machine-readable SPDX or CycloneDX document listing for every component its name, version, source, licence expression, package URL and digest where one exists.
- **REQ-QUAL.SBOM-002** If a dependency resolved in any lockfile of the release, or a bundled native component, is absent from the artefact's SBOM, then the completeness check shall fail and name the component.
- **REQ-QUAL.SBOM-003** The dependency vulnerability scan shall run on every PR that changes a lockfile, nightly on the default branch, and on every release candidate.
- **REQ-QUAL.SBOM-004** If a release candidate has a finding of critical severity (CVSS base score 9.0 or higher, or the database's equivalent band) without a VEX status of `not_affected` or `fixed`, then the release gate shall fail.
- **REQ-QUAL.SBOM-005** Every finding on a release candidate shall have a VEX statement, and each `not_affected` statement shall carry a justification code and a named reviewer.
- **REQ-QUAL.SBOM-006** Each release shall publish its VEX document next to its SBOMs, covering every finding present at release time.
- **REQ-QUAL.SBOM-007** When a finding is set to `affected`, the system shall open a private triage item in the `QUAL.VULN` process within 1 working day [unverified target].
- **REQ-QUAL.SBOM-008** If the advisory database cannot be reached during a release scan, then the release gate shall fail rather than pass on stale data older than 24 hours.

## Interfaces

- **Offers:** SBOM policy and completeness check; scan job and release gate (run in `ECO.CI.WF`); VEX documents (to `QUAL.EVID`, `QUAL.CRA`); `affected` findings (to `QUAL.VULN`).
- **Consumes:** SBOM files from `ECO.REL.SIGN`; reusable workflows and required checks from `ECO.CI.WF`; licence data from `ECO.CI.LIC`.

## Decomposition hints

POLICY (format and field rules), COMPLETE (lockfile to SBOM check), SCAN (scanner jobs and database freshness), GATE (thresholds), VEX (statement format and review).

## Open questions

- `ECO.REL.SIGN` leaves the SBOM format unverified; the concept doc proposes CycloneDX. One format should be chosen jointly; cross-domain.
- The concept doc requires a dependency decision record for every new core dependency; no ECO node owns that yet; cross-domain.
- Scanner and database choice is a leaf-level decision; both must run offline-capable in CI mirrors for air-gapped builds [unverified].

## Children

_None yet: status `open`._
