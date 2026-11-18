---
id: IO.QIF.GEN.PIN
title: Pinned XSD manifest and digest check
level: 4
parent: IO.QIF.GEN
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-qif-gen]
satisfies: [REQ-IO.QIF.GEN-001]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.GEN.PIN: Pinned XSD manifest and digest check

## Purpose

The first step of the generator: load the manifest that pins every DMSC QIF 3.0 XSD (and normative XSLT) file by size and SHA-256, check the files on disk against it, and hand only verified bytes to the parser. Nothing downstream ever reads a schema file that this leaf has not checked.

## Scope

**In:**
- Manifest file `qif-xsd.manifest.toml`: upstream source (repository URL, tag or commit), licence file digest, and one row per file (relative path, byte size, SHA-256, role `xsd` or `xslt`)
- Verification of the schema directory against the manifest (missing, extra, changed files)
- A `PinnedSet` value that gives read access to verified bytes by relative path only
- Rejection of `xs:include`/`xs:import` locations that are not in the manifest

**Out:**
- Parsing XSD content (`IO.QIF.GEN.PARSE`)
- Regeneration and diff gate in CI (`IO.QIF.GEN.GATE`)
- Fetching the files from the network (done once by a maintainer; the build never fetches)

## Requirements

- **REQ-IO.QIF.GEN.PIN-001** The manifest shall list every pinned file with relative path, byte size, SHA-256 and role, plus the upstream source reference and the SHA-256 of the upstream licence file.
- **REQ-IO.QIF.GEN.PIN-002** If any listed file is missing or its size or SHA-256 differs from the manifest, then verification shall fail with `xsd_digest_mismatch` naming the file and both digests.
- **REQ-IO.QIF.GEN.PIN-003** If the schema directory contains an `.xsd` or `.xsl` file that the manifest does not list, then verification shall fail with `xsd_unpinned_file` naming the file.
- **REQ-IO.QIF.GEN.PIN-004** When a schema location is requested through `PinnedSet`, the leaf shall resolve it only to a manifest row and shall fail with `xsd_unpinned_reference` for any other location, including absolute paths, URLs and `..` components.
- **REQ-IO.QIF.GEN.PIN-005** Verification shall read no file outside the schema directory and open no network connection.

## Acceptance criteria

- **AC-1** Given `fixtures/io/qif/gen/pin/ok/` (three small XSDs and a matching manifest), when `PinnedSet::verify` runs, then it returns a set of three files with their digests.
- **AC-2** (error path) Given `fixtures/io/qif/gen/pin/changed/` where one byte of `b.xsd` differs, then verification fails `xsd_digest_mismatch` naming `b.xsd`.
- **AC-3** (error path) Given `fixtures/io/qif/gen/pin/extra/` with an unlisted `c.xsd`, then it fails `xsd_unpinned_file`.
- **AC-4** (error path) Given a request for `http://example.org/x.xsd` and for `../outside.xsd`, then both fail `xsd_unpinned_reference`.
- **AC-5** Given the real pinned DMSC QIF 3.0 set in the implementation repository, when verification runs in CI on Windows, Linux and macOS, then it passes on all three.

## Design

- **Registry commands:** none (build-time library inside the generator).
- **Data structures:** `Manifest { source: { url, rev }, licence_sha256, files: Vec<PinnedFile> }`; `PinnedFile { path: RelPath, bytes: u64, sha256: Digest, role: Xsd|Xslt }`; `PinnedSet` (BTreeMap by path to verified bytes).
- **Algorithm:** parse the TOML manifest with a bounded reader; list the directory (sorted); compare names; stream-hash each file; build the set. Path normalisation uses `/` separators and rejects `..`, absolute and drive-letter forms before lookup.
- **Errors:** `xsd_digest_mismatch`, `xsd_unpinned_file`, `xsd_unpinned_reference`, `manifest_invalid`.
- **Determinism:** D0.
- **Performance budget:** whole QIF 3.0 set verified in < 1 s [unverified until measured].
- **Crate:** `fid-qif-gen` (module `pin`).

## Test fixtures

Hand-written tiny XSDs and manifests under `fixtures/io/qif/gen/pin/` (CC-BY-4.0). The real DMSC QIF 3.0 XSDs come from the `qif-community` repository under its own licence (Boost [unverified: confirm licence file]).

## Build prompt

> In the build-tool crate `fid-qif-gen`, add module `pin`. Load `qif-xsd.manifest.toml` (upstream URL and revision, licence-file SHA-256, rows of relative path, byte size, SHA-256, role `xsd|xslt`). `PinnedSet::verify(dir)` lists the directory in sorted order, fails `xsd_unpinned_file` for unlisted `.xsd`/`.xsl` files and `xsd_digest_mismatch` (file, expected and actual digest) for missing or changed ones, and returns verified bytes keyed by relative path. `PinnedSet::resolve(location)` accepts only manifest paths and fails `xsd_unpinned_reference` for URLs, absolute paths and `..`. No network, no reads outside the directory. CONSTITUTION §8: no panics, no unwrap, no `unsafe`, no std HashMap. Tests AC-1..AC-5 with fixtures under `fixtures/io/qif/gen/pin/`.

## Open questions

- None.

## Children

_Leaf: none._
