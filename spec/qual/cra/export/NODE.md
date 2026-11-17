---
id: QUAL.CRA.EXPORT
title: Export-control notes and data-tag vocabulary
level: 3
parent: QUAL.CRA
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.CRA-006, REQ-QUAL.CRA-007]
covers: []
depends_on: []
uses_artifacts: [ECO.REL.PKG.PLAN]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.CRA.EXPORT: Export-control notes and data-tag vocabulary

## Purpose

Publishes, for each released binary, an export-control classification note with its counsel review status and a statement that customers remain responsible for their own export decisions. It also defines the closed vocabulary of data tags that mark a customer project as export-controlled, and the policy statement that such projects' geometry, PMI, points and journals are not projected to MCP unless an A0 administrator enables an export-controlled profile. The vocabulary is published as a data file for the nodes that will enforce it.

## Scope

**In:**
- `compliance/export/notes.toml` schema `qual-export-notes/1`: per binary in the package plan, self-assessment summary, regimes considered (for example EAR, Wassenaar) [unverified], review status `draft|counsel_reviewed`
- Rendered `compliance/export/EXPORT.md` with the customer-responsibility statement
- `compliance/export/data-tags.toml` schema `qual-data-tags/1`: closed list of tag IDs (initially `itar`, `ear`, `cui`, `export_controlled`) with a description and the data classes covered (`geometry`, `pmi`, `points`, `journal`)
- `compliance/export/PROJECTION-POLICY.md`: the default-deny projection policy statement
- Check: every binary has a note; tag IDs match `[a-z_]{2,24}`, are unique and use only the four data classes

**Out** (and which node owns it instead):
- Enforcing tags in MCP projection and storing them on projects (unowned; see Open questions; candidates `AUTO.MCP.PROJ`, `CORE.PRJ.META`)
- Legal classification (counsel)

## Requirements

- **REQ-QUAL.CRA.EXPORT-001** If a binary in the package plan has no export note, or a note lacks a review status, then the check shall fail and name the binary.
- **REQ-QUAL.CRA.EXPORT-002** The rendered export page shall state that customers remain responsible for their own export decisions and shall show each note's review status.
- **REQ-QUAL.CRA.EXPORT-003** If the tag file has a duplicate tag ID, an ID outside `[a-z_]{2,24}`, or a data class outside `geometry`, `pmi`, `points` and `journal`, then the check shall fail.
- **REQ-QUAL.CRA.EXPORT-004** The projection-policy document shall state that a project carrying any listed tag has its geometry, PMI, points and journals withheld from MCP unless an A0 administrator has enabled an export-controlled profile.
- **REQ-QUAL.CRA.EXPORT-005** The tag file shall carry a semantic version, and removing or renaming a tag shall require a major version increase, enforced by comparing with the previous release's file.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/cra/export/ok/` (package plan with 3 binaries, notes and tag file), when `cargo xtask qual export check --plan package-plan.json --notes notes.toml --tags data-tags.toml --previous-tags prev.toml --out out/` runs, then it exits 0 and `out/EXPORT.md` equals the golden file.
- **AC-2** (error path) Given `missing-note/` (`fid-verify` has no note), then it exits 1 naming it.
- **AC-3** (error path) Given `bad-tag/` (tag `ITAR!`), then it exits 1.
- **AC-4** (error path) Given `removed-tag/` (tag `cui` removed with only a minor version bump), then it exits 1.

## Design

- **Registry command(s):** none; repository tool `cargo xtask qual export check --plan <json> --notes <toml> --tags <toml> --previous-tags <toml> --out <dir>` (W1 for rendering).
- **Data structures:** `Note { binary, summary, regimes, review: {status, reviewer_ref} }`; `Tags { version, tags: Vec<{ id, description, classes }> }`.
- **Algorithm:** check, compare tag sets with the previous version, render fixed templates.
- **Errors:** `EXPORT-NOTE(binary)`, `EXPORT-TAG(id)`, `EXPORT-TAG-VERSION`.
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `qual::cra::export`).

## Test fixtures

`fixtures/qual/cra/export/{ok,missing-note,bad-tag,removed-tag}/`; synthetic, CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::cra::export` with `qual export check --plan <package-plan.json> --notes <toml> --tags <toml> --previous-tags <toml> --out <dir>`. Notes schema `qual-export-notes/1` (binary, summary, regimes, review {status draft|counsel_reviewed, reviewer_ref}); tag schema `qual-data-tags/1` (semver version; tags {id matching [a-z_]{2,24}, description, classes from geometry|pmi|points|journal}). Fail on a binary without a note, invalid or duplicate tags, or a removed or renamed tag without a major version increase versus the previous file. Render `EXPORT.md` with each note's status and the customer-responsibility statement, and keep `PROJECTION-POLICY.md` stating default-deny MCP projection of tagged projects' geometry, PMI, points and journals unless an A0 administrator enables an export-controlled profile. No panics, deny unwrap/expect. Tests: AC-1..AC-4 of spec node QUAL.CRA.EXPORT.

## Open questions

- Counsel review is needed for the classification notes (EAR, Wassenaar) before foreign distribution.
- Cross-domain, unowned: no node stores these tags on projects (`CORE.PRJ.META` stores free-form tags only) or enforces them in MCP projection (`AUTO.MCP.PROJ` projects by side-effect class only). The vocabulary is published here for them to read.

## Children

_Leaf: none._
