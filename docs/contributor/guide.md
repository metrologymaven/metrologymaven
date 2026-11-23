---
title: Contributor guide
revision: f1aa07d0b18bf60bd3bdd04612b66262864ad6d7
---

# Contributor guide

This guide shows how to take one node of the specification tree from open to done: find it, claim it, expand it, and check the tree. It documents the repository at the revision named in the front matter above (`revision:`). Commands are run from the repository root and need Python 3 with the standard library only.

Every command in a block marked `sh run` is executed by `fid-outreach doc-run` against a fresh copy of that revision, so a command shown here works there as written. Blocks marked `sh run expect-fail` are documented error examples and must exit non-zero. To check this guide yourself, see [Checking this guide](#checking-this-guide).

## Find

The tree lives in `spec/`, one `NODE.md` per node. A node with `status: open` still needs to be split or finished as a leaf. `tools/scope.py` lists the open nodes at the shallowest level that still has any:

```sh run
python tools/scope.py open
```

To look at one level, or at the first few nodes only, pass `--level` or `--limit`:

```sh run
python tools/scope.py open --level 4 --limit 5
```

Nodes marked `needs_expert:` are left out of this list, because a person with the named domain expertise must write them (see `AGENTS.md`, rule 13, and the expert queue in `generated/EXPERT-QUEUE.md`). `generated/INDEX.md` is the same tree as a browsable table.

For counts by level and status:

```sh run
python tools/scope.py stats
```

## Claim

Before you write anything, make sure nobody else is working on the node (`AGENTS.md`, working rule 2):

1. Open the node's `NODE.md` and look at `claimed_by:` in its front matter. If it names someone, the node is taken.
2. Search the open pull requests and issues on GitHub for the node ID (for example `OUTR.DOCS.CONTRIB`).
3. Claim it: open an issue with the **Expand node** template, or comment `/claim` on the existing issue, and you are assigned automatically (`/unclaim` releases it). In your pull request, set `claimed_by: <your-handle>` in the node's front matter.

Claims older than 14 days with no pull request can be taken over (`CONTRIBUTING.md`). If you stop, release your claim.

## Expand

Follow `prompts/expand-node.md` exactly. It tells you what to read first (`AGENTS.md`, the constitution, the node, its ancestors, its siblings and the matching seed rows) and then to choose exactly one outcome:

- **Leaf:** the node is small enough for one pull request in one crate (size S or M), with concrete acceptance criteria and a named fixture. Rewrite it from `templates/LEAF.md`.
- **Split:** the node is too big; divide it into 2 to 9 children that between them satisfy every parent requirement, with no overlap.
- **Decision:** the node cannot be finished without a choice that belongs to people (licensing, naming, a phase move, a standards interpretation). Open a record under `decisions/` and set `status: decision`.

You may do this by hand or with any coding agent; `AGENTS.md` applies to both. With Claude Code, for example (not run by the checker, because it needs an agent):

```sh
claude -p "Follow prompts/expand-node.md exactly. TARGET: spec/aln/NODE.md"
```

<!-- claim-lint: allow CL001 reason="section name for running the spec tree checker; no conformance claim" -->
## Validate

Run the tree checker from `tools/scope.py`. It checks every node's front matter, sections and requirement tracing, and rewrites the reports under `generated/`:

```sh run
python tools/scope.py check
```

It exits 0 when there are no errors; warnings never fail it. Fix every error in the files you touched and leave errors in other files alone, because someone else may be working there (`prompts/expand-node.md`, step 5). Per `decisions/D-042-generated-reports-refreshed-on-main.md`, do not commit the regenerated files under `generated/` in your pull request; they are refreshed on `main` in dedicated regeneration pull requests.

An unknown subcommand is an error and exits non-zero:

```sh run expect-fail
python tools/scope.py no-such-verb
```

## House rules

The full rules are in [`AGENTS.md`](../../AGENTS.md), and they apply to people as well as agents. In short:

- Change only your target node, its new children, the `## Children` section of the target, and new files under `decisions/`. If a sibling or ancestor looks wrong, say so in your node's open questions.
- The constitution (`spec/CONSTITUTION.md`) wins over everything else in the tree.
- Each requirement holds one testable `shall`, and every child lists the parent requirements it satisfies.
- Never paste text from ISO, ASME or VDA standards; cite the designator and clause and use your own words. Mark claims you have not checked with `[unverified]`.
- Choices that belong to the maintainers go into a decision record, not into a node.
- A person is accountable for every pull request. Name the agent tool in the description if one generated content, and sign off every commit (`git commit -s`, DCO).
- Report only checks you actually ran, and say whether `python tools/scope.py check` passed.

The pull request checklist and review rules are in [`CONTRIBUTING.md`](../../CONTRIBUTING.md).

## Building from source

The Rust code lives in the same repository as the tree, under `crates/`. Build and test commands are in the Build section of the [README](../../README.md#build). A full guide to building from source (toolchain setup and supported platforms) does not exist yet; this page will link it once it does.

## Checking this guide

The guide is checked by `fid-outreach doc-run`, which needs Git and a Rust toolchain (not run by the checker itself):

```sh
cargo run -q -p fid-outreach -- doc-run --guide docs/contributor/guide.md --repo .
```

It refuses to run (error `revision_mismatch`) unless the `revision:` above is the commit checked out in `--repo`. It then copies that commit into a temporary folder, runs each `sh run` block there in order, and deletes the folder, so your working copy is never changed. Runnable lines are run without a shell, may start only `python` (with a script file from the repository), `git` or `cargo`, and see only a few environment variables such as `PATH`.
