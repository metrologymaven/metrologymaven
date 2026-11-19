---
id: GDT.RES.RESOLVE
title: Pure interpretation resolver
level: 3
parent: GDT.RES
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-gps, fid-gdt]
satisfies: [REQ-GDT.RES-001, REQ-GDT.RES-002, REQ-GDT.RES-003, REQ-GDT.RES-004, REQ-GDT.RES-008]
covers: []
depends_on: [GDT.RES.SCHEMA]
side_effect: R1
determinism: D0
cli: [fid gdt resolve-chain, fid gdt get-dmt-row]
mcp: [gdt_resolve_chain, gdt_get_dmt_row]
journal: [gdt.resolve_chain@1, gdt.get_dmt_row@1]
updated: 2026-11-16
---

# GDT.RES.RESOLVE: Pure interpretation resolver

## Purpose

The resolver maps (characteristic, context, governing spec, DMT) to (row ID, operator chain, chain digest, DMT digest), or to `UNRESOLVED` / `UNSUPPORTED`. It is a pure lookup and selection over data: it never guesses and never encodes an interpretation in code. Every evaluating child calls it first and passes its identifiers into the ResultRecord.

## Scope

**In:** key derivation from a characteristic and its context; exact-key lookup in a loaded `DmtTable`; status handling (`supported`, `partial` with limitations, `unsupported`); variant selection (explicit characteristic override, else defaults-profile choice, else row default); `gdt.resolve_chain@1` (R1) and `gdt.get_dmt_row@1` (R0).
**Out:** the schema and digests (owned by `GDT.RES.SCHEMA`); row content (owned by `GDT.RES.ROWASME` and `GDT.RES.ROWISO`); executing chains (owned by the evaluating children); governing-spec validation (owned by `GDT.CHAR.SPEC`).

## Requirements

- **REQ-GDT.RES.RESOLVE-001** The resolver shall be a pure function of (characteristic type, modifier set, dialect, edition set, defaults-profile keys, override, DMT table) and shall return bit-identical output for identical input on every supported platform.
- **REQ-GDT.RES.RESOLVE-002** When exactly one row matches the derived key, the resolver shall return its row ID, the selected variant's operator chain, the chain digest and the table digest.
- **REQ-GDT.RES.RESOLVE-003** If no row matches, then the resolver shall return `UNRESOLVED` carrying the derived key, and shall not fall back to a near match.
- **REQ-GDT.RES.RESOLVE-004** If the matching row's status is `unsupported`, then the resolver shall return `UNSUPPORTED` with the row ID; if `partial`, it shall return the chain plus the row's limitation codes.
- **REQ-GDT.RES.RESOLVE-005** The resolver shall select the variant named by an explicit characteristic override first, otherwise by the defaults profile, otherwise the row's default, and shall record the chosen variant name and its source (`override`, `profile`, `row_default`) in the output.
- **REQ-GDT.RES.RESOLVE-006** If the override or profile names a variant the row does not have, then the resolver shall return `unknown_variant` and no chain.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/dmt/synthetic_table_v1.json` and characteristic fixture `fixtures/gdt/resolve/flatness_asme.json`, when `fid gdt resolve-chain --char <handle> --json` runs, then the result names row `dmt:asme/flatness/none@1`, its default chain, and digests equal to `expected/flatness_asme.json`.
- **AC-2** Given a characteristic with an unknown modifier combination, then the result is `UNRESOLVED` with the derived key and no chain.
- **AC-3** Given the synthetic row marked `unsupported`, then the result is `UNSUPPORTED` with that row ID; given the `partial` row, the result includes its limitation codes.
- **AC-4** Given a row with variants `surface` (default) and `axis`, a profile choosing `axis` and no override, then variant `axis` with source `profile` is returned; adding override `surface` returns `surface` with source `override`.
- **AC-5** (error path) Given override `median`, which the row lacks, then the result is `unknown_variant`.
- **AC-6** (determinism) Running AC-1..AC-4 on all CI platforms yields byte-identical JSON output.
- **AC-7** Given AC-1, when `fid gdt get-dmt-row --key dmt:asme/flatness/none@1 --json` runs, then the row's canonical JSON is returned.

## Design

- **Registry command(s):** `gdt.resolve_chain@1` (R1; params `char: CharHandle` or inline characteristic, `dmt: TableRef`; result `Resolution = Resolved{row, variant, variant_source, chain, chain_digest, dmt_digest, limitations} | Unresolved{key} | Unsupported{row}`); `gdt.get_dmt_row@1` (R0; params `key`; result `DmtRow`). Both `--json`.
- **Data structures:** `ResolveInput` (the tuple in REQ-001); `Resolution` enum above.
- **Algorithm:** derive canonical key; `BTreeMap` exact lookup; status branch; variant precedence override > profile > row default; digests from `GDT.RES.SCHEMA`.
- **Errors:** `unknown_variant`, `governing_spec_missing` (passed through from the gate in `GDT.CHAR.SPEC`), `dmt_not_loaded`.
- **Determinism:** D0; no floating-point arithmetic.
- **Performance budget:** < 50 µs per resolution on a 5 000-row table [unverified].
- **Crate:** `fid-gps` (module `resolve`).

## Test fixtures

Reuses `synthetic_table_v1.json` from `GDT.RES.SCHEMA` plus small synthetic characteristic JSON files; CC0, no standards content.

## Build prompt

> In crate `fid-gps` (kernel, pure, KNP-1), module `resolve`, implement `resolve(input: &ResolveInput, table: &DmtTable) -> Resolution` using the DMT schema types already in module `dmt`. Derive the canonical key `dmt:<dialect>/<characteristic>/<sorted modifiers|none>@<v>`, do an exact lookup (no fuzzy fallback), return `Unresolved{key}` on miss, `Unsupported{row}` for unsupported rows, and for supported or partial rows the selected variant's chain, its SHA-256 canonical-JSON digest, the table digest and any limitation codes. Variant precedence: characteristic override > defaults-profile choice > row default; record the source; unknown variant is error `unknown_variant`. Register `gdt.resolve_chain@1` (R1, CLI `fid gdt resolve-chain`, MCP `gdt_resolve_chain`) and `gdt.get_dmt_row@1` (R0, CLI `fid gdt get-dmt-row`, MCP `gdt_get_dmt_row`), both with `--json`; the command layer calls the governing-spec gate first. Tests: the acceptance criteria above with the synthetic table, including byte-identical output across CI platforms. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1, no std `HashMap`/`HashSet`, no interpretation logic in code.

## Open questions

- Context beyond characteristic type and modifiers (for example feature type: hole versus pin) may need to enter the key; if so the key grammar in `GDT.RES.SCHEMA` gains a segment and this leaf follows.

## Children

_Leaf: none._
