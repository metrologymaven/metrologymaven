---
title: Contributor guide
revision: f7885bf978b93bdb31379e0bacf37026f5cb44c4
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

Under `D-052`, autonomous build mode is active (`needs_expert:` is retired, and all open public nodes enter `scope.py open`). `generated/INDEX.md` is the tree as a browsable table, and `generated/milestones/launch-1.md` tracks the launch waves.

For counts by level and status:

```sh run
python tools/scope.py stats
```

## Claim

Before you write anything, make sure nobody else is working on the node (`AGENTS.md`, working rule 2):

1. Open the node's `NODE.md` and look at `claimed_by:` in its front matter. If it names someone, the node is taken.
2. Search the open pull requests and issues on GitHub for the node ID (for example `OUTR.DOCS.CONTRIB`).
3. Claim it in your pull request (or comment `/claim` on an `expand-node` issue to be assigned automatically, `/unclaim` to release), and set `claimed_by: <your-handle>` if you leave a claim open. Release your claim if you stop.

## Expand

Follow `prompts/expand-node.md` exactly. It tells you what to read first (`AGENTS.md`, the constitution, the node, its ancestors, its siblings and the matching seed rows) and then to choose exactly one outcome:

- **Leaf:** the node is small enough for one pull request in one crate (size S or M), with concrete acceptance criteria and a named fixture. Rewrite it from `templates/LEAF.md`.
- **Split:** the node is too big; divide it into 2 to 9 children that between them satisfy every parent requirement, with no overlap.
- **Decision:** only external account, credential, DNS, spending, or licensing actions require a maintainer (`D-052`); architecture, specification, and clean-room standards interpretation proceed autonomously (`D-045` provisional labels apply to unreviewed GD&T rules). If an exceptional maintainer choice is required, open a record under `decisions/` and set `status: decision`.

Any coding agent or manual edit works; `AGENTS.md` is tool-neutral. For example, invoking a CLI coding agent (not run by the checker, because it needs an agent):

```sh
<agent-cli> "Follow prompts/expand-node.md exactly. TARGET: spec/aln/NODE.md"
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
- Under `D-052`, spec expansions, architectural decisions, and implementation PRs are reviewed by an independent agent reviewer (`AGENTS.md` rule 8) plus required CI gates.
- A person is accountable for every pull request. Name the agent tool in the description if one generated content, and sign off every commit (`git commit -s`, DCO).
- Report only checks you actually ran, and say whether `python tools/scope.py check` passed.

The pull request checklist and review rules are in [`CONTRIBUTING.md`](../../CONTRIBUTING.md).

## Building from source

The Rust workspace lives under `crates/` and `vendor/fid-clarabel/crate` (MSRV `1.89`, edition `2021`).

### Prerequisites

- **All platforms:** Rust `>= 1.89` with `clippy` and `rustfmt`, Git, and Python `3.10+` (`jsonschema` is needed for fixture schema checks; `tools/scope.py` and `site/build.py` use the Python standard library only).
- **C toolchain (`D-043`):** `fid-store` compiles bundled SQLite (`libsqlite3-sys`) via `cc-rs`. Install Visual Studio Build Tools (MSVC C++) and the Windows SDK on Windows, `gcc`/`cc` + libc headers on Linux, or Xcode Command Line Tools on macOS. Pure-Rust crates without `libsqlite3-sys` (`fid-verify`, `fid-mcp`, `fid-pkg`, `fid-platform`, `fid-outreach`, `fid-scan-cmp`) build without a C compiler or binutils, while `fid-cli` links `windows-sys` (via `fid-ctrl-acl`), which uses MSVC `windows-targets` (recommended) or requires MinGW `dlltool` on `-gnu` targets.
- **Optional tools:** `wasm32-wasip1` target and `wasmtime` (for `/guide/` WASM labs and kernel WASM checks), `cargo-nextest`, `cargo-deny`, and Node.js (for canonical journal fixture checks).

### Commands

```sh
cargo build --locked
cargo test --locked --workspace --exclude fid-clarabel
cargo nextest run --workspace --no-fail-fast --no-tests=pass
cargo test --workspace --exclude fid-clarabel --doc
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask fmt --check
cargo xtask layers
cargo xtask kernel-allow
cargo xtask lint-crate-graph
python site/build.py
```

On Windows, prefer `cargo xtask fmt --check` over `cargo fmt --all --check` so crate formatting batches stay under the 32 KiB `CreateProcess` command-line limit.

## Checking this guide

The guide is checked by `fid-outreach doc-run`, which needs Git and a Rust toolchain (not run by the checker itself):

```sh
cargo run -q -p fid-outreach -- doc-run --guide docs/contributor/guide.md --repo .
```

It refuses to run (error `revision_mismatch`) unless `revision:` matches the commit checked out in `--repo`. Because a commit cannot embed its own SHA-1, CI and `fid-outreach` tests substitute `HEAD` into a temporary copy of this file before running `doc-run`. `doc-run` then checks out that commit into a temporary folder, runs each `sh run` block there in order, and deletes the folder so your working copy is never changed. Runnable lines execute without a shell, may start only `python` (with a repository-relative `.py` path), `git` or `cargo`, and see only a restricted environment allowlist.
