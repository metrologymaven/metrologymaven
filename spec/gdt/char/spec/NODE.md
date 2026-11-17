---
id: GDT.CHAR.SPEC
title: Governing spec, dialect selection and refusal
level: 3
parent: GDT.CHAR
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.CHAR-001, REQ-GDT.CHAR-002, REQ-GDT.CHAR-003]
covers: [GOM.GDT.17, PCD.DIM.05, PW.GDT.08]
depends_on: [GDT.CHAR.MODEL, GDT.CHAR.DPROF]
side_effect: W2
determinism: D0
cli: [fid gdt set-governing-spec]
mcp: [gdt_set_governing_spec]
journal: [gdt.set_governing_spec@1]
updated: 2026-11-16
---

# GDT.CHAR.SPEC: Governing spec, dialect selection and refusal

## Purpose

Every characteristic must say which rulebook governs it: one dialect (`ASME` or `ISO`), the standard designators with editions, and one defaults profile pinned by ID, version and digest. This leaf owns that **governing spec** value, the command that sets it, explicit inheritance from plan or tenant defaults, and the gate that every evaluation command calls to refuse a characteristic without a complete governing spec. It is the user-facing form of "ISO or ASME selection" in incumbents, made explicit per characteristic.

## Scope

**In:** `GoverningSpec` type; standard designator grammar (`std:<body>:<designator>:<edition>`); `gdt.set_governing_spec@1`; copy-on-create inheritance from plan default, then tenant default, recorded as inherited with its level; the `require_governing_spec` gate used by every evaluation command.
**Out:** defaults-profile objects and publication (owned by `GDT.CHAR.DPROF`); which interpretation a dialect implies (owned by `GDT.RES`); the characteristic types (owned by `GDT.CHAR.MODEL`).

## Requirements

- **REQ-GDT.CHAR.SPEC-001** A governing spec shall contain exactly one dialect from {`ASME`, `ISO`}, one or more standard designators each with an edition, and one defaults-profile reference made of ID, semantic version and SHA-256 digest.
- **REQ-GDT.CHAR.SPEC-002** If a standard designator's body does not match the dialect (for example `std:iso:1101:2017` under `ASME`), then `gdt.set_governing_spec@1` shall reject it with error `dialect_mismatch`.
- **REQ-GDT.CHAR.SPEC-003** If an evaluation command is invoked on a characteristic whose governing spec is missing, incomplete, or references a defaults profile not present in the project's profile store with a matching digest, then the gate shall return a typed error naming the missing or bad field and the command shall emit no ResultRecord.
- **REQ-GDT.CHAR.SPEC-004** When a characteristic is created and a plan default governing spec exists, the system shall copy it into the characteristic and journal `inherited_from: plan`; when only a tenant default exists it shall copy that and journal `inherited_from: tenant`; when neither exists the characteristic shall be created with no governing spec.
- **REQ-GDT.CHAR.SPEC-005** A later change to a plan or tenant default shall not alter any existing characteristic's governing spec.
- **REQ-GDT.CHAR.SPEC-006** When `gdt.set_governing_spec@1` succeeds, the system shall journal one event containing the characteristic ID, the old and new governing spec, and `inherited_from: explicit`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/spec/plan_asme.fidproj` (plan default ASME Y14.5-2018 with profile `dp:asme-y14.5-2018@1.0.0`), when `fid gdt create-characteristic --json` creates a flatness characteristic, then its governing spec equals the plan default and the journal event carries `inherited_from: plan`.
- **AC-2** Given the same project, when `fid gdt set-governing-spec --char <id> --dialect ISO --std std:iso:1101:2017 --profile <iso ref> --json` runs, then the spec is replaced and one `gdt.set_governing_spec@1` event is journaled with old and new values.
- **AC-3** (error path) Given `--dialect ASME --std std:iso:1101:2017`, when the command runs, then it fails with `dialect_mismatch` and journals nothing.
- **AC-4** (error path) Given fixture `fixtures/gdt/spec/no_spec.fidproj` with a characteristic lacking a governing spec, when any `check.eval_*` command is invoked on it, then it fails with `governing_spec_missing` naming field `dialect`, and no ResultRecord exists afterwards.
- **AC-5** (error path) Given a profile reference whose digest differs from the stored profile, when the gate runs, then it fails with `defaults_profile_digest_mismatch`.
- **AC-6** Given AC-1, when the plan default is changed to ISO, then the existing characteristic still reads ASME.
- **AC-7** (replay) Given the journal from AC-1 and AC-2, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `gdt.set_governing_spec@1`, W2. Params: `char: CharId`, `dialect: Dialect`, `standards: [StdRef]` (min 1), `defaults_profile: ProfileRef`. Result: the updated `GoverningSpec`. Dry-run returns the validated spec without committing.
- **Data structures:** `GoverningSpec { dialect, standards: Vec<StdRef>, defaults_profile: ProfileRef, inherited_from: Explicit | Plan | Tenant }`; `StdRef { body: Asme | Iso, designator: String, edition: String }` parsed from `std:<body>:<designator>:<edition>`; `ProfileRef { id, version: SemVer, digest: Sha256 }`.
- **Algorithm:** validation is a pure function `validate(spec, profile_store) -> Result<(), SpecError>`; the gate `require_governing_spec(char, store)` wraps it and is called first by every evaluation command. Inheritance is a copy at creation, never a live reference.
- **Errors:** `governing_spec_missing{field}`, `dialect_mismatch`, `bad_std_ref`, `defaults_profile_unknown`, `defaults_profile_digest_mismatch`.
- **Determinism:** D0; no floating point.
- **Performance budget:** gate < 10 µs per characteristic [unverified].
- **Crate:** `fid-gdt`.

## Test fixtures

Two synthetic project fixtures (`plan_asme.fidproj`, `no_spec.fidproj`) and two synthetic defaults profiles whose content is placeholder keys only (no standards text), all authored in-repo under CC0.

## Build prompt

> In crate `fid-gdt`, implement the governing spec. Types: `Dialect { Asme, Iso }`; `StdRef` parsed from `std:<asme|iso>:<designator>:<edition>`; `ProfileRef { id, version, digest }`; `GoverningSpec { dialect, standards, defaults_profile, inherited_from }`. Implement registry command `gdt.set_governing_spec@1` (side-effect W2; CLI `fid gdt set-governing-spec`; MCP `gdt_set_governing_spec`; journal one event with old and new spec, `inherited_from: explicit`; `--json` and `--dry-run`). Implement `require_governing_spec(char, profile_store) -> Result<&GoverningSpec, SpecError>` for every `check.eval_*` command to call before computing, and creation-time inheritance (plan default, else tenant default, else none) that copies the value and records the level. Errors: `governing_spec_missing{field}`, `dialect_mismatch`, `bad_std_ref`, `defaults_profile_unknown`, `defaults_profile_digest_mismatch`; a rejected command journals nothing and produces no ResultRecord. Acceptance: inheritance from plan journaled; explicit set replaces and journals; ASME with an ISO designator rejected; evaluation on a characteristic without a spec rejected naming the field; digest mismatch rejected; changing the plan default leaves existing characteristics unchanged; exact replay is IDENTICAL. Constraints (CONSTITUTION §8): no `unwrap`/`expect`/`panic`/`unsafe`, bounded parsing that fails closed, no std `HashMap`/`HashSet`, no standards text anywhere in code or fixtures.

## Open questions

- Whether tenant defaults are allowed at all is still open on `GDT.CHAR`; this leaf supports them, and removing tenant inheritance is a one-branch deletion if the steering group says no.

## Children

_Leaf: none._
