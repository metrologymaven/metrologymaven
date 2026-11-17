---
id: IO.CORP.SCORE
title: Round-trip and fidelity metrics and the scoring command
level: 3
parent: IO.CORP
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CORP-005, REQ-IO.CORP-006, REQ-IO.CORP-009]
covers: []
depends_on: [IO.CORP.MANIF, IO.REG.ENV]
side_effect: R1
determinism: D0
cli: [fid import score-fidelity]
mcp: [import_score_fidelity]
journal: [import.score_fidelity@1]
updated: 2026-11-16
---

# IO.CORP.SCORE: Round-trip and fidelity metrics

## Purpose

Computes, per format and per corpus file, the numbers that say how well a reader and writer preserve data: round-trip equality after the format's documented normaliser, maximum coordinate difference, entity counts preserved and lost by type, and metadata fields preserved and lost. Exposes this as an R1 command so anyone can score their own files.

## Scope

**In:**
- `import.score_fidelity@1` (R1): file or manifest in, metrics JSON out; runs in scratch, never touching the working document
- Round-trip pipeline import → export → import in scratch, compare entities under the format's normaliser hook
- Metrics: `roundtrip_equal: bool`, `max_coord_diff_m`, `entities{type: {kept, lost}}`, `metadata{field: kept|lost}`
- Per-format aggregate: round-trip score = equal files / accepted files
- The `FormatNormaliser` hook formats implement (e.g. `qif-norm@1` for QIF)

**Out:**
- Reports and the CI gate (`IO.CORP.GATE`)
- Format-specific normalisers (format children supply them)

## Requirements

- **REQ-IO.CORP.SCORE-001** For every format with both reader and writer, the command shall compute the round-trip score as the fraction of accepted corpus files whose import → export → import yields equal entities after the format's normaliser.
- **REQ-IO.CORP.SCORE-002** For each scored file, the result shall state the maximum coordinate difference in metres, entity counts kept and lost by type, and metadata fields kept and lost.
- **REQ-IO.CORP.SCORE-003** The command shall run entirely in scratch state and shall not change the working document or write outside a scratch directory.
- **REQ-IO.CORP.SCORE-004** If a format has no writer, then the result shall report `roundtrip: not_applicable` and still report import-side counts against the manifest's expected counts.
- **REQ-IO.CORP.SCORE-005** For hostile entries, the result shall report whether the reader returned exactly the expected error.

## Acceptance criteria

- **AC-1** Given the STL manifest `fixtures/io/corp/score/stl.toml` (cube binary, cube ASCII, one hostile truncated file), when `fid import score-fidelity --manifest ... --json` runs, then round-trip score is 1.0 over 2 accepted files, `max_coord_diff_m` is 0 for binary, and the hostile entry shows `expected_error_match: true`.
- **AC-2** Given a planted lossy test writer that drops the third vertex coordinate's last bit, then `roundtrip_equal: false` and `max_coord_diff_m` > 0 is reported for each file.
- **AC-3** Given a PTX manifest (reader only), then `roundtrip: not_applicable` and counts are compared with expected counts.
- **AC-4** Given a working document open in the session, when the command runs, then the document's journal head is unchanged.
- **AC-5** (replay) Given the same manifest and inputs, two runs give byte-identical result JSON (RFC 8785).

## Design

- **Registry command:** `import.score_fidelity@1`, R1; params `format?: FormatId`, `source?: RootHandle`, `manifest?: RootHandle` (one of source or manifest), `limits?`; result `FidelityResult { format, files: [FileScore], roundtrip_score?: Decimal }`.
- **Data structures:** `FileScore { sha256, roundtrip_equal?, max_coord_diff_m?, entities, metadata, expected_error_match? }`; trait `FormatNormaliser { fn normalise(&self, doc) -> NormalisedDoc }`.
- **Algorithm:** scratch document per file; coordinate difference computed by pairing entities by stable ID or index order, max over |Δ| in f64 with fixed-order reduction; score as exact rational rendered as decimal string.
- **Errors:** `format_unknown`, `manifest_missing`, IO.REG errors.
- **Determinism:** D0.
- **Performance budget:** dominated by the readers; overhead < 10% [unverified].
- **Crate:** `fid-io-corp`.

## Test fixtures

`fixtures/io/corp/score/` manifests over fixtures from `IO.MESH.STL` and `IO.CLOUD.PTX` (CC-BY-4.0); planted test writer.

## Build prompt

> In crate `fid-io-corp`, implement registry command `import.score_fidelity@1` (R1; CLI `fid import score-fidelity`; MCP `import_score_fidelity`): for a file or manifest and format, run import → export → import in a scratch document (never the working document), compare under the format's `FormatNormaliser`, and report per file: round-trip equality, max coordinate difference in metres (fixed-order f64 reduction), entities kept/lost by type, metadata fields kept/lost, and for hostile entries whether the exact expected error was returned; aggregate the round-trip score as accepted-equal / accepted. Reader-only formats report `not_applicable`. Output canonical JSON (RFC 8785), byte-identical across runs. No panics, no unwrap; no std HashMap (CONSTITUTION §8). Tests AC-1..AC-5.

## Open questions

- Metadata field lists per format are declared by the format leaves; until then only geometry and counts are scored.

## Children

_Leaf: none._
