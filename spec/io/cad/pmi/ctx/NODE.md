---
id: IO.CAD.PMI.CTX
title: Proposed spec context from dialect evidence in the file
level: 4
parent: IO.CAD.PMI
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.PMI-007]
covers: []
depends_on: [IO.CAD.PMI.READ]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.PMI.CTX: Proposed spec context from dialect evidence

## Purpose

Collects evidence in an AP242 file about which GD&T dialect and edition the PMI was authored under (explicit standard references, dialect-specific modifiers or symbols, originating system) and attaches a `ProposedSpecContext` marked `proposed`, with every piece of evidence listed. It never adopts a context; adoption is a human act in GDT (C5, D-031).

## Scope

**In:**
- Evidence sources: document or standard references attached to the model [unverified: how AP242 files carry these], modifiers that exist in only one dialect (from a checked-in evidence table), originating-system string from the header
- `ProposedSpecContext { dialect: AsmeY145 | IsoGps | Unknown, edition: Option<String>, evidence: Vec<Evidence>, status: proposed }`
- Conflicting evidence → `dialect: Unknown` with all evidence listed

**Out:**
- Adopting a spec context and defaults profiles (GDT, human-only)
- The dialect mapping table (GDT, D-009)

## Requirements

- **REQ-IO.CAD.PMI.CTX-001** When the file contains dialect evidence, the system shall attach a `ProposedSpecContext` with status `proposed` listing every evidence item with its entity number or header field.
- **REQ-IO.CAD.PMI.CTX-002** If evidence items point to different dialects, then the proposal shall name `Unknown` and list all items.
- **REQ-IO.CAD.PMI.CTX-003** The system shall never commit a spec context with status `adopted` from an import.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/pmi/ctx/asme_ref.stp` (a standard reference naming ASME Y14.5-2018), when the proposal is built, then `dialect: AsmeY145`, `edition: "2018"`, status `proposed`, one evidence item.
- **AC-2** Given `fixtures/io/cad/pmi/ctx/mixed.stp` (an ISO-only modifier and an ASME reference), then `dialect: Unknown` with both evidence items.
- **AC-3** Given `fixtures/io/cad/pmi/ctx/none.stp`, then no proposal is attached.
- **AC-4** Given any fixture, the committed entities contain no spec context with status `adopted`.

## Design

- **Registry commands:** none (library).
- **Data structures:** `ProposedSpecContext` above; `EVIDENCE_TABLE: &[(StepName, Dialect)]` checked in (names only, no standards text).
- **Algorithm:** scan graph and header for table hits; aggregate; conflicts give `Unknown`.
- **Errors:** none.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-io-pmi`.

## Test fixtures

Hand-written fixtures under `fixtures/io/cad/pmi/ctx/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-pmi`, build a `ProposedSpecContext` (dialect `AsmeY145|IsoGps|Unknown`, optional edition, evidence list with entity numbers or header fields, status always `proposed`) from standard references, dialect-only modifiers (checked-in name table, no standards text) and the originating-system string; conflicting evidence → `Unknown`. Never produce status `adopted`. CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-4.

## Open questions

- The `ProposedSpecContext` shape must match what GDT adopts; GDT owns the adopted `SpecContext` type. If GDT's type differs, this leaf adapts its output to it (IO cannot depend on GDT under §2.1, so the proposal type lives here or in CORE).

## Children

_Leaf: none._
