---
id: IO.REG.PROV
title: Source provenance, unit and frame capture
level: 3
parent: IO.REG
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-io-reg]
built_pr: 250
satisfies: [REQ-IO.REG-010, REQ-IO.REG-011, REQ-IO.REG-012]
covers: []
depends_on: [IO.REG.LIMITS, GEO.FRAME]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# IO.REG.PROV: Source provenance, unit and frame capture

## Purpose

Builds the `SourceProvenance` record that every import journals (digest, size, format, reader, effective options, unit, frame) and applies the "unit required" rule and the single-rounding unit conversion into SI metres. Format readers hand it their declared unit, if any; this leaf decides the effective unit and converts.

## Scope

**In:**
- Streaming SHA-256 of the source while the reader consumes it
- The `SourceProvenance` type and its canonical JSON form for the journal event
- Effective-unit resolution: declared by the format, given by the caller, or `unit_required`
- Conversion of length values into SI metres with one rounding per value, and the journaled conversion record
- Source frame capture (the frame the file's coordinates are expressed in)

**Out:**
- Writing the journal event (`IO.REG.ENV` through CORE.BUS)
- Frame types and transforms (GEO.FRAME)
- Decimal authored values in QIF (`IO.QIF.JSON`), which are kept lexically and never converted here

## Requirements

- **REQ-IO.REG.PROV-001** When an import succeeds, the provenance record shall hold the source SHA-256, byte size, format ID and version, reader ID and version, and the effective options after defaults are applied.
- **REQ-IO.REG.PROV-002** If the format declares a unit and the caller gives a different one, then the import shall fail with error `unit_conflict` unless the caller sets `override_unit`, which shall be recorded in the provenance.
- **REQ-IO.REG.PROV-003** If the format declares no unit and the caller gives none, then the import shall fail with error `unit_required`.
- **REQ-IO.REG.PROV-004** When a source unit is not metres, each length value shall be converted with a single correctly rounded multiplication by an exact factor (`mm` 1e-3, `um` 1e-6, `in` 0.0254, `ft` 0.3048), and the provenance shall record the source unit and factor.
- **REQ-IO.REG.PROV-005** Every import shall record a source frame ID; when the format carries no frame, the frame shall be a new frame named `source:<digest prefix>` placed at the identity transform of the document's world frame.

## Acceptance criteria

- **AC-1** Given `fixtures/io/reg/prov/cube_mm.xyz` (8 points, no unit) and caller unit `mm`, when provenance is built, then the record's digest equals the file's SHA-256, size is 96 bytes, source unit `mm`, factor `1e-3`, and point (25.4, 0, 0) becomes 0.0254 m.
- **AC-2** (error path) Given the same file with no caller unit, then the import fails with `unit_required`.
- **AC-3** (error path) Given a 3MF fixture declaring `millimeter` and caller unit `in` without `override_unit`, then it fails with `unit_conflict`.
- **AC-4** Given value 1.0 in `in`, then the result is exactly the f64 nearest to 0.0254 (bit pattern `0x3F9A027525460AA6`) [unverified: bit pattern to confirm in the test].
- **AC-5** Given the provenance record of AC-1 serialised to RFC 8785 JSON on two platforms, then the bytes are identical.

## Design

- **Registry commands:** none (library used by `IO.REG.ENV`).
- **Data structures:** `SourceProvenance { sha256, bytes, format: FormatId, format_version, reader: ReaderId, reader_version, options: CanonicalJson, unit: LengthUnit, unit_factor, unit_overridden: bool, frame: FrameId }`; `HashingSource<R>` wrapping `BoundedSource`.
- **Algorithm:** hash while reading; resolve unit by the table in REQ-002/003; convert with one f64 multiplication by the exact factor (all factors are exact decimals; the product is correctly rounded by IEEE-754). No FMA (KNP-1).
- **Errors:** `unit_required`, `unit_conflict`.
- **Determinism:** D0.
- **Performance budget:** hashing at ≥ 1 GB/s on reference hardware [unverified until measured].
- **Crate:** `fid-io-reg`.

### Pinned at build (fid-io-reg module `prov`)

These pins were made when the leaf was built. Where one differs from the text above, the pin wins, and the reason is given.

- **Single rounding, done exactly (supersedes "one f64 multiplication").** `0.001`, `0.000001` and `0.0254` are not `f64` values, so `x * fl(0.001)` rounds twice: once for the factor and once for the product. A reference set of 100,000 random values showed this differs from the correctly rounded result in about 14 % of `mm` cases and 26 % of `in` cases. CONSTITUTION §4 requires a single rounding. `to_metres(x, unit)` therefore returns the `f64` nearest to the exact product of the `f64` `x` and the unit's exact rational factor (`LengthUnit::ratio` of `fid-geo-frame`, `GEO.FRAME.UNITS`), ties to even, subnormals included. It uses `u128` integer arithmetic (`m * p * 2^64 / q`, with a sticky remainder), and no float multiply, divide or FMA touches the value. A zero result keeps the sign of `x`. Metres are returned unchanged. NaN and infinity are refused (`NonFiniteLength`). AC-1 and AC-4 hold unchanged: 25.4 mm and 1 in both give `0x3F9A027525460AA6`.
- **Units.** The `LengthUnit` of `fid-geo-frame` is reused: `m`, `mm`, `um`, `nm`, `in`, `ft` and `thou`, with exact ratios 1, 1/10^3, 1/10^6, 1/10^9, 127/5000, 381/1250 and 127/5000000. The record writes the factor as an exact decimal string: `1`, `0.001`, `0.000001`, `0.000000001`, `0.0254`, `0.3048`, `0.0000254`. Format readers map their own unit words (3MF `millimeter`, STEP contexts) to these units.
- **Resolution table.** `resolve_unit(declared, given, override_unit)`:
  - no declared unit, no given unit: `unit_required`, whatever `override_unit` is;
  - no declared unit, given G: G, source `caller`;
  - declared D, no given unit, without `override_unit`: D, source `declared`;
  - declared D, no given unit, with `override_unit`: `unit_required` (the override names no unit);
  - declared D, given D: D, source `declared`;
  - declared D, given G different from D, without `override_unit`: `unit_conflict`;
  - declared D, given G different from D, with `override_unit`: G, source `override`.
  `unit_overridden` is true exactly when the source is `caller` or `override`, that is, when the unit is not one the file declared. This also meets `IO.CAD.STEP.HDR` AC-4. `unit_conflict` is added to the shared `IoError` taxonomy after `unit_required`.
- **Hashing.** `HashingSource<R>` wraps a `BoundedSource<R>` and hashes every byte it delivers. `finish()` reads whatever the reader left unread to the end, through the same bounded source and charged to the byte limit. So `sha256` and `bytes` always describe the whole file, never only the part a reader consumed. A file over the byte limit fails `limit_exceeded{bytes}`.
- **Detector result.** `Detection::detect` wraps `Registry::detect` (`IO.REG.DETECT`) and records `format_selected_by`: `caller` when the caller named the format, `content_signature` otherwise. Detection errors pass through unchanged.
- **Reader and version.** `reader` is a `ReaderId`, with the `FormatId` alphabet. `reader_version` and `format_version` are version tokens: 1 to 32 bytes of ASCII letters, digits, `.`, `-` and `+`, never `*`. `format_version` is `null` when the reader identified none. Raw header text never goes in a token. It goes in `declared`.
- **Principal.** The record holds the principal kind only, as the journal's `PrincipalKind` wire name (`human`, `agent`, `script`, `service`, `device`, `system`; `CORE.JRN.ENTRY`). The identity stays in the journal entry's salted commitment and is never copied here.
- **Evidence: imported data never looks measured.** `evidence` is `uncharacterized` by default. It is `claimed` when the reader records the file's own statement of origin (`origin_claim`, untrusted text). There is no other value: no import makes content measured, attested or characterized.
- **Untrusted text.** `source_name`, `origin_claim`, `declared` values and a declared frame label are `UntrustedText`. Canonical JSON keeps them exactly, with RFC 8785 string escaping only. The `Display` form shows them only through `as_display()`, inside double quotes with `"` escaped. Text longer than 4,096 bytes (255 bytes for a frame label) is refused with `too_large` and never cut, because the record states exactly what was read.
- **Keys.** `declared` keys are chosen by the reader: lowercase snake_case segments joined by `.`, at most 64 bytes, at most 256 items. A repeated key is `duplicate_key`. Uppercase is refused, so keys never differ only in case. The effective options must be a JSON object of at most 64 KiB of canonical bytes and depth 32. A key repeated in one object is `duplicate_key`, and keys of one object that are equal after ASCII lowercasing are `case_duplicate_key`, at every depth. A second `origin_claim` is `duplicate_key`.
- **Frame.** When the format carries a frame, the record holds its ID and label (`frame_origin: declared`). Otherwise the frame ID is the first 16 bytes of SHA-256(`fid.io.reg.prov.source_frame@1` || 0x00 || the 32 digest bytes). Its name is `source:` plus the first 12 hex digits of the digest (`frame_origin: synthesized`). `source_frame()` returns that `Frame` with kind `User` and parent `FrameId::WORLD` at `Transform::IDENTITY`, for `IO.REG.ENV` to insert. The ID is deterministic (D0), so re-importing the same file gives the same frame.
- **Canonical record (`source_provenance@1`).** An RFC 8785 object whose keys are always all present: `schema`, `sha256` (`sha256:` + 64 hex), `bytes` (decimal string), `source_name` (string or null), `format`, `format_version` (string or null), `format_selected_by`, `reader`, `reader_version`, `principal`, `options` (object), `unit` (symbol), `unit_factor` (decimal string), `unit_source`, `unit_overridden` (bool), `frame` (32 hex), `frame_name` (string or null), `frame_origin`, `evidence`, `origin_claim` (string or null) and `declared` (object of strings). The record has no floats.
- **Errors.** `ProvError` codes: `duplicate_key`, `case_duplicate_key`, `invalid` and `too_large`. Unit errors are `IoError::UnitRequired` and `IoError::UnitConflict`.
- **Fixtures.** `fixtures/io/reg/prov/make_cases.py` (stdlib only: `hashlib`, `fractions`, `json`, `random` with a fixed seed) writes `cube_mm.xyz` and `expected.txt`: the digest and size, the converted cube corners, 1,400 conversion cases over all seven units (fixed edge values, random bit patterns and a realistic range), the synthesized frame, and the AC-5 canonical record. CI runs it with `--check`. All data is synthetic.
- **AC-3 without a 3MF file.** `IO.MESH.TMF` is not built yet, so there is no 3MF reader or test helper. AC-3 is tested at the boundary this leaf owns: declared `mm` (what a 3MF `unit="millimeter"` maps to) and given `in` give `unit_conflict`. No 3MF fixture is committed.

## Test fixtures

`fixtures/io/reg/prov/cube_mm.xyz` (synthetic, CC-BY-4.0) and a minimal 3MF declaring millimetres generated by the `IO.MESH.TMF` test helper.

## Build prompt

> In crate `fid-io-reg`, implement `SourceProvenance`, `HashingSource<R>` (streaming SHA-256 over a `BoundedSource`) and `resolve_unit(declared: Option<LengthUnit>, given: Option<LengthUnit>, override_unit: bool) -> Result<(LengthUnit, f64), IoError>` with errors `unit_required` and `unit_conflict`. Conversion to metres is one f64 multiplication by an exact factor (mm 1e-3, um 1e-6, in 0.0254, ft 0.3048), no FMA (KNP-1). When no frame is given, create frame ID `source:<first 12 hex of digest>` at identity in world. Provenance serialises to RFC 8785 canonical JSON. No panics, no unwrap (CONSTITUTION §8). Tests AC-1..AC-5.

## Open questions

- Should a caller-given unit on a format that declares one be an error (as here) or a warning? Chosen as error to fail closed.

## Children

_Leaf: none._
