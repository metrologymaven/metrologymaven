---
id: MIG.TEMPL.ADOPT
title: Agent provenance and human adoption of form maps
level: 3
parent: MIG.TEMPL
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-MIG.TEMPL-005]
covers: []
depends_on: [MIG.TEMPL.MAP, CORE.BUS.PRIN, CORE.JRN.CANON]
side_effect: W2
determinism: D0
cli: [fid migrate adopt-form-map]
mcp: [migrate_adopt_form_map]
journal: [migrate.adopt_form_map@1]
updated: 2026-11-17
---

# MIG.TEMPL.ADOPT: Agent provenance and human adoption of form maps

## Purpose

Implements CONSTITUTION C5 for form maps. A form map drafted or changed by an agent is `proposed` and lives on the agent's branch; it cannot fill a release-labelled form or be used by a released report until a human adopts it with `migrate.adopt_form_map@1`. The command is `human_only` (D-031), so agents and scripts can only request it. Adoption records who adopted which exact digest; any later agent edit returns the form map to `proposed`.

## Scope

**In:**
- Provenance states for form maps: `human`, `proposed`, `adopted`
- `migrate.adopt_form_map@1` (W2, `human_only: true`): adopt a form-map template version by digest, bringing it from the agent branch to the working branch
- `form_map_release_block(template_version)` query used by `MIG.TEMPL.FILL` and by RPT release checks

**Out:**
- Creating form maps (owned by `MIG.TEMPL.MAP`)
- Branch promotion mechanics (owned by `CORE.JRN.BR`) and principals (owned by `CORE.BUS.PRIN`)

## Requirements

- **REQ-MIG.TEMPL.ADOPT-001** When a human principal runs `migrate.adopt_form_map@1` with a template version and its expected form-map digest, the system shall set the form map's provenance to `adopted` with adopter, time and digest, and make it available on the working branch.
- **REQ-MIG.TEMPL.ADOPT-002** If the principal is not human, then the command shall fail with `adopter_not_human` and journal nothing.
- **REQ-MIG.TEMPL.ADOPT-003** If the expected digest differs from the form map's current digest, then the command shall fail with `digest_mismatch` and journal nothing.
- **REQ-MIG.TEMPL.ADOPT-004** When an agent modifies an `adopted` or `human` form map, the new version's provenance shall be `proposed`.
- **REQ-MIG.TEMPL.ADOPT-005** When `form_map_release_block` is queried, it shall return the template version and reason `proposed` for a `proposed` form map, and nothing otherwise.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/templ/adopt/agent-draft/` (a form map created by `agent-1`), when human `qa-1` runs `fid migrate adopt-form-map --template <v> --expected-digest <d>`, then the form map is `adopted` by `qa-1` on the working branch and the release block is empty.
- **AC-2** (error path) Given the same, when `agent-1` runs the command, then it fails with `adopter_not_human` and journals nothing; over MCP only a pending human request results.
- **AC-3** (error path) Given a stale digest, then it fails with `digest_mismatch`.
- **AC-4** Given AC-1, when `agent-1` edits a binding, then the new version is `proposed` and the release block lists it.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `migrate.adopt_form_map@1`, W2, `human_only: true` (MCP projection request-only); params `template: TemplateVersionId`, `expected_digest: Sha256`, `note?: UntrustedText`; result `{ provenance, adopted_digest }`.
- **Data structures:** `FormMapProvenance = Human | Proposed { agent, session } | Adopted { by, at, digest }`, stored on the form-map extension block.
- **Algorithm:** principal check → digest check → copy the version from the agent branch to the working branch as one journaled change with provenance `adopted`.
- **Errors:** `adopter_not_human`, `digest_mismatch`, `not_found`.
- **Determinism:** D0.
- **Performance budget:** under 20 ms [unverified until measured].
- **Crate:** `fid-mig` (module `templ::adopt`).

## Test fixtures

`fixtures/mig/templ/adopt/`: small projects with agent-branch form maps produced by the test harness, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `templ::adopt` and register `migrate.adopt_form_map@1` (W2, `human_only: true`; CLI `fid migrate adopt-form-map`, MCP `migrate_adopt_form_map` projected request-only). Implement spec node MIG.TEMPL.ADOPT: provenance `Human | Proposed{agent,session} | Adopted{by,at,digest}` on form maps; adoption requires a human principal (`adopter_not_human`) and the current digest (`digest_mismatch`), journals nothing on failure, and brings the version from the agent branch to the working branch as `adopted`; any agent edit yields a `proposed` version; `form_map_release_block` returns proposed versions. Tests for AC-1..AC-5. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- None.

## Children

_Leaf: none._
