---
id: ECO.PARITY.STATUS
title: Per-app status and known-gaps report
level: 3
parent: ECO.PARITY
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.PARITY-005]
covers: []
depends_on: [ECO.PARITY.TRACE, ECO.PARITY.GEN]
uses_artifacts: [ECO.APPS.MANIFEST.EXPORT, ECO.APPS.LIST]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.PARITY.STATUS: Per-app status and known-gaps report

## Purpose

Generates `status.json` and `status.md`: for each app in the app list, its declared status and status scope, its live/total parity over the seed rows in that app's scope, every in-scope row that is not live, and the known gaps declared in its manifest, plus consistency findings where the declaration and reality disagree. OUTR renders it on the website and docs portal; this leaf only produces it (REQ-ECO-010). It reads the trace (what is live) and `apps-status.json` (what each app declares and enables), both as files.

## Scope

**In:**
- Deciding which seed rows are in an app's scope: by namespace of the commands named by covering leaves, with a seed-prefix fallback for rows that have no covering leaf
- Joining declared status, status scope and known gaps from `apps-status.json`
- Apps listed but without a manifest, shown as `planned`
- Consistency findings and the `--strict` mode used as a release gate

**Out** (and which node owns it instead):
- The live rule (owned by `ECO.PARITY.TRACE`)
- Header format and rendering conventions (shared with `ECO.PARITY.GEN`, whose writer this leaf reuses)
- The drift check (owned by `ECO.PARITY.DRIFT`, which also covers `status.*`)
- Manifest content (owned by the per-app nodes under `ECO.APPS`)
- Rendering on the website (owned by `OUTR`)

## Requirements

