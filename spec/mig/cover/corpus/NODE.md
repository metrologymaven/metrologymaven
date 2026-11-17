---
id: MIG.COVER.CORPUS
title: Migration corpus admission and construct inventory
level: 3
parent: MIG.COVER
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-MIG.COVER-005]
covers: []
depends_on: [IO.REG.DETECT, MIG.SRC.REG]
uses_artifacts: [ECO.DATA.CAT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# MIG.COVER.CORPUS: Migration corpus admission and construct inventory

## Purpose

Each migration format has a test corpus: real or synthetic files of that format that the producing nodes (`MIG.PROJ`, `MIG.TEMPL`, `MIG.PROGEQ`) are run over to produce per-item reports. This leaf owns the corpus manifest (`mig-corpus@1`) that lists those files and the check that every file is admitted in the project dataset catalogue with its licence and redistribution status, so a coverage table can never be computed from a file the project may not hold or publish.

## Scope

**In:**
- The `mig-corpus@1` manifest: one per format, listing each corpus file by catalogue ID, SHA-256, format ID and visibility (`public` or `private`)
- The corpus digest: SHA-256 over the canonical JSON of the manifest, used in coverage table headers
- `cargo xtask mig corpus-check`: catalogue admission, digest, format and visibility checks

**Out:**
- The dataset catalogue file and its licence policy (owned by `ECO.DATA.CAT`)
- Fetching and caching corpus files (owned by `ECO.DATA.FETCH`)
- Construct taxonomies (owned by `MIG.COVER.TAXO`)
- Counting constructs and generating tables (owned by `MIG.COVER.GEN`)

## Requirements

- **REQ-MIG.COVER.CORPUS-001** Each corpus manifest entry shall name a catalogue ID, a SHA-256, a format ID registered in the format registry, and a visibility of `public` or `private`.
- **REQ-MIG.COVER.CORPUS-002** If a manifest entry's catalogue ID is absent from the dataset catalogue, or its SHA-256 differs from the catalogue entry, then the corpus check shall fail with `CORPUS-UNCATALOGUED` or `CORPUS-DIGEST` and name the entry.
- **REQ-MIG.COVER.CORPUS-003** If a manifest entry is marked `public` while its catalogue entry is `fetch-only` or declares customer data, then the corpus check shall fail with `CORPUS-VISIBILITY`.
- **REQ-MIG.COVER.CORPUS-004** If a manifest's format ID has no entry in the `MIG.SRC` register with status `ADMITTED` or `OPEN_EXPORT_ONLY`, then the corpus check shall fail with `CORPUS-SOURCE`.
- **REQ-MIG.COVER.CORPUS-005** The corpus digest shall be the SHA-256 of the RFC 8785 canonical JSON of the manifest with entries sorted by catalogue ID, and the check shall print it.
- **REQ-MIG.COVER.CORPUS-006** A manifest that contains any `private` entry shall be labelled `private_run: true`, and the check shall print that label.

## Acceptance criteria

- **AC-1** Given `fixtures/mig/cover/corpus/ok/` (manifest of three public entries matching `fixtures/mig/cover/corpus/datasets.toml`), when `cargo xtask mig corpus-check --manifest ok/corpus.json --catalogue datasets.toml --sources sources.json` runs, then it exits 0 and prints the corpus digest equal to `ok/expected-digest.txt`.
- **AC-2** (error path) Given `missing.json` (one catalogue ID not in the catalogue), then it exits 1 with `CORPUS-UNCATALOGUED <id>`.
- **AC-3** (error path) Given `public-fetchonly.json` (a public entry whose catalogue entry is fetch-only), then it exits 1 with `CORPUS-VISIBILITY <id>`.
- **AC-4** (error path) Given `pending-source.json` (format whose register entry is `PENDING_REVIEW`), then it exits 1 with `CORPUS-SOURCE <format>`.
- **AC-5** Given `mixed.json` (one private entry), then it exits 0 and prints `private_run: true`.
- **AC-6** Given `ok/corpus.json` with entries shuffled, then the printed digest is unchanged.

## Design

- **Registry command(s):** none; repository tool `cargo xtask mig corpus-check --manifest <json> --catalogue <toml> --sources <json>`.
- **Data structures:** `CorpusManifest { schema: "mig-corpus@1", format_id, name, entries: Vec<CorpusEntry> }`, `CorpusEntry { catalogue_id, sha256, format_id, visibility: Public | Private, note? }`.
- **Algorithm:** bounded JSON parse (1 MiB) → per entry: catalogue lookup, digest compare, visibility rule (public needs `redistribution = mirror` and no customer data), register status lookup → sort by catalogue ID → canonical JSON (`CORE.JRN.CANON` encoder) → SHA-256.
- **Errors:** `CORPUS-SCHEMA`, `CORPUS-UNCATALOGUED`, `CORPUS-DIGEST`, `CORPUS-VISIBILITY`, `CORPUS-SOURCE`, `CORPUS-DUP`.
- **Determinism:** D0 (pure function of the three inputs).
- **Performance budget:** 1,000-entry manifest checked in under 200 ms [unverified until measured].
- **Crate:** `fid-mig` (module `cover::corpus`), exposed through `fid-xtask`.

## Test fixtures

`fixtures/mig/cover/corpus/`: synthetic manifests, a small `datasets.toml` and a `sources.json` register extract; all written for the project, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `cover::corpus` and expose it as `cargo xtask mig corpus-check --manifest <json> --catalogue <toml> --sources <json>`. Implement spec node MIG.COVER.CORPUS: parse the `mig-corpus@1` manifest (bounded, 1 MiB, fail closed), check each entry against the dataset catalogue (`datasets.toml`, schema of ECO.DATA.CAT) for presence (`CORPUS-UNCATALOGUED`) and SHA-256 (`CORPUS-DIGEST`), refuse public entries whose catalogue entry is fetch-only or declares customer data (`CORPUS-VISIBILITY`), refuse formats whose source-register entry is not `ADMITTED` or `OPEN_EXPORT_ONLY` (`CORPUS-SOURCE`), and print the corpus digest = SHA-256 of RFC 8785 canonical JSON of the manifest with entries sorted by catalogue ID, plus `private_run: true` when any entry is private. Exit 1 on any finding, printing findings sorted by entry and code. Tests for AC-1..AC-6 with fixtures under `fixtures/mig/cover/corpus/`. No `unwrap`/`expect`/`panic`/`unsafe`; no HashMap/HashSet (use BTreeMap).

## Open questions

- None.

## Children

_Leaf: none._
