# Contributing

Every unit of work in this project is a node in the spec tree, which makes it easy to find something to do. You can contribute by hand or with a coding agent; both are first-class.

## Ways to contribute (easiest first)

| # | Contribution | Where | Skill needed |
|---|---|---|---|
| 1 | **Verify a workflow coverage row**: confirm the workflow exists and fix the wording | `seeds/*.md` | Practical metrology or shop-floor experience |
| 2 | **Contribute non-proprietary benchmark parts or workflow inventories** | `seeds/`, `fixtures/` | Machine-shop, lab or metrology experience |
| 3 | **Answer an open decision** with evidence | `decisions/` | Domain knowledge |
| 4 | **Review a split**: does it overlap, leave gaps or have the wrong boundaries? | PRs labelled `split` | Metrology or software design |
| 5 | **Expand an open node** (split it or finish it as a leaf) | `spec/**/NODE.md` with `status: open` | Follow `prompts/expand-node.md` |
| 6 | **Become a domain maintainer** | `GOVERNANCE.md` | Sustained contributions |

## Expert contributions

Some nodes must be written or reviewed by a person with real domain expertise: GD&T interpretation, measurement uncertainty, CMM control. They carry `needs_expert: <area>` in their frontmatter, agents skip them, and each one has a GitHub issue labelled `expert-queue` and `expert:<area>` (kept in sync automatically from [generated/EXPERT-QUEUE.md](generated/EXPERT-QUEUE.md)).

1. Browse issues labelled [`expert-queue`](../../issues?q=is%3Aissue+is%3Aopen+label%3Aexpert-queue), or open an **Offer expertise** issue and we'll route work to you.
2. Comment `/claim` to be assigned (`/unclaim` to release).
3. Either expand the node yourself, or comment your review in your own words and an agent drafts the PR for you to approve.

Experts are credited in the node's `owners` field, in `CONTRIBUTORS.md` and in release notes.

## Claiming a node

1. Run `python tools/scope.py open` or look at `generated/INDEX.md`.
2. Open an issue with the **Expand node** template, or comment `/claim` on the existing one (you're assigned automatically).
3. Set `claimed_by: <your-handle>` in the node's frontmatter in your PR. Claims older than 14 days with no PR can be taken over.

## Expanding with an agent

```bash
claude -p "Follow prompts/expand-node.md exactly. TARGET: spec/aln/NODE.md"
```

Any agent works; `AGENTS.md` is tool-neutral. To expand a whole level, use `tools/grow.ps1`.

## PR checklist

- [ ] `python tools/scope.py check` passes
- [ ] I only changed the target node, its new children and `decisions/` (see AGENTS.md rule 1)
- [ ] Requirements are testable, with one `shall` each
- [ ] No standards text is pasted; unverified claims are marked `[unverified]`
- [ ] Splits at levels 1–2: a maintainer of that domain is requested as reviewer
- [ ] Commits are signed off (`git commit -s`, DCO)

## Review rules

- **Level 1–2 splits, constitution changes, decision records and `frozen` transitions:** approval from a domain maintainer (the founder until maintainers exist).
- **Level 3 and deeper:** one approving review from a human **or an independent agent reviewer** (a different model or a fresh session without the author's context, per D-037 P2), then merge after 72 h with no objection.
- **Provenance (D-036):** if a PR contains agent-generated content, its description names the agent tool. Only contribute material you have the right to license under D-001.
- **Seeds and typos:** one approving review.
