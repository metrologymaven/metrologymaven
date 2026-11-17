# Contributing

Every unit of work in this project is a node in the spec tree, which makes it easy to find something to do. You can contribute by hand or with a coding agent; both are first-class.

## Ways to contribute (easiest first)

| # | Contribution | Where | Skill needed |
|---|---|---|---|
| 1 | **Verify a competitor feature row**: confirm it exists and fix the wording | `seeds/*.md` | Have used the software |
| 2 | **Add a competitor inventory** (CALYPSO, Verisurf, Control X, Metrolog, CAMIO, MCOSMOS, ...) | `seeds/` | Have used the software |
| 3 | **Answer an open decision** with evidence | `decisions/` | Domain knowledge |
| 4 | **Review a split**: does it overlap, leave gaps or have the wrong boundaries? | PRs labelled `split` | Metrology or software design |
| 5 | **Expand an open node** (split it or finish it as a leaf) | `spec/**/NODE.md` with `status: open` | Follow `prompts/expand-node.md` |
| 6 | **Become a domain maintainer** | `GOVERNANCE.md` | Sustained contributions |

## Claiming a node

1. Run `python tools/scope.py open` or look at `generated/INDEX.md`.
2. Open an issue with the **Expand node** template, or comment on the existing one.
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

- **Level 1–2 splits, constitution changes and `frozen` transitions:** approval from a domain maintainer (the founder until maintainers exist).
- **Level 3 and deeper:** lazy consensus. Merge after 72 h with one approving review and no objection.
- **Seeds and typos:** one approving review.
