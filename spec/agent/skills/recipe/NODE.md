---
id: AGENT.SKILLS.RECIPE
title: Worked recipes from golden journals and replay check
level: 3
parent: AGENT.SKILLS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-AGENT.SKILLS-004]
covers: []
depends_on: [CORE.RPL.EXEC, CORE.RPL.CLASS]
uses_artifacts: [FLOW.RUNNER, CORE.JRN.CANON]
side_effect: R0
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.SKILLS.RECIPE: Worked recipes from golden journals and replay check

## Purpose

A recipe shows an agent one complete, real task: the prompt a person would give, then the commands that did it, cut from a FLOW golden journal. This leaf defines the recipe file, the extraction tool that cuts it from a golden journal, and the CI check that every recipe still replays on the current manifest.

## Scope

**In:**
- Recipe file format (`agent/recipes/<recipe-id>.json`)
- Extraction from a FLOW acceptance-job package by entry range
- CI replay check

**Out** (and which node owns it instead):
- Golden journals and job packages (`FLOW.RUNNER` format, FLOW app nodes)
- Replay engine and outcome classes (`CORE.RPL.EXEC`, `CORE.RPL.CLASS`)
- Serving recipes to agents (`AGENT.SKILLS.QUERY`)
- Prose rules (`AGENT.SKILLS.STYLE`)

## Requirements

- **REQ-AGENT.SKILLS.RECIPE-001** A recipe file shall hold recipe ID, title, task prompt, source job ID, source golden-journal digest, the first and last entry hash of the excerpt, the excerpt's entries, and a short step commentary of at most 40 words per entry.
- **REQ-AGENT.SKILLS.RECIPE-002** When extraction runs on a job package and an entry range, the tool shall copy the entries unchanged and record the package's golden-journal digest and the range's first and last entry hashes.
- **REQ-AGENT.SKILLS.RECIPE-003** If a recipe's excerpt does not match the entries at that range of its source golden journal byte for byte, then the check shall fail with `excerpt_mismatch`.
- **REQ-AGENT.SKILLS.RECIPE-004** When the CI check replays a recipe's excerpt from the source journal's state at the excerpt start on the current manifest, it shall pass on `IDENTICAL` or `EQUIVALENT` and otherwise fail naming the recipe, the outcome and the first divergent entry.
- **REQ-AGENT.SKILLS.RECIPE-005** If an excerpt contains an entry by a principal of kind `agent`, a W3, S1 or M1 entry, then extraction shall refuse with `unsuitable_entry`, so recipes only show steps an agent may perform or request.

## Acceptance criteria

- **AC-1** Given fixture package `fixtures/agent/skills/recipe/job-demo/` and range entries 4 to 9, when `agent-recipe extract` runs, then the recipe file equals the golden file and records the golden-journal digest and both entry hashes.
- **AC-2** Given that recipe, when `agent-recipe check` runs on the fixture manifest, then the outcome is `IDENTICAL` and the check passes.
- **AC-3** (error path) Given the recipe with one entry edited, then the check fails with `excerpt_mismatch`.
- **AC-4** (error path) Given a fixture manifest where the recipe's fit command changed its algorithm version, then the check fails with `DIVERGENT` naming the first divergent entry.
- **AC-5** (error path) Given a range including an S1 signing entry, then extraction refuses with `unsuitable_entry`.

## Design

- **Registry command(s):** none; CI binary `agent-recipe` with subcommands `extract` and `check` in `fid-agent`.
- **Data structures:** `Recipe { id, title, prompt, source_job, golden_digest, first_entry, last_entry, entries: Vec<JournalEntry>, commentary: Vec<String> }`.
- **Algorithm:** extraction reads the job package (FLOW.RUNNER format); check reconstructs state at the excerpt start, replays the span in exact mode through `CORE.RPL.EXEC`, and classifies with `CORE.RPL.CLASS`.
- **Errors:** `excerpt_mismatch`, `unsuitable_entry`, `source_missing`, replay outcomes.
- **Determinism:** D2: replay of seeded steps under the recorded seeds; the check's verdict is reproducible for a given manifest.
- **Performance budget:** each recipe check < 30 s on reference hardware [unverified].
- **Crate:** `fid-agent`

## Test fixtures

A project-authored synthetic acceptance-job package `fixtures/agent/skills/recipe/job-demo/` with its golden journal; real recipes come from FLOW golden journals once FLOW publishes them (P1a).

## Build prompt

> In crate `fid-agent`, add CI binary `agent-recipe` with `extract <package> --from <n> --to <m> --id <recipe-id>` and `check [<recipe>...]`. A recipe file holds id, title, prompt, source job, golden-journal digest, first and last entry hash, the copied entries and short commentary (at most 40 words per entry). Extraction copies entries unchanged and refuses ranges containing agent-principal, W3, S1 or M1 entries (`unsuitable_entry`). The check verifies the excerpt byte for byte against the source journal (`excerpt_mismatch`), reconstructs state at the excerpt start and replays the span in exact mode via the CORE replay engine, passing on IDENTICAL or EQUIVALENT and otherwise reporting the outcome and first divergent entry. Acceptance: AC-1 to AC-5 of AGENT.SKILLS.RECIPE. Constraints: CONSTITUTION §8 (no panic, deny `unwrap` etc.), §4 replay outcomes, RFC 8785 canonical JSON for files.

## Open questions

- Real recipes depend on FLOW golden journals, which FLOW's app nodes publish from P1a.

## Children

_Leaf: none._
