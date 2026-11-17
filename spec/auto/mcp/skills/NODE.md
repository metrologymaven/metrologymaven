---
id: AUTO.MCP.SKILLS
title: Generated Skills and task guides
level: 3
parent: AUTO.MCP
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.MCP-010]
covers: []
depends_on: [AUTO.SURF.GEN, AUTO.MCP.PROJ]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.MCP.SKILLS: Generated Skills and task guides

## Purpose

Generates agent Skills (folders with a `SKILL.md` and front matter) from task guides that domains declare next to their commands in the registry. A Skill tells an agent which commands to call in which order for a task such as "align a scan to CAD"; it never contains values, standards text or anything not declared in the registry.

## Scope

**In:**
- Skill emitter (surface `skills`) and its index
- Validation that every command a guide names exists in the manifest at the stated major and is served by the guide's profile
- Validation that guides never instruct a class the agent cannot perform (M1, S1, A0, W3 performed directly)

**Out:**
- Writing the guide content (each domain, in its registry declarations)
- Tool definitions (`AUTO.MCP.PROJ`)
- Human tutorials (`OUTR.DOCS`)

## Requirements

- **REQ-AUTO.MCP.SKILLS-001** The emitter shall write one Skill folder per declared task guide, with front matter `name` and `description` and a body built only from the guide's declared fields and fixed templates.
- **REQ-AUTO.MCP.SKILLS-002** If a guide names a command ID or major absent from the manifest, then generation shall fail with `unknown_command` naming the guide and command.
- **REQ-AUTO.MCP.SKILLS-003** If a guide names an M1, S1 or A0 command, then generation shall fail with `forbidden_step`; a W3, X1 or D1 step shall be rendered as "file a request for a human".
- **REQ-AUTO.MCP.SKILLS-004** Each Skill shall name the manifest digest it was generated from.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/manifest/guides.json` with guide `demo.align_scan` naming three commands, when surfaces are generated, then `skills/demo-align-scan/SKILL.md` equals the golden file and names the manifest digest.
- **AC-2** (error path) Given a guide naming `demo.ghost_cmd@1`, then generation fails with `unknown_command`.
- **AC-3** (error path) Given a guide naming the M1 command `demo.move_axis@1`, then generation fails with `forbidden_step`.
- **AC-4** Given a guide step on the W3 command `demo.release_plan@1`, then the rendered step says it files a request for a human.

## Design

- **Registry command(s):** none.
- **Data structures:** `TaskGuide { id, title, summary, steps: Vec<GuideStep { command: CommandRef, purpose: String }>, profile }` read from the manifest (declared by `CORE.REG`).
- **Algorithm:** validate, then render a fixed Markdown template; Skill folder name is the guide ID with dots and underscores to hyphens.
- **Errors:** `unknown_command`, `forbidden_step`, `profile_mismatch`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-mcp`

## Test fixtures

`fixtures/manifest/guides.json` (project-authored) with guides referencing fixture commands; golden Skill files under `fixtures/mcp/skills/`.

## Build prompt

> In crate `fid-mcp`, implement a Skills emitter (surface `skills`). For each task guide declared in the manifest, write `skills/<guide-id-with-hyphens>/SKILL.md` with front matter `name` and `description` and a body rendered from a fixed template listing the steps (command tool name and declared purpose), plus the manifest digest. Fail with `unknown_command` for a missing command or major, `forbidden_step` for M1, S1 or A0 steps, and render W3, X1, D1 steps as "file a request for a human". Write the surface index. Acceptance: golden Skill for `fixtures/manifest/guides.json`, and the error cases. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; only registry-declared text and fixed templates (no standards text, CONSTITUTION §5).

## Open questions

- The task-guide declaration field must be added to the registry schema by `CORE.REG`; until then the emitter reads an optional field and emits nothing when it is absent.

## Children

_Leaf: none._
