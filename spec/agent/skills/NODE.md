---
id: AGENT.SKILLS
title: Curated skill catalog and worked recipes
level: 2
parent: AGENT
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-AGENT-001]
covers: []
depends_on: [AUTO.MCP.SKILLS, AUTO.MCP.SRV, AGENT.PROPOSE, FLOW]
cli: [fid agent list-recipes, fid agent show-recipe]
mcp: [agent_list_recipes, agent_show_recipe]
journal: []
updated: 2026-11-16
---

# AGENT.SKILLS: Curated skill catalog and worked recipes

## Purpose

Decides **what** agents are taught and keeps it true. AUTO.MCP.SKILLS is the generator that turns registry task guides into Skill folders; this node owns the curated catalog of top workflows per MCP server profile, the content standard for the guides that feed it, and the worked recipes (replayable journal excerpts from the acceptance jobs) that show an agent a complete task end to end. It exists separately because the catalog is editorial and cross-domain, while generation is mechanical.

## Scope

**In:**
- The workflow catalog: for each server profile, its top workflows, each mapped to one registry task guide
- The content standard for task guides and recipes (length budget, no values of record, no standards text, ends with a proposal)
- Worked recipes: journal excerpts derived from FLOW golden journals, with the task prompt that produced them
- CI checks that every catalog entry has a guide and a generated Skill, and that every recipe replays on the current manifest
- R0 commands to list and show recipes to agents

**Out** (and which node owns it instead):
- Skill generation, front matter and the `unknown_command` and `forbidden_step` checks (`AUTO.MCP.SKILLS`)
- Server profiles and front-door tools (`AUTO.MCP.SRV`)
- The proposal brief an agent must attach (`AGENT.PROPOSE`)
- Eval tasks that exercise each workflow (`AGENT.EVAL`)
- Golden journals of acceptance jobs (`FLOW`)
- Human tutorials (`OUTR`)

## Requirements

- **REQ-AGENT.SKILLS-001** For each MCP server profile, the catalog shall list that profile's top workflows, with at least 5 workflows for each app profile [unverified that 5 is enough], each mapped to exactly one registry task guide.
- **REQ-AGENT.SKILLS-002** If a catalog workflow has no task guide in the manifest or no Skill generated from it at the current manifest digest, then the CI catalog check shall fail naming the workflow.
- **REQ-AGENT.SKILLS-003** Each curated Skill shall carry a catalog version that changes whenever its guide or recipe changes, together with the manifest digest it was generated from.
- **REQ-AGENT.SKILLS-004** Each recipe shall be an excerpt of a FLOW golden journal, and if replaying it on the current manifest gives an outcome other than `IDENTICAL` or `EQUIVALENT`, then the CI recipe check shall fail naming the recipe and the first divergent entry.
- **REQ-AGENT.SKILLS-005** Guide and recipe prose shall contain no standards text, no numeric value presented as a result, and no instruction to perform an M1, S1, A0 or W3 command directly.
- **REQ-AGENT.SKILLS-006** Every guide whose workflow changes document state shall end with the step that attaches a proposal brief (`AGENT.PROPOSE`) and requests promotion.
- **REQ-AGENT.SKILLS-007** The rendered body of each curated Skill shall stay within 1,500 words, with longer material moved to recipes that the agent fetches on demand [unverified budget].
- **REQ-AGENT.SKILLS-008** `agent.list_recipes@1` and `agent.show_recipe@1` shall be R0 and shall return recipe text with every content-derived string marked untrusted.

## Interfaces

- **Offers:** the catalog file (per profile, workflow to guide ID); recipe files; `agent.list_recipes@1` (R0), `agent.show_recipe@1` (R0); the CI catalog and recipe checks.
- **Consumes:** `AUTO.MCP.SKILLS` generated Skills and manifest digest; `AUTO.MCP.SRV` profile list; `AGENT.PROPOSE` brief command names; FLOW golden journals and replay.

## Decomposition hints

CATALOG (workflow list per profile and its CI check), STYLE (guide and recipe content standard and linter), RECIPE (recipe format, extraction from golden journals, replay check), QUERY (list and show commands).

## Open questions

- Who curates the catalog for each app: the app's domain owners or AGENT owners? Proposed: domain owners propose, AGENT owners review.
- Recipe replay depends on FLOW golden journals that FLOW's children are still defining.

## Children

_None yet: status `open`._
