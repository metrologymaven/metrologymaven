---
id: KRN.NUM.FMT.REG
title: Formatter registry, dispatch, hex rendering and version guard
level: 4
parent: KRN.NUM.FMT
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-num]
satisfies: [REQ-KRN.NUM.FMT-001, REQ-KRN.NUM.FMT-006]
covers: []
depends_on: [KRN.NUM.FMT.LEN, KRN.NUM.FMT.ANG, KRN.NUM.ERR]
uses_artifacts: [CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.NUM.FMT.REG: Formatter registry, dispatch, hex rendering and version guard

## Purpose

The identity layer of the kernel formatters. It lists every `fmt.<kind>@<major>` as data for the kernel manifest and for consumers that record which formatter produced a reported value, dispatches `fmt::format` to the right implementation, renders f64 bits as IEEE-754 hex for hashed forms, and enforces in CI that any change to any formatter's output on the frozen corpora requires a new major.

## Scope

**In:**
- `fmt::registry() -> &'static [FormatterDescriptor]`: id, kind, major, units, precision bounds, golden-corpus digest
- `fmt::format(x, &FormatSpec) -> Result<Reported, NumError>` dispatch by `FormatterId`, with a typed error for unknown IDs
- `fmt.hexbits@1`: f64 -> `0x` plus 16 lowercase hex digits of its IEEE-754 bits, byte-identical to the `CORE.JRN.CANON` f64 encoding
- The version guard: a test that recomputes every formatter's outputs on its frozen golden corpus and fails if the digest differs from the descriptor's digest while the major is unchanged

**Out** (and which node owns it instead):
- The formatter implementations and their corpora (`KRN.NUM.FMT.LEN`, `KRN.NUM.FMT.ANG`)
- Writing the registry into the kernel manifest (`KRN.MAN.MANI`)
- Locale presentation (`I18N.LOCALE`), report-side formatter selection (`RPT.MODEL.FMT`)

## Requirements

- **REQ-KRN.NUM.FMT.REG-001** The registry shall list every formatter as `FormatterDescriptor { id, kind, major, units, precision_bounds, corpus_digest }`, sorted by id, with no two entries sharing an id.
- **REQ-KRN.NUM.FMT.REG-002** When `fmt::format` receives a `FormatterId` not in the registry, it shall return a typed `unknown_formatter` error naming the id.
- **REQ-KRN.NUM.FMT.REG-003** When CI runs, the version guard shall recompute the SHA-256 of each formatter's outputs over its frozen golden corpus and shall fail if that digest differs from the registry's `corpus_digest` for an unchanged major.
- **REQ-KRN.NUM.FMT.REG-004** `fmt.hexbits@1` shall render any f64, including -0.0 and subnormals, as `0x` followed by the 16 lowercase hex digits of its bits, and shall return a typed error for NaN and infinities.
- **REQ-KRN.NUM.FMT.REG-005** The hex rendering shall match every f64 vector published by `CORE.JRN.CANON` byte for byte.
- **REQ-KRN.NUM.FMT.REG-006** The registry contents and the dispatch results shall be identical on x86-64, ARM64 and wasm32.

## Acceptance criteria

- **AC-1** Given the built crate, when `fmt::registry()` is listed, then it contains `fmt.length@1`, `fmt.scalar@1`, `fmt.angle@1` and `fmt.hexbits@1`, sorted, with non-empty corpus digests.
- **AC-2** (version guard) Given a deliberately altered rounding in a test build of `fmt.length@1` with the major unchanged, when the guard runs over `fixtures/num/fmt/len-golden-v1`, then it fails naming `fmt.length@1`.
- **AC-3** (error path) Given `FormatterId { kind: "length", major: 9 }`, when `fmt::format` runs, then it returns `unknown_formatter` naming `fmt.length@9`.
- **AC-4** Given `fixtures/core/jrn/canon/vectors.json` (f64 entries), when `fmt.hexbits@1` renders each value, then every string equals the vector; 0.0 gives `0x0000000000000000` and -0.0 gives `0x8000000000000000`.
- **AC-5** Given the registry and AC-1..AC-4 on x86-64, ARM64 and wasm32 under wasmtime, then all outputs are identical.

## Design

- **Registry command(s):** none; library API `fid_num::fmt::{registry, format, hexbits, FormatterDescriptor}`. `KRN.MAN.MANI` reads `registry()` at build time.
- **Data structures:** `FormatterDescriptor { id: FormatterId, units: &'static [Unit], precision_bounds: (u8, u8, u8), corpus_digest: &'static str }`, a static sorted array.
- **Algorithm:** dispatch by match on (kind, major). The guard is a test binary that streams each corpus through `format`, hashes outputs in corpus order with SHA-256 (test-only dependency), and compares with the descriptor. A changed digest requires a new major entry; the old major stays listed while any supported release uses it.
- **Errors:** `FmtError::{UnknownFormatter, NonFinite}` in `NumError`.
- **Determinism:** D0.
- **Performance budget:** dispatch overhead < 50 ns [unverified until measured].
- **Crate:** `fid-num` (module `fmt`, file `registry`).

## Test fixtures

The golden corpora owned by LEN and ANG; the f64 vectors in `fixtures/core/jrn/canon/vectors.json` published by `CORE.JRN.CANON`.

## Build prompt

> In crate `fid-num`, module `fmt`, add `registry()` returning a static, id-sorted array of `FormatterDescriptor { id, units, precision_bounds, corpus_digest }` for `fmt.length@1`, `fmt.scalar@1`, `fmt.angle@1` and `fmt.hexbits@1`; `format(x, &FormatSpec) -> Result<Reported, NumError>` dispatching to `fmt::len` and `fmt::ang`, with `UnknownFormatter` for anything else; and `hexbits(x) -> Result<String, NumError>` producing `0x` plus 16 lowercase hex digits of the IEEE-754 bits (error on NaN or infinity). Add a CI test (the version guard) that runs every formatter over its frozen golden corpus (`fixtures/num/fmt/len-golden-v1`, `ang-golden-v1`), hashes the outputs with SHA-256 (dev-dependency only) and fails if the digest differs from `corpus_digest` while the major is unchanged. Test `hexbits` against the f64 vectors in `fixtures/core/jrn/canon/vectors.json`. Constraints: KNP-1 (no FMA, no HashMap/HashSet, no SIMD dispatch, no `unsafe`, no I/O in non-test code), deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.NUM.FMT.REG.

## Open questions

- `CORE.JRN.CANON` already encodes f64 bits as hex for hashing. This leaf keeps `fmt.hexbits@1` because FMT's scope lists hex rendering for hashed forms and reports may show it; if maintainers prefer, it can call the CANON encoding instead, but CANON is L0 too so either direction is layer-legal.

## Children

_Leaf: none._
