# Release runbook

This is the release procedure of Metrology Maven, step by step, with who performs each step.
Every step is run by CI except one: **sign-approval**, performed by a person holding the
`release-approver` role (GOVERNANCE.md, D-003). The runbook names the role, not people.

## Read this first: what exists today

Most steps are **planned**: the spec defines them, but the tool that performs them is not
built yet. A step marked `available` names a command that exists in this repository today;
a step marked `planned` must not be described or performed as if it worked. In particular,
nothing in this repository signs, notarises, publishes or updates channel feeds yet, and
the release workflow itself (ECO.REL.PROC.FLOW) does not exist: `release-workflow.yml` in
this directory is a skeleton of job IDs, never run.

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
step, or if this file differs from the rendering.

Channels: `nightly` builds run the CI steps without approval and are signed with a separate
nightly key; `beta`, `stable` and `lts` releases stop at sign-approval.
