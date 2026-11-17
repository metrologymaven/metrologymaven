---
id: QUAL.THREAT
title: Threat model and security architecture review
level: 2
parent: QUAL
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-QUAL-003]
covers: []
depends_on: [ECO.REL.PROC]
cli: []
mcp: []
journal: []
updated: 2026-11-16
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
- **Consumes:** the release runbook and minor-version tags (`ECO.REL.PROC`); the side-effect class table (CONSTITUTION §3); the spec tree for requirement ID resolution.

## Decomposition hints

MODEL (document and register schema), REVIEW (per-minor review record and gate), TRIGGER (PR-level change triggers), CHECK (requirement-ID resolution).

## Open questions

- The minor-release gate needs a step in `ECO.REL.PROC`'s runbook; cross-domain.
- Who may act as reviewer (a maintainer other than the author, or an external reviewer) is a governance choice for D-003.
- The concept doc's threat table (01 §10.2) is a starting point and needs re-checking against the current spec before it is imported [unverified currency].

## Children

_None yet: status `open`._
