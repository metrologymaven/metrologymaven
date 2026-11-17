---
id: AGENT.SKILLS.CATALOG
title: Workflow catalog per server profile and its CI check
level: 3
parent: AGENT.SKILLS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-AGENT.SKILLS-001, REQ-AGENT.SKILLS-002, REQ-AGENT.SKILLS-003]
covers: []
depends_on: []
uses_artifacts: [CORE.REG.MAN, AUTO.MCP.SKILLS, AUTO.MCP.SRV]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.SKILLS.CATALOG: Workflow catalog per server profile and its CI check

## Purpose

Owns the curated list of top workflows for each MCP server profile, each mapped to one registry task guide, and the CI check that keeps the list true against the current manifest and the generated Skills. It also stamps each curated Skill with a catalog version, so an agent run can say exactly which teaching it had.

## Scope

**In:**
- The catalog file `agent/catalog.json`: profile, workflow ID, title, task guide ID, optional recipe IDs
- Catalog versions per workflow
- The CI catalog check

**Out** (and which node owns it instead):
- Generating Skills from guides (`AUTO.MCP.SKILLS`)
- Guide and recipe content rules (`AGENT.SKILLS.STYLE`)
- Recipe files and replay (`AGENT.SKILLS.RECIPE`)
- Server profile definitions (`AUTO.MCP.SRV`)

## Requirements

- **REQ-AGENT.SKILLS.CATALOG-001** The catalog shall list, for each server profile in the manifest, its curated workflows, each with a workflow ID, a title and exactly one task guide ID.
- **REQ-AGENT.SKILLS.CATALOG-002** If an app profile lists fewer than 5 workflows [unverified that 5 is enough], then the catalog check shall fail naming the profile.
- **REQ-AGENT.SKILLS.CATALOG-003** If a catalog workflow names a task guide absent from the manifest, a guide whose profile differs from the workflow's profile, or a guide with no generated Skill carrying the current manifest digest, then the catalog check shall fail naming the workflow and the reason.
- **REQ-AGENT.SKILLS.CATALOG-004** Each workflow's catalog version shall be the SHA-256 digest of the canonical JSON of its catalog row, its task guide declaration and its recipe files, and the check shall write a version index mapping workflow ID to catalog version and manifest digest.
- **REQ-AGENT.SKILLS.CATALOG-005** If two catalog rows share a workflow ID, or a profile in the catalog is absent from the manifest, then the check shall fail naming them.

## Acceptance criteria

- **AC-1** Given fixtures `fixtures/agent/skills/manifest-ok.json`, `catalog-ok.json` and generated Skills under `fixtures/agent/skills/skills-ok/`, when the catalog check runs, then it passes and writes `catalog-index.json` equal to the golden file.
- **AC-2** (error path) Given `catalog-missing-guide.json` naming guide `demo.ghost`, then the check fails with `missing_guide` naming the workflow.
- **AC-3** (error path) Given Skills generated from an older manifest digest, then the check fails with `stale_skill`.
- **AC-4** (error path) Given an app profile with 4 workflows, then the check fails with `too_few_workflows`.
- **AC-5** Given a one-character change to a recipe file of workflow W, then only W's catalog version changes.

## Design

- **Registry command(s):** none; a CI check run as `cargo run -p fid-agent --bin agent-catalog-check` (wired into CI by ECO.CI).
- **Data structures:** `CatalogRow { profile, workflow_id, title, guide_id, recipes: Vec<RecipeId> }`; `CatalogIndex { manifest_digest, rows: Vec<{ workflow_id, catalog_version }> }`.
- **Algorithm:** load manifest (CORE.REG.MAN format) and profile list (AUTO.MCP.SRV declarations); load catalog; check rows; read each generated `SKILL.md` front matter for its manifest digest (AUTO.MCP.SKILLS output); compute digests over RFC 8785 canonical JSON; write the index sorted by workflow ID.
- **Errors:** `missing_guide`, `profile_mismatch`, `stale_skill`, `missing_skill`, `too_few_workflows`, `duplicate_workflow`, `unknown_profile`.
- **Determinism:** D0.
- **Performance budget:** < 2 s for 500 workflows [unverified].
- **Crate:** `fid-agent` (check binary; reads files only)

## Test fixtures

Project-authored fixture manifests, catalogs and generated Skills under `fixtures/agent/skills/`.

## Build prompt

> In crate `fid-agent`, add a CI check binary `agent-catalog-check`. Inputs: the registry manifest JSON, the generated Skills folder, and `agent/catalog.json` (rows: profile, workflow_id, title, guide_id, recipes). Fail with typed errors when a guide is missing (`missing_guide`), its profile differs (`profile_mismatch`), the Skill is missing or names another manifest digest (`missing_skill`, `stale_skill`), an app profile has fewer than 5 workflows (`too_few_workflows`), IDs repeat (`duplicate_workflow`) or a profile is unknown (`unknown_profile`). Write `catalog-index.json` mapping each workflow to its catalog version (SHA-256 over RFC 8785 canonical JSON of row + guide declaration + recipe files) and the manifest digest. Acceptance: AC-1 to AC-5 of AGENT.SKILLS.CATALOG. Constraints: CONSTITUTION §8 (no panic, deny `unwrap` etc., bounded parsing); deterministic ordering (no std HashMap/HashSet in outputs).

## Open questions

- Curation: domain owners propose catalog rows for their app, AGENT owners review (proposed; maintainers to confirm in governance).

## Children

_Leaf: none._
