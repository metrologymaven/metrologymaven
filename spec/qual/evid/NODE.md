---
id: QUAL.EVID
title: Per-release software verification evidence bundle
level: 2
parent: QUAL
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-QUAL-001]
covers: []
depends_on: [QUAL.SBOM, KRN.MAN.VAL, KRN.MAN.MANI, KRN.MAN.XART, UNC.ORS.RUN, UNC.RDL.FREEZE, TRUST.REPLAY.CORPUS, TRUST.SIGN.DSSE, ECO.REL.PROC, ECO.REL.SIGN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# QUAL.EVID: Per-release software verification evidence bundle

## Purpose

Assembles, for every release, one signed bundle of the project's own verification evidence: kernel validation results, open reference suite results, replay and determinism results, the kernel manifest, the SBOM and vulnerability status, a change-impact report against the previous release, and a list of known anomalies. It is what IQ checks against on the customer's machine and what the VAP and the compliance matrix point to. The bundle aggregates outputs that other domains generate; it never recomputes them and is never hand-edited.

## Scope

**In:**
- Bundle layout and a top-level release-evidence statement listing every member with its digest
- Member set: kernel manifest; per-release validation report (TraCIM and NIST sets); ORS pass statistics; replay conformance corpus results; cross-artifact bit-identity results; frozen RDL release digest; SBOM and VEX digests; change-impact report; known anomalies and limitations list
- Change-impact report: changed algorithm IDs and implementation digests, RDL and ORS deltas, REP-1 exceedances against the previous release
- Known anomalies list, maintained as data and reviewed per release
- CI assembly step in the release workflow and a completeness gate

**Out** (and which node owns it instead):
- Producing the validation report, ORS runs, replay corpus and bit-identity gate (owned by `KRN.MAN.VAL`, `UNC.ORS.RUN`, `TRUST.REPLAY.CORPUS`, `KRN.MAN.XART`)
- SBOM and VEX content (owned by `QUAL.SBOM`)
- Release signing keys and the human approval step (owned by `ECO.REL.SIGN`)
- The compliance matrix, which cites this bundle by digest (owned by `QUAL.MATRIX`)
- Transparency-log anchoring and the Evidence Bundle SCITT registration (P2, owned by `TRUST` when specified)

## Requirements

- **REQ-QUAL.EVID-001** Each release shall publish one evidence bundle whose top-level statement lists every member with its SHA-256 digest and the release version.
- **REQ-QUAL.EVID-002** The bundle shall contain the kernel manifest, the per-release validation report, ORS pass statistics, replay corpus results, bit-identity results, the frozen RDL release digest, the SBOM and VEX digests, a change-impact report and a known-anomalies list.
- **REQ-QUAL.EVID-003** If any required member is missing or its digest does not match the generated artefact, then the release workflow shall stop before the signing approval step and name the member.
- **REQ-QUAL.EVID-004** The change-impact report shall list every algorithm whose implementation digest changed since the previous release on the same channel, with its RDL and ORS result deltas and any REP-1 exceedance.
- **REQ-QUAL.EVID-005** The bundle shall be assembled by CI from generated artefacts only, and regenerating it from the same inputs shall produce a byte-identical top-level statement.
- **REQ-QUAL.EVID-006** The top-level statement shall be signed as a DSSE envelope with the release key after the human release approval, and shall be checkable offline with published tools.
- **REQ-QUAL.EVID-007** The known-anomalies list shall have one entry per open defect labelled as affecting values of record, with affected algorithm IDs and workaround, and a release with an unlisted such defect shall fail the completeness gate.
- **REQ-QUAL.EVID-008** If redistribution terms of an external test report forbid publishing reference values, then the bundle shall include its pass status and digest only and shall state the omission.

## Interfaces

- **Offers:** evidence bundle and its top-level statement digest (to `QUAL.IQOQ`, `QUAL.VAP`, `QUAL.MATRIX`, `ECO.REL.PROC` release notes, `OUTR` publication); change-impact report; known-anomalies list.
- **Consumes:** reports from `KRN.MAN.VAL`, `UNC.ORS.RUN`, `TRUST.REPLAY.CORPUS`, `KRN.MAN.XART`; `KRN.MAN.MANI`; `UNC.RDL.FREEZE`; SBOM and VEX from `QUAL.SBOM`; DSSE encoding from `TRUST.SIGN.DSSE`; release workflow from `ECO.REL.PROC`; signing step from `ECO.REL.SIGN`.

## Decomposition hints

LAYOUT (bundle layout and statement), ASSEMBLE (CI step and completeness gate), IMPACT (change-impact report), ANOM (known-anomalies list).

## Open questions

- Adding the completeness gate to the release runbook needs a step in `ECO.REL.PROC`; cross-domain.
- TraCIM report redistribution terms are unverified (also raised in `KRN.MAN.VAL`).
- The concept doc also lists build provenance (SLSA) and the MCP tool-definition manifest digest as members; build provenance belongs to `ECO.REL.REPRO` (P1b), so it is left out of the P1a member set and should be added when that node is specified.

## Children

_None yet: status `open`._
