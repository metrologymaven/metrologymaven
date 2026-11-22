---
id: ALN.DATUM.SYS.GATE
title: Dialect gate and per-datum resolver calls
level: 4
parent: ALN.DATUM.SYS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.DATUM.SYS-001, REQ-ALN.DATUM.SYS-002, REQ-ALN.DATUM.SYS-004, REQ-ALN.DATUM.SYS-007]
covers: []
depends_on: [GDT.RES.RESOLVE, GDT.RES.SCHEMA]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-align]
updated: 2026-11-22
---

# ALN.DATUM.SYS.GATE: Dialect gate and per-datum resolver calls

## Purpose

The pure function that refuses a datum-system alignment unless dialect, edition and defaults profile are stated, then asks GDT's interpretation resolver, once per datum feature in precedence order, for the DMT row, operator chain and digests. It never decides an interpretation itself: whatever the resolver does not resolve stops the alignment.

## Scope

**In:** the explicit-context check; derivation of one resolver query per datum feature (datum feature type, modifier set, precedence slot, dialect, edition, profile keys); pass-through of `UNRESOLVED` and `UNSUPPORTED` with the DMT key; assembly of the `ResolvedDatumChain` that BUILD and CMD record.
**Out:** the resolver and DMT table (`GDT.RES.RESOLVE`, `GDT.RES.SCHEMA`); row content (`GDT.RES.ROWASME`, `GDT.RES.ROWISO`); simulation and frame build (`ALN.DATUM.SYS.BUILD`); recording on the element (`ALN.DATUM.SYS.CMD`).

## Requirements

- **REQ-ALN.DATUM.SYS.GATE-001** If any of dialect, standard edition or defaults profile is absent or empty, then the function shall fail with `dialect_unspecified` naming the missing fields; it shall never substitute a default.
- **REQ-ALN.DATUM.SYS.GATE-002** For each datum feature, in precedence order, the function shall call the resolver with the datum feature's type, modifier set (RMB only in P1a; any other modifier fails `unsupported_modifier_p1a`), dialect, edition set and profile keys.
- **REQ-ALN.DATUM.SYS.GATE-003** If the resolver returns `UNRESOLVED` or `UNSUPPORTED` for any datum, then the function shall fail with that outcome, the datum label and the DMT key, and stop.
- **REQ-ALN.DATUM.SYS.GATE-004** On success the function shall return, per datum, the row ID, selected variant and its source, operator chain, chain digest and limitation codes, plus the DMT table digest and one combined chain digest (SHA-256 over the RFC 8785 canonical JSON of the ordered per-datum chain digests).
- **REQ-ALN.DATUM.SYS.GATE-005** The function shall be a pure, D0 function of its inputs and the loaded DMT table.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/datum/sys/gate_asme.json` (datums A plane, B plane, C plane; ASME Y14.5-2018; the ASME defaults profile; a test DMT table), when `resolve_datum_chain` runs, then three row IDs and a combined digest equal to the fixture's expected value are returned.
- **AC-2** (error path) Given the same input with `defaults_profile` missing, then it fails with `dialect_unspecified{fields: [defaults_profile]}`.
- **AC-3** (error path) Given a test DMT table whose row for the secondary datum is `unsupported`, then it fails with `UNSUPPORTED`, label B and the key.
- **AC-4** Given datum B at MMB in P1a, then it fails with `unsupported_modifier_p1a`.

## Design

- **Registry command(s):** none; R1 library function `resolve_datum_chain(datums: &[DatumRef], ctx: &DialectContext, dmt: &DmtTable) -> Result<ResolvedDatumChain, GateError>`.
- **Data structures:** `DialectContext { dialect: Option<Dialect>, edition: Option<Edition>, defaults_profile: Option<ProfileId> }`; `ResolvedDatumChain { per_datum: [{ label, row_id, variant, variant_source, chain, chain_digest, limitations }], dmt_digest, combined_digest }`.
- **Errors:** `dialect_unspecified`, `unsupported_modifier_p1a`, `UNRESOLVED{label, key}`, `UNSUPPORTED{label, row_id}`.
- **Determinism:** D0.
- **Performance budget:** under 50 µs for three datums [unverified until measured].
- **Crate:** `fid-align-solve` (kernel layer; provisional name), calling the pure resolver in `fid-gps`.

## Test fixtures

`gate_asme.json` and a small test DMT table authored for tests only (rows marked `test_only`, not interpretations), CC0.

## Build prompt

> In crate `fid-align-solve` (pure Rust, no I/O), implement `resolve_datum_chain(datums, ctx, dmt) -> Result<ResolvedDatumChain, GateError>`. Fail `dialect_unspecified` (listing missing fields) when dialect, edition or defaults profile is absent; never default them. For each datum in precedence order call the GDT resolver (`gdt.resolve_chain` pure function in `fid-gps`, GDT.RES.RESOLVE) with datum feature type, modifiers (only RMB in P1a, else `unsupported_modifier_p1a`), dialect, edition and profile keys; pass `UNRESOLVED`/`UNSUPPORTED` through with label and key. Return per datum row ID, variant and source, chain, chain digest, limitations; the DMT digest; and a combined digest = SHA-256 of RFC 8785 canonical JSON of the ordered chain digests. Never encode an interpretation in code. Tests: AC-1..AC-4. Constraints: CONSTITUTION §5 (cite standards by designator only), §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1, D0.

## Open questions

- Layering: this ALN leaf calls GDT's resolver, while CONSTITUTION §2.1 orders ALN before GDT in L2. It is legal only if the resolver's crate (`fid-gps`, pure) is classed L0; see the parent's open questions.

## Children

_Leaf: none._
