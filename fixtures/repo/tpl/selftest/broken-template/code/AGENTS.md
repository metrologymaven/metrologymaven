# AGENTS.md: rules for coding agents in {{repo_id}}

<!-- house-rules:begin block=core source=metrologymaven@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa sha256=e08fec59e57bce5a803c60492d03cec9628a0fef6ab1f311d8a067711f971f78 -->
## House rules (core)

These rules hold in every repository of the project, for people and coding agents alike.

### Accountability

A named person answers for everything an agent opens, commits or comments. Each PR description names the agent tool that produced its content, and every commit carries a DCO sign-off.

### Check then claim

Before starting on a node or issue, search open PRs and issues for its ID and look at `claimed_by:`. Claim the work before writing, and release the claim if you stop.

### Verify before citing

Open a node, decision, seed or file before you refer to it. Treat notes in Open questions as hypotheses, not facts. Mark claims you have not verified with `[unverified]`.

### Report only what ran

State which checks and tests you actually ran and whether they passed. Never report a result for a check that did not run.

### Ask only for high-stakes actions

Ask a maintainer before licensing changes, publishing, pushing to a new remote, handling secrets, spending money or destructive git operations. For everything else, keep working.

### main is PR-only

Every change reaches `main` through a reviewed pull request. CI and tooling changes go in their own PRs, never mixed with other work.

### Never crash

All input is hostile. Parsers are bounded and fail closed. No panics and no `unsafe` outside tests, except named, reviewed exceptions; the kernel has none.

### Clean room

Work only from public specifications and observed behaviour. Never copy code, strings, icons or assets from other products. List every third-party asset in `ATTRIBUTION.md`.

### Never paste standards text

ISO, ASME and VDA texts are copyrighted. Cite designators and clause numbers only and paraphrase in your own words. Never send standards text to an external service.
- Hand-edited line that breaks the recorded SHA-256 digest.
<!-- house-rules:end block=core -->

This file applies to every coding agent working in this repository, and people should read it too.
The generated house-rules blocks are written directly under the heading above by the house-rules
sync tool. Do not edit them by hand: change the canonical source and run sync instead.

## Repository-specific notes

Add notes that apply only to this repository below this heading, outside the generated blocks.