- **REQ-ECO.PARITY.STATUS-001** The status report shall list each app from `apps-status.json` with its declared `status` and `status_scope`, or status `planned` if it has no manifest.
- **REQ-ECO.PARITY.STATUS-002** A seed row shall be in an app's scope when any command ID listed for it in the trace has a namespace (the part before the first dot) among the app's enabled namespaces, or, when the trace lists no command for the row, when the row's seed prefix maps to the app in `status-scope.toml`.
- **REQ-ECO.PARITY.STATUS-003** For each app with a manifest, the report shall give live and in-scope counts and the floor percentage, and list every in-scope row that is not live with its trace reason code.
- **REQ-ECO.PARITY.STATUS-004** The report shall list as consistency findings: `GAP-LIVE` (a declared seed gap that is live), `GAP-UNDECLARED` (an in-scope row not live and not declared as a gap), `GAP-OUT-OF-SCOPE` (a declared seed gap outside the app's scope) and `STABLE-WITH-GAPS` (status `stable` while any in-scope row is not live, contrary to D-017).
- **REQ-ECO.PARITY.STATUS-005** The report shall use the same generated header as the parity report, naming the trace digest and the `apps-status.json` and `status-scope.toml` digests, and shall be byte-identical for the same inputs.
- **REQ-ECO.PARITY.STATUS-006** When run with `--strict`, the generator shall exit 1 if any consistency finding exists, after writing the report.

## Acceptance criteria

- **AC-1** Given `fixtures/parity/trace.json` (from ECO.PARITY.TRACE AC-1: X.A.01 live via `a.run_one@1`, X.A.02 `AC-FAIL` via `a.run_two@1`, X.B.01 `NO-LEAF`, X.B.02 out of scope), `fixtures/status/apps-status.json` (app `alpha` with manifest, namespaces `[a]`, status `beta`, scope "subset", known gap `X.A.02`; app `beta` without manifest) and `fixtures/status/status-scope.toml` (prefix `X.B` → `alpha`), when `cargo xtask parity status --trace ... --apps-status ... --scope-map ... --out <tmp>` runs, then `alpha` shows live 1, in scope 3, 33 %, not-live rows X.A.02 (`AC-FAIL`) and X.B.01 (`NO-LEAF`), one finding `GAP-UNDECLARED X.B.01`, and `beta` shows `planned`.
- **AC-2** `status.md` matches `fixtures/status/status.golden.md` byte for byte, and both files start with the generated header including the three input digests.
- **AC-3** Given an `apps-status.json` declaring gap `X.A.01` for `alpha`, then the finding `GAP-LIVE X.A.01` appears; given status `stable`, then `STABLE-WITH-GAPS alpha` appears.
- **AC-4** (strict) With `--strict` on AC-1, the report is written and the exit code is 1; without a finding, the exit code is 0.
- **AC-5** (error path) Given an `apps-status.json` with an unknown schema version, then exit 1 with `STATUS-INPUT` and nothing written.
- **AC-6** (determinism) AC-1 run on Windows, Linux and macOS gives identical bytes.

## Design

- **Registry command(s):** none; repository tool `cargo xtask parity status --trace <json> --apps-status <json> --scope-map <toml> --out <dir> [--release <tag>] [--strict]`. `side_effect: W1`: writes reports into a local output root.
- **Data structures:** `status-scope.toml`: `[prefix_map] "GOM" = ["inspect"], "PW" = ["inspect"], "PCD" = ["probe"], "SA" = ["network"]` (initial proposal, matched on the longest dotted prefix); `status.json`: `{header, apps: [{id, status, status_scope, live, in_scope, pct, not_live: [{id, reason}], known_gaps: [...], findings: [{code, seed}]}]}` sorted.
- **Algorithm:** for each seed in the trace (excluding out-of-scope rows), compute the app set by namespace or prefix fallback; aggregate per app with BTreeMaps; compare with declared gaps; render JSON and Markdown with the GEN writer; with `--release`, write into `<out>/<tag>/`.
- **Errors:** `STATUS-INPUT`, `STATUS-IO`.
- **Determinism:** D0; integer percentages (floor).
- **Performance budget:** under 1 s for 5 000 seed rows and 10 apps [unverified until measured].
- **Crate:** `fid-xtask` (module `parity`).

## Test fixtures

`fixtures/parity/trace.json` shared with ECO.PARITY.TRACE and GEN; `fixtures/status/` apps-status, scope map and golden Markdown written for this leaf (MIT OR Apache-2.0).

## Build prompt

> In `fid-xtask` (implementation repository), add `parity status --trace <json> --apps-status <json> --scope-map status-scope.toml --out <dir> [--release <tag>] [--strict]`. Inputs: the trace from `parity trace` (`seeds[] {id, state, reason?, commands[]}`), `apps-status.json` v1 from ECO.APPS.MANIFEST.EXPORT (`apps[] {id, manifest, status, status_scope, known_gaps, namespaces}`), and a prefix map for rows with no commands. Per app: `planned` if no manifest; else scope = rows whose commands' namespaces (prefix before the first dot) are enabled, or, for rows without commands, whose seed prefix maps to the app (longest dotted prefix); out-of-scope rows never count. Output live, in-scope, floor percentage, not-live rows with reason, declared gaps, and findings `GAP-LIVE`, `GAP-UNDECLARED`, `GAP-OUT-OF-SCOPE`, `STABLE-WITH-GAPS`. Write `status.json` and `status.md` with the same generated header writer as `parity gen` (naming trace, apps-status and scope-map SHA-256), LF endings, sorted. `--strict` exits 1 after writing if any finding exists. Bad input → `STATUS-INPUT`, nothing written. Golden tests for AC-1..AC-6 of spec node ECO.PARITY.STATUS on three OSes. BTreeMap only; no floats; no `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- Cross-domain (ECO.PARITY.TRACE): REQ-002 needs the trace's `commands[]` to list every command ID named in covering leaves' `journal:` fields, including commands not yet in the registry manifest (so not-yet-built rows can still be scoped). Confirm in TRACE's output contract; if it lists only manifest-present commands, the prefix fallback covers the rest.
- The seed-prefix map is an initial proposal (GOM and PW to Inspect, PCD to Probe, SA to Network; Report rows by area) for maintainers to adjust as data.

## Children

_Leaf: none._
