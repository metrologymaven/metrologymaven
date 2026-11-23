# Contributing

Every unit of work in this project is a node in the spec tree, which makes it easy to find something to do. You can contribute by hand or with a coding agent; both are first-class.

## Ways to contribute (easiest first)

| # | Contribution | Where | Skill needed |
|---|---|---|---|
| 1 | **Build a ready Rust leaf** | `generated/milestones/launch-1.md`, `crates/` | Follow the leaf's Build prompt |
| 2 | **Expand an open node** (split it or finish it as a leaf) | `spec/**/NODE.md` with `status: open` | Follow `prompts/expand-node.md` |
| 3 | **Add clean-room mathematical fixtures or refine workflow rows** | `fixtures/`, `seeds/*.md` | Metrology or software engineering |
| 4 | **Review a split or implementation PR** | Open PRs | Independent agent or human reviewer |
| 5 | **Optionally sign off provisional GD&T interpretation rules (`D-045`)** | `spec/gdt/`, `crates/fid-gdt` | Optional; never blocks development |

## Autonomous build mode (D-052)

Under [D-052](decisions/D-052-autonomous-zero-human-bottlenecks.md), no spec node or build step waits on external expert recruitment or legal outreach. Agents expand and implement every domain (including GD&T, uncertainty, and CNC) using clean-room mathematics, standard clause citations (never pasted standards text), and independent arbitrary-precision oracles. Any unreviewed interpretation rule stays visibly labelled `interpretation: provisional` (`D-045`).

## Claiming a node

1. Run `python tools/scope.py open` or inspect `generated/milestones/launch-1.md`.
2. Check open pull requests to avoid duplicating work.
3. Open a pull request from a feature branch (do not commit regenerated `generated/` files; see `D-042`).

## Expanding or building with an agent

```bash
claude -p "Follow prompts/expand-node.md exactly. TARGET: spec/aln/NODE.md"
```

Any agent works; `AGENTS.md` is tool-neutral. To expand a whole level, use `tools/grow.ps1`.

## PR checklist

- [ ] `python tools/scope.py check` passes (`generated/` not committed per `D-042`)
- [ ] For code PRs: `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check` pass
- [ ] Requirements are testable, with one `shall` each
- [ ] No standards text is pasted; unverified claims are marked `[unverified]`
- [ ] Commits are signed off (`git commit -s`, DCO)

## Review rules (D-037 P2 as amended by D-052)

- **Spec expansions, architecture decisions, and implementation PRs:** reviewed by an **independent agent reviewer** (a different model or a fresh session without the author's context, running `scope.py check`) plus required CI gates. An agent never reviews its own work.
- **Provenance (D-036):** if a PR contains agent-generated content, its description names the agent tool. Only contribute material you have the right to license under `D-001`.
