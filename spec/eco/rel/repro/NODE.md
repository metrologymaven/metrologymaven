---
id: ECO.REL.REPRO
title: Reproducible builds and their verification
level: 3
parent: ECO.REL
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.REL-006]
covers: []
depends_on: [ECO.REL.PKG, ECO.REL.SIGN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.REL.REPRO: Reproducible builds and their verification

## Purpose

Makes release artefacts reproducible, so that anyone building the same tag on the reference builder gets the same bytes apart from signatures, and runs a second independent build per release to prove it. Reproducibility lets users and auditors check that a signed installer came from the published source.

## Scope

**In:**
- Build-environment pinning (toolchain, dependencies, timestamps, paths)
- Signature stripping and comparison rules per artefact type
- A second, independent rebuild per release and a published comparison result
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

- **Offers:** rebuild comparison report per release; rebuild recipe.
- **Consumes:** ECO.REL.PKG build definitions; ECO.REL.SIGN signature formats (for stripping).

## Decomposition hints

PIN (environment pinning), STRIP (signature stripping per format), REBUILD (independent rebuild job), RECIPE.

## Open questions

- Reproducibility of signed macOS and Windows installers is known to be difficult [unverified]; REQ-002 states the honest status per format rather than promising it.

## Children

_None yet: status `open`._
