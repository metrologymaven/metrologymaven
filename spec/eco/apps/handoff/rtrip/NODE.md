---
id: ECO.APPS.HANDOFF.RTRIP
title: Cross-app round-trip suite
level: 4
parent: ECO.APPS.HANDOFF
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS.HANDOFF-001, REQ-ECO.APPS.HANDOFF-005]
covers: []
depends_on: []
uses_artifacts: [ECO.APPS.MANIFEST.EXPORT, ECO.DATA.EX, AUTO.CLI.TARGET.APP, CORE.PRJ.CONT, CORE.JRN.CHAIN]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.APPS.HANDOFF.RTRIP: Cross-app round-trip suite

## Purpose

Proves REQ-ECO.APPS-004 on every change: a project opened and closed by one app, then by another, keeps every element ID, journal entry and hash-chain link. The suite is a test harness (tooling layer) that drives the built app binaries through the shared `fid` CLI's running-app target, for every ordered pair of apps that have a manifest, on the published example projects, and writes one JSON report.

## Scope

**In:**
- Pair enumeration from the apps that have a manifest (read from `apps-status.json`)
- The per-pair procedure: copy example → open in A → close → open in B → close → compare against the original
- The comparison: element ID set, journal entry digests in order, chain head and checkpoint digests
- A JSON report `rtrip-report.json` and the CI job definition that runs it (wired by ECO.CI.WF)

**Out:**
- Example projects (owned by `ECO.DATA.EX`)
- The running-app CLI target (owned by `AUTO.CLI.TARGET.APP`)
- Chain verification semantics (owned by `CORE.JRN.CHAIN`)
- Edits across apps (not a round trip; covered by ECO.APPS.HANDOFF.RO tests)

## Requirements

- **REQ-ECO.APPS.HANDOFF.RTRIP-001** The suite shall run every ordered pair (A, B) with A ≠ B of the apps whose `apps-status.json` entry has a manifest, on every example project listed for any app in the example index.
- **REQ-ECO.APPS.HANDOFF.RTRIP-002** For each pair and example, the suite shall fail the case if the element ID set, the ordered list of Record Journal entry digests, or the chain head digest after B differs from the original.
- **REQ-ECO.APPS.HANDOFF.RTRIP-003** The suite shall drive the apps only through registry commands sent with the shared CLI's running-app target, never through app-private interfaces.
- **REQ-ECO.APPS.HANDOFF.RTRIP-004** The suite shall write `rtrip-report.json` listing each case `{example, from, to, outcome: pass|fail, diff?}` sorted by example, from, to, and shall exit 1 if any case fails.
- **REQ-ECO.APPS.HANDOFF.RTRIP-005** If fewer than two apps have a manifest, then the suite shall exit 0 with an empty case list and the note `fewer-than-two-apps`.

## Acceptance criteria

- **AC-1** Given test apps `a` and `b` built from `fixtures/handoff/apps/` and example `fixtures/rtrip/ex-basic/` (index entry for app `a`), when `cargo xtask apps-rtrip --apps-status fixtures/rtrip/apps-status.json --examples fixtures/rtrip/index.toml --out <tmp>` runs, then two cases (a→b, b→a) pass and the exit code is 0.
- **AC-2** (error path) Given test app `c` built with a fault-injection feature that appends a spurious entry on open, when the suite runs with apps `a` and `c`, then cases a→c and c→a fail with a `diff` naming `journal-entries` and the exit code is 1.
- **AC-3** Given `fixtures/rtrip/apps-status-one.json` with one app, then the report has no cases, note `fewer-than-two-apps`, and exit code 0.
- **AC-4** (determinism) Running AC-1 twice gives byte-identical `rtrip-report.json`.
- **AC-5** A grep of the suite's sources finds no import of any app crate (it drives binaries only).

## Design

- **Registry command(s):** none; repository tool `cargo xtask apps-rtrip --apps-status <json> --examples <index> --bin-dir <dir> --out <dir>`. `side_effect: W1`: writes copies and the report into a local output root.
- **Data structures:** `Case { example, from, to }`; `Snapshot { element_ids: BTreeSet, entry_digests: Vec, chain_head }` taken with `fid project` and `fid journal` R0 commands run headless on the copy; report as above.
- **Algorithm:** for each case: copy the example to a temp dir; start A on it; run `fid --app <A> project open-project` and `close-project`; stop A; repeat with B; take a headless snapshot; compare with the snapshot of the untouched copy.
- **Errors:** `RTRIP-INPUT`, `RTRIP-LAUNCH`, `RTRIP-DIFF`.
- **Determinism:** D0 for the report (cases sorted, no timings in the report).
- **Performance budget:** under 5 minutes for 4 apps and 4 examples in CI [unverified until measured].
- **Crate:** `fid-xtask` (module `apps_rtrip`); tooling, so it reads artifacts and runs binaries only.

## Test fixtures

Fixture manifests and test app binaries from `fixtures/handoff/apps/`, a small example project and index under `fixtures/rtrip/` (MIT OR Apache-2.0).

## Build prompt

> In `fid-xtask` (implementation repository), add `apps-rtrip --apps-status <json> --examples <index.toml> --bin-dir <dir> --out <dir>`. Read the apps that have a manifest from `apps-status.json` (published by ECO.APPS.MANIFEST.EXPORT) and the example projects from the example index (ECO.DATA.EX). For every ordered pair (A, B), A ≠ B, and every example: copy the example; launch app A's binary on it; run `fid --app <A> project open-project` then `close-project` (the shared CLI's running-app target); stop A; do the same with B; then take a headless snapshot (element IDs, ordered Record Journal entry digests, chain head) with `fid` R0 commands and compare it with the untouched copy. Write `rtrip-report.json` (`cases` sorted by example, from, to; `outcome`, optional `diff` naming `element-ids`, `journal-entries` or `chain-head`); exit 1 on any failure; with fewer than two apps, exit 0 with note `fewer-than-two-apps`. Do not link any app or product crate: tooling drives binaries only (CONSTITUTION §2.1). Tests for AC-1..AC-5 of spec node ECO.APPS.HANDOFF.RTRIP, including a fault-injected test app. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- Until ECO.DATA.EX publishes examples, the suite runs on `fixtures/rtrip/` only.

## Children

_Leaf: none._
