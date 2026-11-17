---
id: QUAL.VULN
title: Security policy, vulnerability disclosure and advisories
level: 2
parent: QUAL
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-QUAL-004]
covers: []
depends_on: []
uses_artifacts: [QUAL.SBOM.VEX, ECO.REL.LTS, ECO.REL.PROC.CHANNEL, ECO.REL.PROC.NOTES, ECO.REL.UPD, ECO.REL.SIGN.CUSTODY]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.VULN: Security policy, vulnerability disclosure and advisories

## Purpose

The project's coordinated vulnerability disclosure process: a published security policy, a private reporting channel, target response times, triage and severity scoring, embargoed fixes, CVE assignment and published advisories. It also receives dependency findings that `QUAL.SBOM` judges to affect the product, so customers get one advisory stream regardless of where a flaw came from.

## Scope

**In:**
- `SECURITY.md` in every project repository: supported versions, private reporting channel, what to include, target times, safe-harbour statement
- Triage workflow: acknowledge, reproduce, score severity, decide affected versions, assign an owner
- Embargoed fix handling: private fix branch, patch release request to the release process, coordinated publication date
- CVE assignment and advisories in a machine-readable format alongside a human page
- An `actively_exploited` flag on every confirmed vulnerability, for regulatory reporting duties
- Advisory feed consumed by the in-product update check and by the periodic-review report

**Out** (and which node owns it instead):
- Threat analysis (owned by `QUAL.THREAT`)
- Scanning dependencies and VEX statements (owned by `QUAL.SBOM`)
- Building and signing patch releases (owned by `ECO.REL`)
- CRA reporting duties and their deadlines (owned by `QUAL.CRA`)
- Showing advisories in the product (owned by `ECO.REL.UPD`)

## Requirements

- **REQ-QUAL.VULN-001** Every project repository shall contain a `SECURITY.md` naming supported versions, a private reporting channel and the target response times.
- **REQ-QUAL.VULN-002** The private reporting channel shall not expose report content publicly and shall be reachable without an account on any single commercial platform [unverified channel options].
- **REQ-QUAL.VULN-003** The project shall acknowledge each report within 3 working days and give an initial severity assessment within 10 working days [unverified targets, to be set by maintainers].
- **REQ-QUAL.VULN-004** Each confirmed vulnerability shall receive a severity score under a named, versioned scoring system and a list of affected product versions.
- **REQ-QUAL.VULN-005** When a vulnerability is confirmed and fixed, the project shall publish an advisory with an identifier, affected and fixed versions, severity, workaround if any and credit, in both a machine-readable and a human-readable form.
- **REQ-QUAL.VULN-006** While a vulnerability is under embargo, its fix shall be developed outside public branches and its details shall not appear in public issues, commits or release notes.
- **REQ-QUAL.VULN-007** Every confirmed vulnerability record shall carry an `actively_exploited` flag with the time it was set.
- **REQ-QUAL.VULN-008** When `QUAL.SBOM` marks a dependency vulnerability as `affected`, the project shall handle it through the same triage and advisory process as a reported vulnerability.
- **REQ-QUAL.VULN-009** If a reported vulnerability affects values of record, then the advisory shall name the affected algorithm IDs and whether past results need re-evaluation.

## Interfaces

- **Offers:** `SECURITY.md`; advisory feed (machine-readable) and advisory pages; vulnerability records with `actively_exploited` flag (to `QUAL.CRA`); advisory list (to `QUAL.VAP` review report and `ECO.REL.UPD`).
- **Consumes (as artifacts):** `sbom/affected.json` (`QUAL.SBOM.VEX`); LTS lines (`ECO.REL.LTS`) and channels (`ECO.REL.PROC.CHANNEL`) for supported versions; the PR export (`ECO.REL.PROC.NOTES`) for the embargo scan. Patch releases go through the normal release flow (`ECO.REL.PROC.FLOW`). The threat register refers to advisories, not the reverse, so no dependency on `QUAL.THREAT` remains.

## Decomposition hints

POLICY (SECURITY.md and channel), TRIAGE (workflow, scoring, targets), EMBARGO (private fix and coordinated release), ADVIS (advisory format, CVE assignment and feed).

## Split rationale

The split follows the life of a vulnerability: the public promise before any report (POLICY), the private handling while it is open (TRIAGE, including embargo), and the public record once fixed (ADVIS). The private/public boundary is the main reason: TRIAGE runs in a private repository and its outputs are filtered, while POLICY and ADVIS produce public files.

Alternatives considered:
- **A separate EMBARGO leaf** (the hints). Rejected: the embargo rules need the record's codewords and dates, and the scan is small; it sits with the records.
- **Advisories inside TRIAGE.** Rejected: publication has its own format, leak check and consumers (`ECO.REL.UPD`, `QUAL.VAP.REVIEW`).

## Open questions

- Becoming a CVE Numbering Authority versus requesting IDs through a platform is a maintainer choice.
- The private channel choice (encrypted mail, a platform's private reporting feature, or both) interacts with C7 and with maintainer capacity; `QUAL.VULN.POLICY` requires encrypted mail at minimum.
- Bug bounties are out of scope unless funded; not decided.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| QUAL.VULN.POLICY | SECURITY.md, reporting channel and policy check | 001 002 | none |
| QUAL.VULN.TRIAGE | Vulnerability records, triage timers and embargo checks | 003 004 006 007 008 | none |
| QUAL.VULN.ADVIS | Advisory format, pages and feed | 005 009 | none |
