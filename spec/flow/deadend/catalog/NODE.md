---
id: FLOW.DEADEND.CATALOG
title: Suggested-fix catalogue format and initial entries
level: 3
parent: FLOW.DEADEND
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-fixes]
satisfies: [REQ-FLOW.DEADEND-001]
covers: [JOB.USE.05]
depends_on: [CORE.JRN.CANON]
uses_artifacts: [UNC.UEC.EXPL, UNC.DEC.DEFAULT, GDT.CHAR.RECORD]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.DEADEND.CATALOG: Suggested-fix catalogue format and initial entries

## Purpose

Defines the one data file that maps every unresolved-state reason code to at least one suggested fix naming a registry command, and ships its first entries. Domains that already publish their own fix table (for example `uec-fix@1` in `UNC.UEC.EXPL`) are referenced by ID rather than copied, so a fix is defined once. The catalogue is a FLOW-owned file for now; moving it into the registry manifest needs a `CORE.REG` field (see Open questions).

## Scope

**In:**
- The `fix-catalogue@1` format (JSON, validated by a published JSON Schema) and its loader with bounded parsing
- Entry fields: reason code, state kind, owning node, fixes (command ID with major, argument template, label key), optional reference to a domain fix table
- Initial entries for the reason codes declared by `UNC.UEC.STMT`/`UNC.UEC.EXPL` (`u_missing`, `model_missing`, `instrument_missing`), `UNC.DEC.DEFAULT` (`rule_missing`) and `GDT.CHAR.RECORD` (`no_rule_bound`, `unc_no_decision`)
- The catalogue digest (RFC 8785 + SHA-256)

**Out:**
- Checking the catalogue against the manifest (owned by `FLOW.DEADEND.CHECK`)
- Resolving fixes for a principal and projecting them into results (owned by `FLOW.DEADEND.RESULT`)
- Reason-code semantics (owned by each emitting domain)
- Fix wording (owned by `UX`; the catalogue holds label keys only)

## Requirements

- **REQ-FLOW.DEADEND.CATALOG-001** Each catalogue entry shall hold `reason_code` (`<domain>.<code>`, lowercase), `state_kind` (one of `not_decidable`, `unresolved`, `lint`, `uec0`, `blocked`), `owner` (a spec node ID) and a non-empty `fixes` list.
- **REQ-FLOW.DEADEND.CATALOG-002** Each fix shall hold `command` (a registry command ID with `@major`), an `args` template whose placeholders are drawn only from the closed set `${entity_id}`, `${characteristic_id}`, `${project_id}`, `${dialect}` and `${reason_detail}`, and a `label_key`.
- **REQ-FLOW.DEADEND.CATALOG-003** If an entry declares `source_table` (for example `uec-fix@1`), then the loader shall expose the entry's fixes in the same order as the referenced table rows it was built from.
- **REQ-FLOW.DEADEND.CATALOG-004** The initial catalogue shall map `decide.rule_missing` and `gdt.no_rule_bound` to `decide.bind_rule@1` and `decide.set_default_rule@1`, `unc.model_missing` to `unc.bind_model@1`, `unc.instrument_missing` to `unc.bind_instrument_model@1` and `unc.list_profile@1`, and `unc.u_missing` to `unc.propagate_auto@1` and `unc.declare_uncertainty@1`.
- **REQ-FLOW.DEADEND.CATALOG-005** If the catalogue file exceeds 4 MiB, contains a duplicate `reason_code`, or fails schema validation, then the loader shall return a typed error and no partial catalogue.

## Acceptance criteria

- **AC-1** Given `fixtures/flow/fixes/catalogue-valid.json` (6 entries), when loaded, then 6 entries are returned in `reason_code` order and the digest equals `fixtures/flow/fixes/catalogue-valid.digest`.
- **AC-2** (error path) Given `catalogue-dup.json` (two `decide.rule_missing` entries), when loaded, then the loader returns `FIX-DUP decide.rule_missing`.
- **AC-3** (error path) Given `catalogue-badph.json` (placeholder `${path}`), when loaded, then it returns `FIX-PLACEHOLDER`.
- **AC-4** Given the shipped `fix-catalogue.json`, when loaded, then the six reason codes of REQ-004 are present with exactly the fixes listed there.

## Design

- **Registry command(s):** none (data file and library).
- **Data structures:** `FixCatalogue { version: "fix-catalogue@1", entries: Vec<FixEntry> }`; `FixEntry { reason_code, state_kind, owner, fixes: Vec<FixTemplate>, source_table: Option<String> }`; `FixTemplate { command, args: BTreeMap<String, String>, label_key }`. Ordered maps only.
- **Algorithm:** bounded JSON parse (4 MiB, depth 32), schema validation, sort by `reason_code`, RFC 8785 digest via `CORE.JRN.CANON`.
- **Errors:** `FIX-SCHEMA`, `FIX-DUP`, `FIX-PLACEHOLDER`, `FIX-SIZE`, `FIX-IO`.
- **Determinism:** D0 (pure data handling, ordered maps).
- **Performance budget:** load of 2 000 entries under 50 ms [unverified].
- **Crate:** `fid-fixes` (L3, FLOW).

## Test fixtures

Hand-written catalogues under `fixtures/flow/fixes/` (project-authored, MIT OR Apache-2.0).

## Build prompt

> Create crate `fid-fixes` (layer L3) with module `catalogue`. Define `fix-catalogue@1`: `{version, entries:[{reason_code, state_kind: not_decidable|unresolved|lint|uec0|blocked, owner, fixes:[{command:"domain.verb_object@N", args:{name: template}, label_key}], source_table?}]}` and publish `schema/fix-catalogue-v1.json` generated from the Rust types (test fails if the checked-in schema differs). Loader: bounded parse (4 MiB, depth 32), schema check, unique `reason_code`, placeholders limited to `${entity_id}`, `${characteristic_id}`, `${project_id}`, `${dialect}`, `${reason_detail}`; sorted output; RFC 8785 + SHA-256 digest using the project's canonical JSON crate. Ship `fix-catalogue.json` with: `decide.rule_missing` and `gdt.no_rule_bound` -> `decide.bind_rule@1`, `decide.set_default_rule@1`; `unc.model_missing` -> `unc.bind_model@1`; `unc.instrument_missing` -> `unc.bind_instrument_model@1`, `unc.list_profile@1`; `unc.u_missing` -> `unc.propagate_auto@1`, `unc.declare_uncertainty@1`; `gdt.unc_no_decision` -> `unc.propagate_auto@1`. Errors `FIX-SCHEMA|FIX-DUP|FIX-PLACEHOLDER|FIX-SIZE|FIX-IO`. Tests for AC-1..AC-4 (spec node FLOW.DEADEND.CATALOG). No `unwrap`/`expect`/`panic`/`unsafe`; no std HashMap.

## Open questions

- Resolution of the parent's question: the catalogue stays a FLOW-owned file in `fid-fixes`, and per-domain fix tables are referenced, not copied. Moving fixes into each command's manifest entry (better fit for CONSTITUTION C3) would need a `CORE.REG.DEF` field; raise it with CORE if the catalogue drifts.
- The `gdt.unc_no_decision` fix is a guess at the most useful command [unverified with GDT].

## Children

_Leaf: none._
