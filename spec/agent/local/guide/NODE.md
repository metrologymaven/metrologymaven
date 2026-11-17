---
id: AGENT.LOCAL.GUIDE
title: Generated open-weight model guidance page
level: 3
parent: AGENT.LOCAL
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-AGENT.LOCAL-003, REQ-AGENT.LOCAL-004]
covers: []
depends_on: []
uses_artifacts: [AGENT.BOARD.BUNDLE, AGENT.SKILLS.CATALOG, AGENT.LOCAL.REF, AGENT.LOCAL.CIRUN]
side_effect: W1
determinism: D0
cli: [fid-eval local guide]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.LOCAL.GUIDE: Generated open-weight model guidance page

## Purpose

Tells an offline shop, in one generated page, which open-weight models can do which curated workflows, on what hardware, under what licence, and which workflows not to delegate. Every figure comes from the published scoreboard; nothing is typed by hand.

## Scope

**In:**
- `fid-eval local guide --board <bundle> --catalog <catalog-index.json> --hosts <board-input-dir> --out <dir>`
- Hardware from the `host.json` files of `AGENT.LOCAL.CIRUN` and licences from model cards (`AGENT.LOCAL.REF` format)

**Out** (and which node owns it instead):
- Scores (`AGENT.BOARD.BUNDLE`)
- Hosting the page (`OUTR`)

## Requirements

- **REQ-AGENT.LOCAL.GUIDE-001** For each open-weight model on the board, the page shall list its success rate per curated workflow, the hardware it ran on (CPU model, RAM, GPU model and memory) and its licence as declared by the publisher.
- **REQ-AGENT.LOCAL.GUIDE-002** For each such model, the page shall list as "not recommended for delegation" every curated workflow whose success rate is below 80 % [unverified threshold] or whose tasks show any committed safety violation.
- **REQ-AGENT.LOCAL.GUIDE-003** Every score on the page shall be copied from `board.json` of a named release, and the page shall name that release and its red-team verdict.
- **REQ-AGENT.LOCAL.GUIDE-006** If a model's results have no `host.json`, then its hardware shall be shown as `not reported`.
- **REQ-AGENT.LOCAL.GUIDE-004** A workflow with no task results for a model shall be listed as `not evaluated`, never as recommended.
- **REQ-AGENT.LOCAL.GUIDE-005** Generating twice from the same inputs shall give byte-identical output.

## Acceptance criteria

- **AC-1** Given `fixtures/agent/local/board/` (two open-weight models) and a catalog index, when `fid-eval local guide` runs, then the page equals the golden file.
- **AC-2** Given a model with 0.75 success on workflow W, then W is listed as not recommended for that model.
- **AC-3** Given a model with no runs on workflow V, then V is `not evaluated`.
- **AC-4** Given AC-1 twice, then outputs are byte-identical.

## Design

- **Registry command(s):** none (D-032). Writes into a local output folder.
- **Data structures:** `GuideRow { model, workflow, success: Fraction|NotEvaluated, recommended: bool, hardware, licence }`.
- **Algorithm:** map tasks to workflows via the task `workflows` field and the catalog index; aggregate per workflow as exact fractions; compare with the threshold exactly; render static Markdown and HTML from fixed templates.
- **Errors:** `board_missing`, `catalog_mismatch`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-eval-board`

## Test fixtures

Project-authored board bundle and catalog index under `fixtures/agent/local/`.

## Build prompt

> In crate `fid-eval-board` (Tooling), implement `fid-eval local guide --board <bundle> --catalog <catalog-index.json> --hosts <board-input-dir> --out <dir>`. For each open-weight model on the board, output per curated workflow the success rate (exact fraction, from `board.json` only), hardware (CPU, RAM, GPU and memory from `host.json` in the board input, or `not reported`) and publisher-declared licence; mark workflows below 80 % or with any committed violation as not recommended for delegation, and workflows without results as `not evaluated`. Name the board release and its red-team verdict. Render static Markdown and HTML, byte-identical for identical inputs. Acceptance: AC-1 to AC-4 of AGENT.LOCAL.GUIDE. Constraints: CONSTITUTION C7 and §8 (honest, generated status).

## Open questions

- The 80 % threshold is a placeholder for maintainers to confirm.

## Children

_Leaf: none._
