---
id: AGENT.SKILLS.STYLE
title: Guide and recipe content standard and linter
level: 3
parent: AGENT.SKILLS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-agent]
satisfies: [REQ-AGENT.SKILLS-005, REQ-AGENT.SKILLS-006, REQ-AGENT.SKILLS-007]
covers: []
depends_on: [AGENT.SKILLS.CATALOG]
uses_artifacts: [CORE.REG.MAN, AUTO.MCP.SKILLS, AGENT.SKILLS.RECIPE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.SKILLS.STYLE: Guide and recipe content standard and linter

## Purpose

Writes down how curated task guides and recipes must read, and enforces the machine-checkable parts in CI: no standards text, no numbers dressed as results, no instruction to perform a forbidden class directly, a proposal step at the end of every state-changing workflow, and a word budget per Skill.

## Scope

**In:**
- The content standard document (`agent/STYLE.md`) for guide and recipe prose
- The CI linter over curated guides (from the manifest), rendered Skills and recipe prose

**Out** (and which node owns it instead):
- Rejecting M1, S1, A0 steps at generation (`AUTO.MCP.SKILLS`; this linter checks prose, not step lists)
- Which workflows are curated (`AGENT.SKILLS.CATALOG`)
- Recipe format and replay (`AGENT.SKILLS.RECIPE`)

## Requirements

- **REQ-AGENT.SKILLS.STYLE-001** If the prose of a curated guide or recipe contains a numeric token with a length unit, angle unit or `%` outside a fenced command example, then the linter shall fail with `result_like_number` naming the file and line [unverified heuristic; maintainers may allow-list lines].
- **REQ-AGENT.SKILLS.STYLE-002** If the prose names an M1, S1, A0 or W3 command, or its tool name, other than in the phrase pattern for filing a request for a human, then the linter shall fail with `forbidden_instruction`.
- **REQ-AGENT.SKILLS.STYLE-003** If a curated guide's workflow contains any W1 or W2 step and its last step is not `agent.attach_brief@1` followed by `journal.request_promote_branch@1`, then the linter shall fail with `missing_proposal_step`.
- **REQ-AGENT.SKILLS.STYLE-004** If the rendered body of a curated Skill exceeds 1,500 words [unverified budget], then the linter shall fail with `over_budget` giving the word count.
- **REQ-AGENT.SKILLS.STYLE-005** If guide or recipe prose contains a run of 12 or more consecutive words matching the project's standards-phrase deny list, or a quotation longer than 25 words, then the linter shall fail with `possible_standards_text` [unverified heuristic].
- **REQ-AGENT.SKILLS.STYLE-006** The content standard shall state each rule above in plain words with one compliant and one non-compliant example authored by the project.

## Acceptance criteria

- **AC-1** Given `fixtures/agent/skills/style/clean/` (guides, Skills and recipes that follow every rule), when the linter runs, then it passes.
- **AC-2** (error path) Given `style/numbers.md` containing "the bore measures 12.004 mm" in prose, then the linter fails with `result_like_number` at that line; the same text inside a fenced command example passes.
- **AC-3** (error path) Given a guide with a W2 step and no closing brief and promotion-request steps, then it fails with `missing_proposal_step`.
- **AC-4** (error path) Given prose "now call `journal_promote_protected_branch`", then it fails with `forbidden_instruction`.
- **AC-5** (error path) Given a Skill of 1,600 words, then it fails with `over_budget` reporting 1600.

## Design

- **Registry command(s):** none; CI binary `agent-style-lint` in `fid-agent`.
- **Data structures:** `LintFinding { file, line, rule, detail }`; deny list `agent/style-denylist.txt` (project-authored phrase stems, never standards text).
- **Algorithm:** tokenise Markdown prose outside fenced blocks; regex for number plus unit tokens from the unit catalogue symbols; match command IDs and tool names against manifest classes; check guide step lists for the closing pair; count words in rendered Skill bodies (whitespace-separated tokens outside front matter).
- **Errors:** the rule codes above.
- **Determinism:** D0.
- **Performance budget:** < 5 s on the full catalog [unverified].
- **Crate:** `fid-agent`

## Test fixtures

Project-authored fixture guides, Skills and recipes under `fixtures/agent/skills/style/`. The deny list contains project-written stems only.

## Build prompt

> In crate `fid-agent`, add CI binary `agent-style-lint` and write `agent/STYLE.md`. Lint curated guides (from the manifest), rendered Skills and recipe prose for: numbers with length, angle or percent units outside fenced command examples (`result_like_number`); naming an M1, S1, A0 or W3 command or tool other than as "file a request for a human" (`forbidden_instruction`); state-changing guides not ending with `agent.attach_brief@1` then `journal.request_promote_branch@1` (`missing_proposal_step`); Skill bodies over 1,500 words (`over_budget`); deny-list phrase runs of 12+ words or quotations over 25 words (`possible_standards_text`). Report file, line and rule. Acceptance: AC-1 to AC-5 of AGENT.SKILLS.STYLE. Constraints: CONSTITUTION §5 and §8 (never store standards text; deny list is project-authored stems), no panic, deterministic output order.

## Open questions

- The numeric and standards-text rules are heuristics; false positives are expected and need an allow-list mechanism reviewed by maintainers.

## Children

_Leaf: none._
