# house-rules: canonical source of the project's house rules

This directory is the **one canonical source** of the contributor and agent rules (spec node ECO.RULES.SRC). Every other copy of these rules, such as the house-rules blocks inside a repository's `AGENTS.md`, is **generated** from here by the sync tool (ECO.RULES.SYNC) in the block format of ECO.RULES.BLOCK (`tools/house_rules/`).

**Do not copy any file in this directory into another repository by hand.** Only the generated block bodies travel to other repositories. To change a rule, edit it here in a PR and let the sync tool refresh the copies.

| File | What it holds |
|---|---|
| `manifest.toml` | Block IDs, source files, target repositories, and the size budgets (core block body 3 072 bytes, whole `AGENTS.md` 10 240 bytes) |
| `manifest.json` | The same manifest for Python < 3.11, which has no `tomllib`; keep it equal to `manifest.toml` |
| `core.md` | Rules for every repository |
| `rust.md` | Rules for repositories with Rust code |
| `python.md` | Rules for the spec repository's Python tooling |
| `restricted-paths.txt` | Path patterns (one per line, glob style) whose content must never be sent to an external service; read by ECO.CI.BOTS |
| `repos.fixture.toml` | Stand-in repository map until `repos.toml` from ECO.REPO.TOPO exists |

Sizes are measured in UTF-8 bytes after normalising line endings to LF. The test is `tools/tests/test_house_rules_src.py`.

The rules restate this repository's `AGENTS.md` and `spec/CONSTITUTION.md` §8; they add no new policy.
