---
id: MIG.COVER.GEN
title: Coverage table generator and drift check
level: 3
parent: MIG.COVER
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.COVER-001, REQ-MIG.COVER-002, REQ-MIG.COVER-003, REQ-MIG.COVER-004, REQ-MIG.COVER-006]
covers: []
depends_on: [MIG.COVER.CORPUS, MIG.COVER.TAXO, CORE.JRN.CANON, IO.REG.ROOTS]
uses_artifacts: [ECO.PARITY.GEN, MIG.PROJ.MAPREP, MIG.TEMPL.CHECK, MIG.PROGEQ]
side_effect: R1
determinism: D0
cli: [fid migrate generate-coverage]
mcp: [migrate_generate_coverage]
journal: [migrate.generate_coverage@1]
updated: 2026-11-17
---

# MIG.COVER.GEN: Coverage table generator and drift check

## Purpose

Turns the per-item reports that producers emit over a corpus (mapping reports from `MIG.PROJ.MAPREP`, fill reports from `MIG.TEMPL.CHECK`, and, from P2, equivalence reports from `MIG.PROGEQ`) into one coverage table per format: every construct of the format's taxonomy with a status of `SUPPORTED`, `PARTIAL`, `UNSUPPORTED` or `UNTESTED` and the occurrence counts behind it. It also offers a check mode that regenerates and byte-compares committed tables, so tables can never be hand-edited.

## Scope

**In:**
- `migrate.generate_coverage@1` (R1): inputs are a format ID, a corpus manifest, the taxonomy and a set of producer reports, all as handles in approved import roots; output is the table as JSON (`mig-coverage@1`) and Markdown
- Status rules per construct and the occurrence counts
- The generated header (release, corpus name and digest, taxonomy version, generator version, input digests) in the `ECO.PARITY.GEN` header format
- Check mode (`--check <committed-dir>`): regenerate and byte-compare, with the `DRIFT-*` error codes

**Out:**
- Producing the per-item reports (owned by `MIG.PROJ.MAPREP`, `MIG.TEMPL.CHECK`, `MIG.PROGEQ`)
- Corpus admission (owned by `MIG.COVER.CORPUS`) and taxonomies (owned by `MIG.COVER.TAXO`)
- Publishing pages on the website (docs pipeline in `ECO`)

## Requirements

- **REQ-MIG.COVER.GEN-001** When `migrate.generate_coverage@1` runs for a format, the system shall output one row for every construct of that format's taxonomy, in taxonomy order, with its status and the counts of `MAPPED`, `PARTIAL` and `UNSUPPORTED` occurrences.
- **REQ-MIG.COVER.GEN-002** A construct shall be `SUPPORTED` only when it has at least one occurrence, every occurrence is `MAPPED`, and, for program formats, every corpus program containing it has the verdict `EQUIVALENT`.
- **REQ-MIG.COVER.GEN-003** A construct with at least one `MAPPED` or `PARTIAL` occurrence that is not `SUPPORTED` shall be `PARTIAL`, and a construct whose occurrences are all `UNSUPPORTED` shall be `UNSUPPORTED`.
- **REQ-MIG.COVER.GEN-004** A construct with zero occurrences in the corpus shall be `UNTESTED`.
- **REQ-MIG.COVER.GEN-005** Each table shall carry a header stating release, corpus name, corpus digest, taxonomy version, generator version, `private_run` flag and the SHA-256 of every input report.
- **REQ-MIG.COVER.GEN-006** If a producer report's corpus file digest is not listed in the corpus manifest, then the command shall fail with `coverage_foreign_report` and name the report.
- **REQ-MIG.COVER.GEN-007** When run in check mode, if any committed table differs in any byte from a fresh generation from the inputs named in its header, then the command shall exit non-zero with `DRIFT-DIFF` and name the table and first differing line.
- **REQ-MIG.COVER.GEN-008** The same inputs shall produce byte-identical JSON (RFC 8785 canonical) and Markdown output.

## Acceptance criteria

