---
id: AGENT.SKILLS.QUERY
title: List and show recipe commands
level: 3
parent: AGENT.SKILLS
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-AGENT.SKILLS-008]
covers: []
depends_on: [CORE.REG.DEF, AUTO.MCP.TAINT]
uses_artifacts: [AGENT.SKILLS.RECIPE, AGENT.SKILLS.CATALOG]
side_effect: [R0, R0]
determinism: D0
cli: [fid agent list-recipes, fid agent show-recipe]
mcp: [agent_list_recipes, agent_show_recipe]
journal: [agent.list_recipes@1, agent.show_recipe@1]
updated: 2026-11-17
---

# AGENT.SKILLS.QUERY: List and show recipe commands

## Purpose

Lets an agent fetch worked recipes on demand, so Skill bodies stay short. Two read-only commands list the recipes shipped with the installed manifest and return one recipe, with every content-derived string marked untrusted.

## Scope

**In:**
- `agent.list_recipes@1` (R0) and `agent.show_recipe@1` (R0)
- Filtering by server profile and workflow

**Out** (and which node owns it instead):
- Recipe files and their checks (`AGENT.SKILLS.RECIPE`)
- The catalog (`AGENT.SKILLS.CATALOG`)

## Requirements

- **REQ-AGENT.SKILLS.QUERY-001** `agent.list_recipes@1` shall be class R0 and shall return recipe ID, title, workflow ID and catalog version for every recipe shipped with the running manifest, optionally filtered by profile or workflow, sorted by recipe ID.
- **REQ-AGENT.SKILLS.QUERY-002** `agent.show_recipe@1` shall be class R0 and shall return the recipe's prompt, commentary and entries, with each entry given as command ID, MCP tool name and arguments.
- **REQ-AGENT.SKILLS.QUERY-003** Every string in a recipe that came from journal content or prose (prompt, commentary, entity names, argument strings) shall be declared untrusted and returned with the `AUTO.MCP.TAINT` marking.
- **REQ-AGENT.SKILLS.QUERY-004** If the recipe ID is unknown, then `agent.show_recipe@1` shall fail with `unknown_recipe`.
- **REQ-AGENT.SKILLS.QUERY-005** The returned entries shall carry no ResultRecord values, only command IDs, arguments and entity IDs.

## Acceptance criteria

- **AC-1** Given the installed fixture recipes `fixtures/agent/skills/query/recipes/` (3 recipes over 2 workflows), when `fid agent list-recipes --workflow inspect.align_scan --json` runs, then it returns the 2 matching recipes in ID order with catalog versions.
- **AC-2** Given `fid agent show-recipe demo-align --json` over MCP, then prompt, commentary and string arguments are `{"untrusted": true, ...}` and command IDs are plain.
- **AC-3** (error path) Given `fid agent show-recipe nope`, then it fails with `unknown_recipe`.
- **AC-4** Given AC-1, then nothing is written to the Record Journal (D-031).

## Design

- **Registry command(s):** `agent.list_recipes@1` (R0; params `profile?: ProfileId`, `workflow?: WorkflowId`); `agent.show_recipe@1` (R0; param `id: RecipeId`).
- **Data structures:** `RecipeSummary { id, title, workflow_id, catalog_version }`; `RecipeView { id, prompt: Untrusted, commentary: Vec<Untrusted>, steps: Vec<{ command_id, tool_name, args }> }`.
- **Algorithm:** load recipe files bundled with the manifest at startup (read-only resource), join with the catalog index; strip ResultRecord payloads from entries.
- **Errors:** `unknown_recipe`, `unknown_profile`.
- **Determinism:** D0.
- **Performance budget:** < 10 ms per call.
- **Crate:** `fid-agent`

## Test fixtures

Project-authored recipes and catalog index under `fixtures/agent/skills/query/`.

## Build prompt

> In crate `fid-agent` (layer L3), implement registry commands `agent.list_recipes@1` and `agent.show_recipe@1`, both class R0 (CLI `fid agent list-recipes`, `fid agent show-recipe`; MCP `agent_list_recipes`, `agent_show_recipe`). Recipes are bundled read-only resources (format from AGENT.SKILLS.RECIPE) joined with `catalog-index.json`. List returns id, title, workflow and catalog version, filterable by profile and workflow, sorted by id. Show returns prompt, commentary and steps (command ID, tool name, arguments) with no ResultRecord values; every content-derived string is declared untrusted. Unknown ID: `unknown_recipe`. Acceptance: AC-1 to AC-4 of AGENT.SKILLS.QUERY. Constraints: CONSTITUTION §8, C6 (untrusted text), R0 reads go to the Activity Log only (D-031).

## Open questions

- None.

## Children

_Leaf: none._
