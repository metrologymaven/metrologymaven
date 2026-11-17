---
id: IO.QIF.XML.XSLT
title: Normative QIF XSLT checks re-expressed as Rust rules
level: 4
parent: IO.QIF.XML
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.XML-001, REQ-IO.QIF.XML-002]
covers: []
depends_on: [IO.QIF.XML.VALID, IO.QIF.GEN.PIN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.XML.XSLT: Normative QIF XSLT checks re-expressed as Rust rules

## Purpose

QIF 3.0 ships normative checks as XSLT 1.0 stylesheets beside the XSDs (about 22 per concept doc 04 [unverified]). This leaf re-expresses each check as a Rust rule over the validated model, keeps a table that traces every rule to its pinned stylesheet and template, and has CI fail if a pinned check has no rule. Re-expression keeps the engine pure Rust (CONSTITUTION §2) and avoids adding a C XSLT engine to the dependency allowlist.

## Scope

**In:**
- Rule table `qif-xslt-rules.toml`: rule ID `qif-xslt:<file>#<template>`, source file digest, a short description in our own words, status `implemented` or `not_implemented`
- One Rust rule per implemented check, reporting violations in the same `Violation` shape as `IO.QIF.XML.VALID`
- A CI check that counts the checks in the pinned stylesheets and compares them with the table
- Import and export reports listing any `not_implemented` checks (honest status)

**Out:**
- Running an XSLT engine (not used)
- XSD validation (`IO.QIF.XML.VALID`)

## Requirements

- **REQ-IO.QIF.XML.XSLT-001** The rule table shall list every check found in the pinned XSLT files, each with its source file, template and file digest.
- **REQ-IO.QIF.XML.XSLT-002** If the pinned XSLT files contain a check that the table does not list, or the table names a file digest that differs from the pin manifest, then CI shall fail with `xslt_check_unmapped`.
- **REQ-IO.QIF.XML.XSLT-003** Every implemented rule shall have one passing and one failing fixture, and its violations shall carry the rule ID `qif-xslt:<file>#<template>`.
- **REQ-IO.QIF.XML.XSLT-004** When any check is `not_implemented`, every import and export report shall list those rule IDs under `checks_skipped`.
- **REQ-IO.QIF.XML.XSLT-005** The rules shall run after XSD validation succeeds and before any entity is staged, and their violations shall be merged into the same first-100 list.

## Acceptance criteria

- **AC-1** Given the pinned QIF 3.0 XSLT files, when the CI check runs, then the number of checks counted equals the number of table rows.
- **AC-2** (error path) Given a test copy of a stylesheet with one extra check template, then CI fails `xslt_check_unmapped`.
- **AC-3** For each implemented rule, given `fixtures/io/qif/xml/xslt/<rule>/pass.qif` and `fail.qif`, then `pass` gives no violation and `fail` gives exactly one with that rule ID.
- **AC-4** Given a table with one rule marked `not_implemented`, when a document is imported, then the import report lists it under `checks_skipped`.

## Design

- **Registry commands:** none (library called by `IO.QIF.XML.CMD` and `IO.QIF.XML.WRITE`).
- **Data structures:** `XsltRule { id, source_file, template, file_sha256, status, check: fn(&Model) -> Vec<Violation> }`.
- **Algorithm:** counting script parses the pinned XSLT with `fid-xml` and lists the check templates [unverified: how checks are marked in the stylesheets]; each Rust rule walks the model through the generated visitor.
- **Errors:** `xslt_check_unmapped` (CI).
- **Determinism:** D0.
- **Performance budget:** all rules together < 20% of XSD validation time [unverified until measured].
- **Crate:** `fid-qif` (module `xml::xslt`).

## Test fixtures

Per-rule pass and fail documents under `fixtures/io/qif/xml/xslt/` (CC-BY-4.0); pinned DMSC XSLT files (Boost [unverified]).

## Build prompt

> In crate `fid-qif`, add module `xml::xslt`: a rule table `qif-xslt-rules.toml` tracing each normative QIF check to its pinned XSLT file, template and file digest; one Rust rule per implemented check over the generated model, emitting `Violation`s with rule ID `qif-xslt:<file>#<template>`; a CI check (parse pinned stylesheets with `fid-xml`, count check templates, compare with the table and the pin manifest; fail `xslt_check_unmapped`); report `checks_skipped` for `not_implemented` rows. No XSLT engine and no C dependency. Descriptions in our own words, never copied stylesheet text. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-4.

## Open questions

- Re-expression in Rust was chosen over an isolated external XSLT engine because it needs no new dependency or process; if a check proves impractical to re-express, it stays `not_implemented` and is reported, rather than adding an engine without a maintainer decision.

## Children

_Leaf: none._
