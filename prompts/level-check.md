# Level check (run after a level has been expanded)

You are reviewing **all nodes at one level** of the spec tree after several agents have expanded them in parallel. You are given `LEVEL`. Your job is to make the level consistent **across sibling groups**, which no single expansion could see.

## Read

- `AGENTS.md` and `spec/CONSTITUTION.md`.
- `python tools/scope.py check` output and `generated/INDEX.md`.
- Every `NODE.md` at `LEVEL`: their frontmatter, Purpose, Scope and Interfaces.

## Check

1. **Overlap across groups.** Do two nodes under different parents own the same responsibility? (Example: point-cloud normal estimation under both GEO and SCAN.)
2. **Gaps.** Is anything a parent's Scope says is In not owned by any child?
3. **Interfaces.**
   - Every `depends_on:` points at a node that exists and that actually offers what is needed.
   - The provider's Interfaces section lists what consumers expect.
4. **Names.**
   - No two nodes propose the same CLI command, MCP tool or journal event.
   - Names follow CONSTITUTION §3.
5. **Sizes.** No node is marked a leaf while being obviously larger than M.
6. **Constitution.** No requirement contradicts it (e.g. a GPU-computed value of record, or motion over MCP).

## Fix

- You may edit the nodes **at this level** to fix overlap, gaps, interfaces, names and sizes:
  - move scope items;
  - adjust `depends_on`;
  - rename commands;
  - add `[unverified]` marks.
- Don't delete nodes, change parents, or change any requirement's meaning. If that's needed, open a decision record.
- Write a review log to `generated/reviews/LEVEL-<LEVEL>.md` that lists each finding and what you did (fixed, or decision D-NNN).

## Finish

Run `python tools/scope.py check`, fix the errors you introduced, and reply with a one-paragraph summary.
