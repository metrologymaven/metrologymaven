---
id: ECO.REL.REPRO
title: Reproducible builds and their verification
level: 3
parent: ECO.REL
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.REL-006]
covers: []
depends_on: [ECO.CI.WF]
uses_artifacts: [ECO.REL.PKG, ECO.REL.WEB, ECO.REL.SIGN]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.REPRO: Reproducible builds and their verification

## Purpose

Makes release artefacts reproducible, so that anyone building the same tag on the reference builder gets the same bytes apart from signatures, and runs a second independent build per release to prove it. Reproducibility lets users and auditors check that a signed installer came from the published source.

## Scope

**In:**
- Build-environment pinning (toolchain, dependencies, timestamps, paths)
- Signature stripping and comparison rules per artefact type
- A second, independent rebuild per release, a published comparison result, and a build provenance statement per artefact
- A documented recipe for third parties to rebuild

**Out** (and which node owns it instead):
- Producing the artefacts (owned by `ECO.REL.PKG` and `ECO.REL.WEB`)
- Signing (owned by `ECO.REL.SIGN`)

## Requirements

- **REQ-ECO.REL.REPRO-001** When the same tag is built twice on the reference builder, the artefacts with signatures removed shall be byte-identical for the `fid` CLI, `fid-verify` and the web bundle.
- **REQ-ECO.REL.REPRO-002** For each OS installer format, the project shall publish whether it is reproducible, and the comparison rule used, and shall list any known non-reproducible bytes [unverified for all three OS installers].
- **REQ-ECO.REL.REPRO-003** Each release shall publish the result of an independent rebuild comparison for every artefact.
- **REQ-ECO.REL.REPRO-004** The project shall publish a rebuild recipe that a third party can follow offline after fetching pinned inputs.

## Interfaces

- **Offers:** `build-env.lock` and the pin check (PIN); `repro-formats.toml` and `cargo xtask rel repro-compare` (STRIP); `rebuild-report.json` and per-artefact provenance statements (REBUILD; consumed by `QUAL.EVID` as a bundle member); `REBUILD.md` and `cargo xtask rel rebuild` (RECIPE).
- **Consumes (as files):** artefacts and build definitions from `ECO.REL.PKG` and `ECO.REL.WEB`; signature formats from `ECO.REL.SIGN` (for stripping).
- **Code dependency:** build jobs of `ECO.CI.WF`.

## Decomposition hints

PIN (environment pinning), STRIP (signature stripping and comparison per format), REBUILD (independent rebuild job, report and provenance), RECIPE.

## Open questions

- Reproducibility of signed macOS and Windows installers is known to be difficult [unverified]; REQ-002 states the honest status per format rather than promising it.
- QUAL.EVID left build provenance (SLSA) out of its P1a member set because this node is P1b; ECO.REL.REPRO.REBUILD now produces the statements, so QUAL.EVID can add them as a member from P1b.
- "Independent" rebuild means a different runner instance from a clean image, not a different organisation; third-party rebuilders are invited through the RECIPE.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ECO.REL.REPRO.PIN | Build-environment pinning and pin check (leaf) | 001 004 | none |
| ECO.REL.REPRO.STRIP | Signature stripping and per-format comparison (leaf) | 001 002 | none |
| ECO.REL.REPRO.REBUILD | Independent rebuild, comparison report and provenance (leaf) | 001 003 | none |
| ECO.REL.REPRO.RECIPE | Third-party offline rebuild recipe (leaf) | 004 | none |
