# Release runbook

This is the release procedure of Metrology Maven, step by step, with who performs each step.
Every step is run by CI except one: **sign-approval**, performed by a person holding the
`release-approver` role (GOVERNANCE.md, D-003). The runbook names the role, not people.

## Read this first: what exists today

Most steps are **planned**: the spec defines them, but the tool that performs them is not
built yet. A step marked `available` names a command that exists in this repository today;
a step marked `planned` must not be described or performed as if it worked. In particular,
nothing in this repository signs, notarises, publishes or updates channel feeds yet:
`release-workflow.yml` (`ECO.REL.PROC.FLOW`) wires all 22 runbook jobs and simulates the
pre-approval/post-approval state machine via `cargo xtask rel flow-sim` in dry-run mode,
while live gate evaluation (`ECO.REL.PROC.GATES`), signing, notarisation and publication
remain planned.

No tool used here creates tags, GitHub releases or signatures as a side effect of checking.

## How this file is made

This file is generated. The step list lives in `runbook.toml`; this preamble lives in
`RUNBOOK.preamble.md`. After editing either, render and check:

```sh
cargo xtask rel runbook-render --dir release --out release/RUNBOOK.md
cargo xtask rel runbook-check --dir release
```

The check is offline and read-only. It fails if a step other than sign-approval is
performed by a person, if a `ci` step names a job the release workflow lacks or a workflow
job has no step, if a step names a gate that `gates.toml` lacks or a gate is used by no
step, if a step marked `available` names a tool that is not an existing `cargo xtask rel`
subcommand, or if this file differs from the rendering. The step IDs and their order, and
the set of `available` steps, are pinned by the AC-6 test in
`crates/fid-xtask/tests/rel_runbook.rs`: changing them means changing that test too.

Channels: `nightly` builds run the CI steps without approval and are signed with a separate
nightly key; `beta`, `stable` and `lts` releases stop at sign-approval.
