# Expand one node (the recursive step)

You are expanding **one** node of a specification tree for an open-source dimensional metrology platform. You write specifications, **not code**. The target node path is given to you as `TARGET`.

## 1. Read, in this order

1. `AGENTS.md` and `spec/CONSTITUTION.md`.
2. The target `NODE.md`.
3. Every ancestor `NODE.md`, walking up the folders to `spec/NODE.md`. Read their Purpose, Scope, Requirements and Interfaces. These are your contracts.
4. Every sibling's frontmatter and `## Purpose` and `## Scope` only. This tells you the boundaries. Don't take over their scope.
5. The seed rows matching the target's `covers:` patterns, in `seeds/*.md`. They tell you what incumbents offer.
6. Optionally, the background concept docs in `../docs/` if present. They are informative only. The constitution and the ancestors are binding.

## 2. Decide exactly one outcome

**A. Leaf.** Choose this if ALL of the following hold:
- an implementing agent could build it in one PR (size S or M; see AGENTS.md);
- it lives in one crate;
- its acceptance criteria can be concrete, with a named fixture;
- no open decision blocks it;
- for a user-facing `kind: feature`, its CLI command, MCP tool and journal event can be named now.

**B. Split.** Choose this if it is too big for a leaf and you can divide it cleanly.

**C. Decision.** Choose this if you cannot split or finish it without a choice that belongs to people: licensing, naming, a phase move, dropping a competitor feature, a constitution conflict, or a standards interpretation that needs a domain expert.

When torn between A and B, choose B. When torn between B and C, choose C only if any split you could make would be guessing.

## 3A. If leaf

- Rewrite the node using `templates/LEAF.md`: `status: leaf`, `size: S|M`, `kind: feature` (user-facing) or `task` (internal).
- Fill in Requirements, Acceptance criteria, Design, Test fixtures and Build prompt.
- The **Build prompt** must be self-contained. It names the crate, the command signature, the requirements, the acceptance criteria and the constitution constraints that apply.
- Name `side_effect:` and `determinism:` in the frontmatter.

## 3B. If split

1. Make **2–9 children** (3–7 is typical). Each is a folder `<lowercase-last-segment>/NODE.md` under the target, made from `templates/NODE.md`.
2. **IDs:** `<TARGET.ID>.<SEGMENT>`. A segment is 2–8 uppercase letters or digits and starts with a letter. It must not be a reserved Windows device name (CON, PRN, AUX, NUL, COM1–9, LPT1–9), because the folder could not be checked out on Windows.
3. `level` = target level + 1. `kind` by level: 2 subsystem, 3 module, 4 feature, 5+ task. `status: open` (or `leaf` if a child is already trivially a leaf; then fill it as in 3A).
4. **Mutually exclusive, collectively exhaustive (MECE):**
   - every target requirement must appear in at least one child's `satisfies:`;
   - every seed ID the target covers must be covered by at least one child;
   - no two children may own the same responsibility. If two must share something, put it in one child and make the other `depends_on:` it.
5. Each child gets refined requirements: more specific than the parent, testable, one `shall` each, IDs `REQ-<child id>-NNN`.
6. Fill in each child's Scope: In and Out. Out names the sibling that owns each excluded item.
7. Fill in each child's Interfaces. Children inherit the target's CLI/MCP/journal patterns and narrow them.
8. Update the **target only**:
   - set `status: split`;
   - replace `## Children` with a table: `| ID | Title | Satisfies | Covers |`;
   - leave its requirements as they are.
9. At level 1 or 2, add a short `## Split rationale` section to the target: the dividing principle and the alternatives considered.

## 3C. If decision

- Create `decisions/D-NNN-<slug>.md` from `templates/DECISION.md`. Use the next free number; check `decisions/` first.
- Set the target's `status: decision`. Add `decision: D-NNN` to its frontmatter, and explain the block in `## Open questions`.

## 4. Quality bar for requirements

- Use EARS-style patterns:
  - ubiquitous: "The X shall ...";
  - event: "When ..., the X shall ...";
  - state: "While ..., the X shall ...";
  - unwanted: "If ..., then the X shall ...".
- Make them measurable: name tolerances, sizes and limits. Mark unmeasured targets `[unverified]`.
- No implementation choices above the leaf level unless the constitution mandates them.
- Cite standards by designator, edition and clause only. Never paste standards text.

## 5. Finish

1. Set `updated:` to today on every file you wrote.
2. Run `python tools/scope.py check`. Fix every error that involves your files. Ignore errors in files you didn't touch; other agents may be working there.
3. Write nothing outside: the target, its new subfolders, and `decisions/`.
4. Reply with one line: `TARGET <id>: leaf | split into N (<ids>) | decision D-NNN`, followed by any warnings you couldn't resolve.
