---
id: GDT.RES
title: Interpretation resolver, DMT and support matrix
level: 2
parent: GDT
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-GDT-001, REQ-GDT-002, REQ-GDT-008]
covers: []
depends_on: [GDT.CHAR, KRN]
cli: [fid gdt resolve-*, fid gdt *-dmt-row, fid gdt *-support-matrix]
mcp: [gdt_resolve_*, gdt_*_dmt_row, gdt_*_support_matrix]
journal: [gdt.resolve_*, gdt.*_dmt_row]
updated: 2026-11-16
---

# GDT.RES: Interpretation resolver, DMT and support matrix

## Purpose

The kernel-side **interpretation resolver**: given a characteristic, its context (feature type, datum references, modifiers) and its governing spec, it selects a row of the versioned **Dialect Mapping Table (DMT)** (name fixed by D-009) and returns a documented **operator chain** in the ISO 17450 operator vocabulary (partition, extraction, filtration, association, collection, construction, reduction, evaluation). Interpretation is data, not code, so a disputed reading becomes a reviewable change to one DMT row. The published support matrix is generated from the same table, so what the project claims and what the kernel does cannot drift apart.

## Scope

**In:**
- DMT schema: row key `dmt:<dialect>/<characteristic>/<modifiers>@v`, clause citations (designator, edition, clause number only), actual-value definition, default association per feature, default decision-rule hint for UNC, ORS case links, status (`supported | partial | unsupported`)
- DMT versioning and digest; DMT rows keyed per standard edition and per defaults profile
- The resolver: deterministic mapping (characteristic, context, governing spec) to (DMT row, operator chain, chain digest)
- Unresolved outcome: callouts that match no row are reported unresolved, never guessed
- Interpretation variants (for example ASME surface versus axis interpretation of position) as named row variants
- Support-matrix generation from the DMT (per dialect, per edition, per callout and modifier combination)
- P1a row set: ASME Y14.5-2018 (+ Y14.5.1-2019 where used) only; ISO 1101, ISO 5459 and ISO 14405-1 rows from P1b (CONSTITUTION §7)

**Out** (and which node owns it instead):
- Characteristic and defaults-profile data (owned by `GDT.CHAR`)
- Executing an operator chain to a value (owned by `GDT.DIM`, `GDT.FORM`, `GDT.LOC`, `GDT.PROF`, `GDT.ZONE`, `GDT.DRF`)
- Fits used inside an association step (owned by `KRN`)
- Decision rules themselves (owned by `UNC`)
- Expert attestation workflow for rows (owned by `TRUST` and `ECO`)
- ORS reference suites (owned by `UNC`)

## Requirements

- **REQ-GDT.RES-001** The resolver shall be a pure function: identical (characteristic, context, governing spec, DMT digest) inputs shall yield a bit-identical operator chain and chain digest (determinism class D0).
- **REQ-GDT.RES-002** When a characteristic resolves, the resolver shall return the DMT row ID, the operator chain, the chain digest (SHA-256 over RFC 8785 canonical JSON) and the DMT digest, and the evaluating child shall pass all three identifiers into the ResultRecord.
- **REQ-GDT.RES-003** If no DMT row matches the characteristic's dialect, edition, characteristic type and modifier set, then the resolver shall return `UNRESOLVED` with the unmatched key, and no value shall be computed for that characteristic.
- **REQ-GDT.RES-004** If a DMT row has status `unsupported` for the requested combination, then the resolver shall return `UNSUPPORTED` with the row ID; if the status is `partial`, it shall return the chain together with the row's listed limitations so they appear in the ResultRecord.
- **REQ-GDT.RES-005** Every DMT row shall cite its basis by standard designator, edition and clause number only, and the DMT shall contain no reproduced standards text (checked in CI by a length limit on free-text fields and a reviewer sign-off) [unverified: lint threshold to be set].
- **REQ-GDT.RES-006** Every DMT row with status `supported` shall link at least one ORS case that exercises it, and CI shall fail when a supported row has none.
- **REQ-GDT.RES-007** The support matrix shall be generated from the DMT on every DMT change, list every (dialect, edition, characteristic, modifier combination) row with its status, and never be edited by hand.
- **REQ-GDT.RES-008** When a row offers interpretation variants, the resolver shall select the variant named by the defaults profile or by an explicit characteristic override, and the chosen variant name shall be recorded in the chain.
- **REQ-GDT.RES-009** The P1a DMT shall contain rows for every ASME Y14.5-2018 callout needed by `GDT.DIM`, `GDT.FORM` and `GDT.LOC` P1a scope, and shall report all ISO rows as `unsupported` until P1b.

