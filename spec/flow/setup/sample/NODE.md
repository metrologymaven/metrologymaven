---
id: FLOW.SETUP.SAMPLE
title: Sample job on public data (wizard step 8)
level: 3
parent: FLOW.SETUP
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.SETUP-008]
covers: [JOB.USE.07]
depends_on: [FLOW.SETUP.WIZARD, CORE.RPL.RETGT, CORE.PRJ.TPL, RPT.OUTPUT.PDF, ECO.DATA.FETCH]
uses_artifacts: [ECO.DATA.EX, ECO.DATA.CAT]
side_effect: W2
determinism: D0
cli: [fid setup run-sample-job]
mcp: [setup_run_sample_job]
journal: [setup.run_sample_job@1]
updated: 2026-11-17
---

# FLOW.SETUP.SAMPLE: Sample job on public data (wizard step 8)

## Purpose

Gives a new user a finished result in minutes: wizard step 8 creates a sample project from a shipped example on public or synthetic data, re-runs its recorded journal under the shop's new settings, and exports a report into the user's approved local export root, all offline once the data is present.

## Scope

**In:**
- `setup.run_sample_job@1` (W2): choose the example for the user's primary app, create the project, re-targeted replay, export report
- Offline operation once the example data is present (shipped or fetched by digest)
- Marking wizard step 8 done

**Out:**
- Example projects and their data (owned by `ECO.DATA.EX`, `ECO.DATA.CAT`)
- Re-targeted replay (owned by `CORE.RPL.RETGT`) and PDF export (owned by `RPT.OUTPUT.PDF`)
- The tutorial that follows (owned by `FLOW.SETUP.ONBOARD`)

## Requirements

- **REQ-FLOW.SETUP.SAMPLE-001** When `setup.run_sample_job@1` runs with an `app` argument, the system shall select the example project registered for that app, verify its data digests, create a new project from it and replay its journal in re-targeted mode under the current settings.
- **REQ-FLOW.SETUP.SAMPLE-002** The sample job shall export a PDF report to an approved local export root chosen by root handle, never by raw path.
- **REQ-FLOW.SETUP.SAMPLE-003** If the example data is present locally, then the sample job shall complete with all network interfaces disabled.
- **REQ-FLOW.SETUP.SAMPLE-004** If the example data is absent and the network is unavailable, then the command shall fail with `setup.sample_data_missing` naming the dataset ID and leave wizard step 8 pending.
- **REQ-FLOW.SETUP.SAMPLE-005** If the replay outcome is `DIVERGENT` because the user's decision-rule settings differ from the example's, then the command shall still complete, mark the affected results, and show their suggested fixes, rather than fail.

## Acceptance criteria

- **AC-1** Given the Report example dataset present and networking disabled, when `fid setup run-sample-job --app report --export-root <handle>` runs as a human, then a project is created, a PDF exists in the root and step 8 is done.
- **AC-2** (error path) Given no example data and no network, then the command fails with `setup.sample_data_missing` and step 8 stays pending.
- **AC-3** Given step 4 acknowledged-skip, when the sample job runs on an ASME example, then it completes with NOT_DECIDABLE results carrying `rule_missing` fixes.
- **AC-4** (replay) Given the journal of AC-1, when replayed in exact mode, then `IDENTICAL`.

## Design

- **Registry command(s):** `setup.run_sample_job@1`, W2, human_only; params `app: AppId`, `export_root: RootHandle`. Result: project ID, report export digest, replay outcome.
- **Algorithm:** look up the example by app in the example registry; verify digests via the dataset fetch library (fetch only if missing and network allowed); create project from the example template; re-targeted replay; `report.export_pdf@1` to the root; mark step 8.
- **Errors:** `setup.sample_data_missing`, `setup.sample_unknown_app`, `setup.step_order`.
- **Determinism:** D0 for the replayed results given the same settings.
- **Performance budget:** under 2 minutes on the minimum-profile machine [unverified].
- **Crate:** `fid-setup`.

## Test fixtures

The P1a Report example from `ECO.DATA.EX` (public or synthetic); a tiny synthetic example for unit tests.

## Build prompt

> In crate `fid-setup`, add `setup.run_sample_job@1` (W2, human_only; CLI `fid setup run-sample-job --app <id> --export-root <handle>`; MCP `setup_run_sample_job` as a request-only tool, generated from `human_only`). Select the example registered for the app, verify data digests (fetch only if missing and network allowed, else `setup.sample_data_missing`), create a project from it, replay re-targeted under current settings, export PDF via report.export_pdf@1 to the root handle, and mark wizard step 8 done. A DIVERGENT replay caused by settings differences completes with marked results and their fixes. Tests AC-1..AC-4 of spec node FLOW.SETUP.SAMPLE; run AC-1 with networking disabled. No raw paths, no panic, no `unwrap`.

## Open questions

- None.

## Children

_Leaf: none._
