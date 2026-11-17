---
id: GDT.RES.ROWISO.STUBS
title: ISO counterpart map and P1a placeholder rows
level: 4
parent: GDT.RES.ROWISO
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.RES.ROWISO-001]
covers: []
depends_on: [GDT.RES.ROWASME.INV, GDT.RES.SCHEMA]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.RES.ROWISO.STUBS: ISO counterpart map and P1a placeholder rows

## Purpose

Makes the P1a ISO gap explicit. A small human-maintained map names, for each P1a ASME inventory key, the ISO key(s) and edition sets that would cover the same callout; a generator turns the map into the P1a ISO inventory (every key `unsupported_ok`) and into placeholder rows with status `unsupported`, an empty chain and no interpretation. The placeholders then go through the normal propose and two-reviewer workflow with the `not_required_placeholder` attestation, so the released table and support matrix list every ISO counterpart honestly as unsupported.

## Scope

**In:** format `dmt-iso-counterparts/v1` (`{asme_key, iso: [{key, designator, editions}]}` or `{asme_key, iso: none, reason_code}`); generator `stubs(asme_inventory, map) -> (iso_inventory, Vec<DmtRowDraft>)`; placeholder row shape; a CI check that the map covers every ASME inventory key.
**Out:** proposing and reviewing the placeholders (owned by `GDT.RES.ROWFLOW`); the map's content (authored by a human maintainer and reviewed like any repository change); real ISO rows (owned by the P1b siblings).

## Requirements

- **REQ-GDT.RES.ROWISO.STUBS-001** The generator shall emit one placeholder row per distinct ISO key in the map, with status `unsupported`, one variant named `placeholder` holding an empty chain, limitation code `placeholder_no_interpretation`, no ORS links and no free text.
- **REQ-GDT.RES.ROWISO.STUBS-002** Each placeholder shall cite only the standard's designator and edition with the scope clause number `1` [unverified that clause 1 is the scope clause in every cited edition], and the generator shall emit no other clause numbers.
- **REQ-GDT.RES.ROWISO.STUBS-003** The generator shall emit a P1a ISO inventory in `dmt-inventory/v1` listing every generated key with `required_status: unsupported_ok` and its family (GEOTOL, DATUM, MATREQ, SIZE or DPROF).
- **REQ-GDT.RES.ROWISO.STUBS-004** If the map omits an ASME inventory key, or maps it to `none` without a reason code, then the CI check shall fail naming the key.
- **REQ-GDT.RES.ROWISO.STUBS-005** Different editions of the same ISO standard shall produce distinct keys (edition encoded in the key's version or characteristic segment as the schema allows), never one key for two editions.
- **REQ-GDT.RES.ROWISO.STUBS-006** Output shall be byte-identical for identical inputs.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/dmt/iso_map_synthetic.json` (5 ASME keys mapped to 6 ISO keys across two editions of one standard) and a synthetic ASME inventory, when the generator runs, then 6 placeholder drafts and a 6-entry ISO inventory are emitted, each draft parses with the schema and passes draft-mode lint.
- **AC-2** (error path) Given a map missing one ASME key, then the check fails naming it; given `iso: none` without a reason code, then it fails naming that key.
- **AC-3** Given the same map in a different order, then the output is byte-identical.
- **AC-4** Given the placeholders adopted after two `not_required_placeholder` reviews, then `GDT.RES.ROWASME.GATE` with the ISO inventory passes and the support matrix lists each as `unsupported`.

## Design

- **Registry command(s):** none; library plus a repository tool target that writes the drafts for a human to propose with `fid gdt propose-dmt-row`.
- **Data structures:** `CounterpartMap { entries: BTreeMap<DmtKey, Counterpart> }`; placeholder `DmtRowDraft`.
- **Algorithm:** load and check coverage; collect distinct ISO keys in key order; build drafts and the inventory.
- **Errors:** `map_incomplete{key}`, `reason_missing{key}`, `bad_dmt_key`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-gdt` (module `dmt_iso_stubs`).

## Test fixtures

Synthetic maps and inventories (invented keys where needed; CC0, in-repo).

## Build prompt

> In crate `fid-gdt`, add module `dmt_iso_stubs`. Define `dmt-iso-counterparts/v1` (`asme_key` → list of `{key, designator, editions}` or `none` with a `reason_code`). Implement `stubs(asme_inv: &Inventory, map: &CounterpartMap) -> Result<(Inventory, Vec<DmtRowDraft>), StubError>`: fail `map_incomplete{key}` if an ASME inventory key is not mapped, `reason_missing{key}` for `none` without a reason; one placeholder per distinct ISO key: status `unsupported`, single variant `placeholder` with an empty chain, limitation `placeholder_no_interpretation`, no ORS links, no free text, clause references = designator + edition + clause `1` only; distinct keys per edition; ISO inventory entries `required_status: unsupported_ok` with family GEOTOL, DATUM, MATREQ, SIZE or DPROF; deterministic output. Add a tool target that writes the drafts for a human to propose. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`, no standards text, no interpretation in any output.

## Open questions

- The scope-clause citation is a workaround for the schema's "at least one clause" rule (parent open question).

## Children

_Leaf: none._