## Interfaces

- **Offers:** `gdt.resolve_chain@1` (R1), `gdt.get_dmt_row@1` (R0), `gdt.export_support_matrix@1` (R0 to file handle; X1 only when written outside approved roots), `gdt.propose_dmt_row@1` (W1, agent branch; adoption is human per C5); DMT and operator-chain types; the support matrix (Markdown and JSON) for OUTR and docs.
- **Consumes:** characteristic and governing spec (GDT.CHAR); KRN algorithm IDs for association steps; ORS case IDs (UNC).

## Decomposition hints

SCHEMA (DMT row and operator-chain types), RESOLVE (the pure resolver), ROWS-ASME (P1a row set), ROWS-ISO (P1b row set), MATRIX (generation and publishing), LINT (copyright and ORS-link checks).

## Open questions

- Authorship and review of rows: settled by D-009 (paid expert authors, two-expert sign-off, partner); implemented as a workflow in `GDT.RES.ROWFLOW`.
- Access to standards texts for authors and reviewers: open as D-016, blocking `GDT.RES.ROWASME` and `GDT.RES.ROWISO`.
- Naming: settled by D-009 as "Dialect Mapping Table (DMT)".
- Parent interface lists `gdt.export_support_matrix@1` as R0 to a file handle; CONSTITUTION §3.1 (D-007) makes a local export W1 with `fs_effect: write_root`, which `GDT.RES.MATRIX` follows.
- Children consume `UNC.ORS` case IDs and `TRUST.ESIG` signatures as data through interfaces, not `depends_on`: declaring them creates a dependency cycle (UNC and TRUST reach `GDT.DRF` through `ALN.DATUM`). The cross-domain layering should be reviewed.

## Split rationale

The dividing principle is **structure and machinery versus expert content** (D-009): agents can build and test the schema, resolver, generator, lint and workflow against synthetic rows now, while the row sets are expert-authored and blocked on access to the texts (D-016). Content is split by dialect because the two row sets have different phases, authors and source texts.

Alternatives considered:
- **Hint split (SCHEMA, RESOLVE, ROWS-ASME, ROWS-ISO, MATRIX, LINT).** Adopted, plus ROWFLOW: the D-009 proposal and two-expert sign-off process needs one owner, and putting it in LINT or SCHEMA would mix a stateful workflow with pure checks.
- **One row-set child per tolerance family.** Rejected at this level: row sets are released and reviewed per dialect and edition; per-family splits are left as hints inside ROWASME.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.RES.SCHEMA | DMT row and operator-chain schema (leaf) | 002 005 | none |
| GDT.RES.RESOLVE | Pure interpretation resolver (leaf) | 001 002 003 004 008 | none |
| GDT.RES.ROWASME | P1a ASME Y14.5-2018 DMT row set (decision D-016) | 009 | none |
| GDT.RES.ROWISO | ISO GPS DMT row set (decision D-016) | 009 | none |
| GDT.RES.ROWFLOW | DMT row proposal, review and sign-off workflow | 005 | none |
| GDT.RES.MATRIX | Generated support matrix (leaf) | 007 | none |
| GDT.RES.LINT | DMT copyright and ORS-link lint (leaf) | 005 006 | none |
