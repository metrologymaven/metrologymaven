---
id: TRUST.PKG.CONTENT.EVID
title: Evidence tiers and point-set gathering
level: 4
parent: TRUST.PKG.CONTENT
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.PKG.CONTENT-002, REQ-TRUST.PKG.CONTENT-003, REQ-TRUST.PKG.CONTENT-007]
covers: []
depends_on: [TRUST.PKG.MANIFEST, CORE.STO.CAS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.PKG.CONTENT.EVID: Evidence tiers and point-set gathering

## Purpose

Applies the evidence tier of an export (`results`, `features` or `points`) and, for `points`, gathers every input point set the included results were computed from, read from the project's content-addressed store by SHA-256 and deduplicated. It is what lets `fid-verify` replay a package without the original project.

## Scope

**In:**
- `EvidenceTier` and its record in the manifest
- Collecting input digests from the records and fetching point-set blobs from the CAS
- Deduplication by SHA-256 and member naming `points/<sha256>.<ext>`
- Errors for missing point sets

**Out:**
- Selecting records and checking fields (`TRUST.PKG.CONTENT.SELECT`)
- The journal segment (`TRUST.PKG.CONTENT.JSEG`)
- Storing blobs (`CORE.STO.CAS`)

## Requirements

- **REQ-TRUST.PKG.CONTENT.EVID-001** When the tier is `points`, gathering shall return one member for each distinct digest in the union of the included records' input digests that the CAS holds as a point set, and shall fail with `pkg.points_missing` listing every digest that the CAS cannot supply.
- **REQ-TRUST.PKG.CONTENT.EVID-002** When the tier is `results` or `features`, gathering shall return no point-set member and shall set the manifest's `evidence_tier` to that tier.
- **REQ-TRUST.PKG.CONTENT.EVID-003** Gathering shall emit each distinct point set exactly once, named `points/<sha256>.<ext>` where `<sha256>` is the lowercase hex digest, however many records reference it.
- **REQ-TRUST.PKG.CONTENT.EVID-004** If a blob read from the CAS does not hash to the requested digest, then gathering shall fail with `pkg.points_corrupt` naming the digest.
- **REQ-TRUST.PKG.CONTENT.EVID-005** Gathering shall pass point-set blobs as streamed blob references, never copying more than one 64 MiB buffer into memory at a time [unverified: buffer size to be tuned].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/pkg/content/points-shared` (5 records referencing 3 point sets, one shared by 3 records), when `gather(view, records, points)` runs, then exactly 3 point members are returned, named by digest, sorted by path.
- **AC-2** Given the same records with tier `results`, then no point member is returned and `evidence_tier` is `results`; the same for `features`.
- **AC-3** (error path) Given `fixtures/pkg/content/points-missing` (one digest absent from the CAS), then `pkg.points_missing` lists it; given a CAS blob with one flipped byte, then `pkg.points_corrupt`.
- **AC-4** Given a 2 GiB synthetic point set, then peak resident memory of the gather step stays under 200 MiB [unverified until measured].

## Design

- **Registry command(s):** none.
- **Data structures:** `EvidenceTier = Results | Features | Points`; `PointMember { digest: Sha256, ext, blob: BlobRef }`.
- **Algorithm:** union of input digests into a `BTreeSet`; for tier `points`, stat and stream each blob from CAS with hash check; extension from the stored media type (`.fpts` for the native point format [unverified: name owned by the point-set format]).
- **Errors:** `pkg.points_missing`, `pkg.points_corrupt`.
- **Determinism:** D0 (sorted digests, no clock).
- **Performance budget:** see AC-4.
- **Crate:** `fid-pkg`, module `content::evid`, feature `export`.

## Test fixtures

Seeded synthetic point sets in an in-memory CAS stub (`fixtures/pkg/content/points-*`), project licence.

## Build prompt

> In crate `fid-pkg`, feature `export`, add module `content::evid` with `pub fn gather(cas: &dyn BlobStore, records: &[ResultRecord], tier: EvidenceTier) -> Result<Vec<PointMember>, PkgError>`. For `Points`: take the union of all records' input digests (BTreeSet), fetch each as a streamed blob, re-hash while streaming and fail `pkg.points_corrupt` on mismatch; collect all absent digests and fail once with `pkg.points_missing` listing them. Name members `points/<lowercase-hex-sha256>.<ext>`, one per digest, sorted by path. For `Results`/`Features`: return nothing and record the tier. Never hold more than one 64 MiB buffer. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8 (no panic/unwrap, bounded reads), D0, no std HashMap.

## Open questions

- Very large point sets may need chunked members (minor format version, owned by `TRUST.PKG.MANIFEST`).

## Children

_Leaf: none._