- **AC-1** Given `fixtures/mig/cover/gen/csv-points/` (taxonomy of 6 constructs, corpus of 4 files, 4 mapping reports), when `fid migrate generate-coverage --format csv-points --corpus corpus.json --taxonomy taxo.json --reports reports/ --release 0.1.0 --json` runs, then the output equals `expected/coverage.json` byte for byte, including one `UNTESTED`, one `PARTIAL` and one `UNSUPPORTED` row.
- **AC-2** Given `fixtures/mig/cover/gen/dmis-prog/` with one program `DIVERGENT` that contains `GOTO` while all its `GOTO` occurrences are `MAPPED`, then `GOTO` is `PARTIAL`, not `SUPPORTED`.
- **AC-3** (error path) Given a report whose file digest is not in the corpus manifest, then the command fails with `coverage_foreign_report` and outputs no table.
- **AC-4** (drift) Given `expected/` with one count edited by hand, when the command runs with `--check expected/`, then it exits non-zero with `DRIFT-DIFF coverage-csv-points.md:<line>`.
- **AC-5** Given AC-1 inputs on x86-64, ARM64 and wasm32, then outputs are byte-identical.

## Design

- **Registry command(s):** `migrate.generate_coverage@1`, R1; params `format: FormatId`, `corpus: ImportHandle`, `taxonomy: ImportHandle`, `reports: ImportHandle` (directory), `release: SemVer`, optional `check: ImportHandle`; result `CoverageTable { header, rows: Vec<CoverageRow> }` plus rendered Markdown. Writing the files into the repository is done by the release build through the docs pipeline, not by this command.
- **Data structures:** `CoverageRow { construct_id, status: Supported | Partial | Unsupported | Untested, mapped: u32, partial: u32, unsupported: u32, programs_equivalent: Option<u32>, programs_not_equivalent: Option<u32> }`; `ProducerItem { corpus_sha256, construct_id, status, program_verdict? }` read from the `mig-mapreport@1`, `mig-fillreport@1` and (P2) `mig-equivalence@1` schemas.
- **Algorithm:** load corpus and taxonomy (validated by the CORPUS and TAXO checkers) → read every report, reject foreign digests → BTreeMap from construct ID to counters → apply REQ-002..004 → render JSON (canonical) and Markdown with the header.
- **Errors:** `coverage_foreign_report`, `coverage_unknown_construct`, `coverage_schema`, `DRIFT-DIFF`, `DRIFT-INPUT`, `DRIFT-HEADER`.
- **Determinism:** D0; integer counts only, no floating point.
- **Performance budget:** 10,000 report items in under 500 ms [unverified until measured].
- **Crate:** `fid-mig` (module `cover::gen`).

## Test fixtures

`fixtures/mig/cover/gen/`: synthetic taxonomies, corpus manifests and producer reports written for the project (CC0-1.0); expected outputs generated once and reviewed by hand.

## Build prompt

> In crate `fid-mig`, add module `cover::gen` and register `migrate.generate_coverage@1` (R1; CLI `fid migrate generate-coverage`, MCP `migrate_generate_coverage`). Params are approved import-root handles (never raw paths): corpus manifest (`mig-corpus@1`), taxonomy (`mig-taxonomy@1`), a reports directory (`mig-mapreport@1`, `mig-fillreport@1`, optional `mig-equivalence@1`), the format ID and release; optional `check` handle. Implement spec node MIG.COVER.GEN: one row per taxonomy construct in taxonomy order; `SUPPORTED` only if ≥1 occurrence, all `MAPPED`, and for programs every containing program `EQUIVALENT`; `PARTIAL` if any `MAPPED`/`PARTIAL` but not supported; `UNSUPPORTED` if all unsupported; `UNTESTED` if zero occurrences. Fail `coverage_foreign_report` when a report's corpus digest is not in the manifest. Emit RFC 8785 canonical JSON and Markdown with a header (release, corpus name and digest, taxonomy version, generator version, private_run, input report digests) in the ECO.PARITY generated-header format. Check mode regenerates and byte-compares, failing `DRIFT-DIFF <file>:<line>`, `DRIFT-INPUT` or `DRIFT-HEADER`. Tests for AC-1..AC-5. Integer arithmetic only; BTreeMap only; no `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- Program-format tables depend on `MIG.PROGEQ` reports (P2, outside launch-1); in launch-1 the generator simply has no program reports to read.

## Children

_Leaf: none._
