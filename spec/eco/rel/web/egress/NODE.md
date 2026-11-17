---
id: ECO.REL.WEB.EGRESS
title: Outbound-request and local-processing check
level: 4
parent: ECO.REL.WEB
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.WEB-002, REQ-ECO.REL.WEB-004]
covers: []
depends_on: [ECO.CI.WF, ECO.REL.WEB.ASSEMBLE]
uses_artifacts: [PLAT.WEB.BROWSERS]
side_effect: W1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.WEB.EGRESS: Outbound-request and local-processing check

## Purpose

Proves, for each release bundle, that it requests nothing from any origin other than the one serving it and never uploads a user's file. A headless browser of each tested engine loads the bundle from a local static server, runs a scripted session that opens a fixture file, and every request the browser makes is logged and checked.

## Scope

**In:**
- Local static server on a loopback address with a request log
- Headless-browser driver per tested engine (from the browser list), with request interception
- Scripted session: load `index.html`, open each app, load a fixture file through the file picker, navigate, close
- `web/egress-report.json`

**Out:**
- Browser support levels (owned by `PLAT.WEB.BROWSERS`) and general browser CI (owned by `PLAT.WEB.CI`)
- Building the bundle (owned by `ECO.REL.WEB.ASSEMBLE`)

## Requirements

- **REQ-ECO.REL.WEB.EGRESS-001** When the scripted session runs, the check shall record every request the browser issues, and shall fail if any request targets an origin other than the serving origin.
- **REQ-ECO.REL.WEB.EGRESS-002** When a fixture file is opened in the session, the check shall fail if any request body or URL contains a 32-byte marker embedded in that fixture file.
- **REQ-ECO.REL.WEB.EGRESS-003** The check shall fail if any request uses a method other than GET or HEAD.
- **REQ-ECO.REL.WEB.EGRESS-004** The check shall fail if the browser reports a CSP violation during the session.
- **REQ-ECO.REL.WEB.EGRESS-005** The check shall run with every browser engine listed as tested in the browser list, and the report shall name the engine and version for each run.

## Acceptance criteria

- **AC-1** Given the fixture bundle from `ECO.REL.WEB.ASSEMBLE`'s AC-1 and the fixture `fixtures/rel/web/egress/part.stl` with marker, when `cargo xtask rel web-egress --bundle out/ --engines chromium,firefox` runs, then it exits 0 and the report lists only same-origin GET requests.
- **AC-2** (error path) Given a bundle variant whose JS fetches `https://example.invalid/x`, then it exits 1 with `EGRESS-FOREIGN-ORIGIN` naming the URL and engine.
- **AC-3** (error path) Given a variant that POSTs the opened file to the serving origin, then it exits 1 with `EGRESS-METHOD` and `EGRESS-MARKER`.
- **AC-4** (error path) Given a variant that injects an inline script, then it exits 1 with `EGRESS-CSP-VIOLATION`.

## Design

- **Registry command(s):** none; repository tool `cargo xtask rel web-egress --bundle <dir> --engines <list> --out <dir>` (W1) driving browsers through a WebDriver-compatible protocol [unverified choice of driver protocol].
- **Data structures:** `EgressReport { bundle_sha256, runs: [{engine, version, requests: [{method, url, origin}], violations: [...]}] }`.
- **Algorithm:** start loopback server on a random port (seeded for reproducible logs), launch browser with a fresh profile and no extensions, run the script, collect requests from both browser interception and server log, compare.
- **Errors:** `EGRESS-FOREIGN-ORIGIN`, `EGRESS-MARKER`, `EGRESS-METHOD`, `EGRESS-CSP-VIOLATION`, `EGRESS-ENGINE-UNAVAILABLE`.
- **Determinism:** D2 (seeded port and script; browser timing is outside control, results compared as sets).
- **Performance budget:** under 5 min per engine.
- **Crate:** `fid-xtask` (module `rel::web_egress`).

## Test fixtures

`fixtures/rel/web/egress/` with a small synthetic STL carrying a marker and three faulty bundle variants; CC-BY-4.0.

## Build prompt

> Add module `rel::web_egress` to `fid-xtask`: `rel web-egress --bundle <dir> --engines <list> --out <dir>`. Serve the bundle from a loopback static server with a request log; for each engine (read the tested engines from the PLAT.WEB browser list file), launch a headless browser with a fresh profile through a WebDriver-compatible driver, intercept all requests, and run a script that opens index.html, each app, and loads `fixtures/rel/web/egress/part.stl` (contains a 32-byte marker) through the file input. Fail on any foreign origin (EGRESS-FOREIGN-ORIGIN), any method other than GET or HEAD (EGRESS-METHOD), the marker in any URL or body (EGRESS-MARKER), or any CSP violation (EGRESS-CSP-VIOLATION). Write `web/egress-report.json` with engine names and versions. Tests: AC-1..AC-4 of spec node ECO.REL.WEB.EGRESS using faulty fixture bundle variants. Constraints: no panics, deny unwrap/expect; the check itself makes no external requests.

## Open questions

- Service workers could cache and later fetch; the bundle registers none, and the check should fail if one is registered (add when PLAT.WEB.OFFLINE decides on offline caching).

## Children

_Leaf: none._
