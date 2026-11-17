# AGENTS.md: rules for coding agents

This file applies to every coding agent working in this repository (Claude Code, Codex, Cursor, Aider, and others). People should read it too.

## What this repository is

A **specification tree** for an open-source dimensional metrology platform. **Do not write implementation code here.** The output is requirements, interfaces, design notes, acceptance criteria and build prompts. Implementation happens later, in a separate repository, from the leaf build prompts.

## The one job

You are usually given **one target node** (`spec/.../NODE.md`). Follow [prompts/expand-node.md](prompts/expand-node.md) exactly. In short: read the context, then do exactly one of these:

1. **Split** the node into 2–9 children (3–7 is typical);
2. **Finish** it as a leaf; or
3. **Block** it on a decision.

Then run `python tools/scope.py check` and fix any error in files you touched.

## Hard rules

1. **Stay in your lane.** Write only:
   - the target node;
   - its new child folders;
   - the `## Children` section of the target;
   - new files under `decisions/`.

   Never edit sibling, ancestor or unrelated nodes. If a sibling or ancestor is wrong, write the problem in the target's `## Open questions` or open a decision.
2. **The constitution wins.** [spec/CONSTITUTION.md](spec/CONSTITUTION.md) defines naming, side-effect classes, determinism, units and the data model. Never contradict it. If it blocks you, open a decision.
3. **Requirements are testable.** One `shall` per requirement, in EARS style ("When <trigger>, the system shall <response>"). No implementation detail above the leaf level unless the constitution mandates it.
4. **Trace everything.**
   - Every child lists the parent requirements it satisfies (`satisfies:`), and every parent requirement must be satisfied by at least one child.
   - Every competitor feature the parent covers (`covers:`) must be covered by at least one child.
5. **Never paste standards text.** ISO, ASME and VDA text is copyrighted. Cite designators and clause numbers only, and paraphrase in your own words. Don't send standards text to any external service.
6. **Mark uncertainty.** Claims about competitor behaviour, standards details or performance numbers you haven't verified get a trailing `[unverified]`.
7. **No invented decisions.** If a choice belongs to the maintainers (licensing, naming, phase moves, dropping a competitor feature), open `decisions/D-NNN-*.md` and set the node to `status: decision`.
8. **Agents never adopt their own proposals.** Splits at levels 1 and 2 and every `frozen` transition need human review in a PR.
9. **Keep files small and regular.** Use the templates in [templates/](templates/). Frontmatter lists are single-line `[a, b, c]`, and list items must not contain commas.

## Working rules (adapted from the Craft family's house rules)

1. **A person is accountable** for everything an agent opens or comments. Agents don't add attribution to PRs.
2. **Check, then claim.** Before expanding a node, search open PRs and issues for its ID, and check `claimed_by:`. Claim it before writing, and release the claim if you stop.
3. **Verify before you cite.** Open a node, seed or decision before referring to it. Treat notes in Open questions as hypotheses.
4. **Report only what ran.** Say whether `scope.py check` passed. Never claim a check you didn't run.
5. **Ask only for high-stakes actions:** licensing, publishing, pushing to a new remote, secrets, spending money, destructive git. Otherwise keep working.
6. **`main` is PR-only.** CI and tooling changes go in their own PRs, never mixed with spec expansions.
7. **Keep this file under 10 KB.** Agents may not follow links, so put essential rules here.

## Dependencies (CONSTITUTION §2.1, D-034)

- Depend on specific nodes (level 2 or 3), not whole domains, and only on the same or lower layers.
- If you only read a file or format another node publishes (a test suite, manifest, report, token file), use `uses_artifacts: [NODE.ID]` instead of `depends_on`. Never leave a dependency as a prose "interface note".
- Mark adoption commands and manual readings `human_only: true` in their Design (D-031).

## IDs and paths

- Node ID: dotted uppercase segments, e.g. `ALN.RPS.ITER`. Folder: `spec/aln/rps/iter/NODE.md` (lowercase last segment).
- Requirement ID: `REQ-<node id>-NNN`, e.g. `REQ-ALN.RPS-003`.
- Registry command, MCP tool and journal names follow [CONSTITUTION §3](spec/CONSTITUTION.md#3-commands-cli-mcp-and-journal-names).

## Sizes

| Size | Meaning (implementation effort, for a leaf) |
|---|---|
| S | Up to ~200 lines of code plus tests, less than half a day of agent work |
| M | Up to ~600 lines of code plus tests, one PR, about a day |
| L / XL / XXL | Too big for a leaf: split |

## Commit messages

`spec(<NODE.ID>): split into N children`, `spec(<NODE.ID>): leaf`, `spec(<NODE.ID>): blocked on D-NNN`, `seeds(<prefix>): ...`, `tools: ...`.
