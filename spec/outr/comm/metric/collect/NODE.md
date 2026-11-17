---
id: OUTR.COMM.METRIC.COLLECT
title: Public-signal collector
level: 4
parent: OUTR.COMM.METRIC
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.COMM.METRIC-002, REQ-OUTR.COMM.METRIC-003, REQ-OUTR.COMM.METRIC-005]
covers: []
depends_on: [OUTR.COMM.METRIC.DEF, OUTR.COMM.METRIC.SNAP]
uses_artifacts: [OUTR.COMM.CHAN]
side_effect: W1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.COMM.METRIC.COLLECT: Public-signal collector

## Purpose

The tool that gathers adoption signals from public sources only: it reads public JSON endpoints and public pages without credentials, counts distinct contributors from public repository history without storing email addresses, and appends a raw snapshot for each metric. It refuses to run if any configured source fails the definition guard, so product data can never enter the adoption figures.

## Scope

**In:** the `metric-collect` command; the `http_json` adapter (unauthenticated GET, size and time limits); the `git_history` adapter (distinct contributors from a public clone); the `manual_public_count` entry path (a maintainer types a count read from a public page, with the page URL); appending snapshots.
**Out:** definitions, source list and deny-list (`OUTR.COMM.METRIC.DEF`); the snapshot format and quarterly values (`OUTR.COMM.METRIC.SNAP`); scheduling (project CI).

## Requirements

- **REQ-OUTR.COMM.METRIC.COLLECT-001** Before fetching anything, the collector shall run the definition guard over the definition and source files, and if it reports any finding, then the collector shall fetch nothing and exit non-zero.
- **REQ-OUTR.COMM.METRIC.COLLECT-002** The `http_json` adapter shall send unauthenticated GET requests only, with no cookies, no credentials and a generic user agent, to the source URL in the source list, and shall refuse redirects to another host.
- **REQ-OUTR.COMM.METRIC.COLLECT-003** The `http_json` adapter shall refuse responses larger than 5 MB or slower than 30 s and record the metric as not collected with the reason, without writing a snapshot.
- **REQ-OUTR.COMM.METRIC.COLLECT-004** The `git_history` adapter shall count distinct contributors in a given period from a public clone, identifying contributors by a salted hash of the normalised author identity held only in memory, and shall store neither email addresses nor names.
- **REQ-OUTR.COMM.METRIC.COLLECT-005** For a `manual_public_count` metric, the collector shall accept a value only with the public page URL it was read from, record the maintainer handle, and store the URL and value as the raw response.
- **REQ-OUTR.COMM.METRIC.COLLECT-006** Each successful collection shall append one snapshot per metric through the snapshot store, with the raw response bytes and their SHA-256.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/comm/metric/collect/ok/` (definitions and sources pointing at a local test HTTP server that serves a JSON body with `stargazers = 180`), when `fid-outreach metric-collect --defs defs.toml --sources sources.toml --store store/ --at 2027-03-30T12:00:00Z --json` runs, then it exits 0 and the store holds a stars snapshot with value 180 and a raw file whose SHA-256 matches the record.
- **AC-2** (error path) Given `guard-fail/` (a source with `public = false`), when the collector runs, then it exits 1, the test server receives no request, and the store is unchanged.
- **AC-3** Given `big/` (the server returns 6 MB), when the collector runs, then that metric is `not_collected` with `response_too_large` and no snapshot is written.
- **AC-4** Given `redirect/` (the server redirects to another host), when the collector runs, then the metric is `not_collected` with `redirect_refused`.
- **AC-5** Given `git/` (a local bare repository with commits by three identities, one using two email casings), when the `git_history` adapter runs for the period, then the value is 3 and neither the snapshot nor the raw file contains an email address or name.
- **AC-6** Given `manual/`, when `metric-collect --manual forum_posts=412 --manual-url https://forum.example.org/about --by alice` runs, then the snapshot stores the value, URL and handle.
- **AC-7** (header check) Given the test server's request log from AC-1, then the request has no `Cookie` or `Authorization` header.

## Design

- **Command:** `fid-outreach metric-collect --defs <file> --sources <file> --store <dir> --at <timestamp> [--only <metric>]... [--manual <id>=<n> --manual-url <url> --by <handle>] [--json]`. Project tooling; nearest class W1 (writes only to the store; outgoing requests carry no user data).
- **HTTP:** a small blocking client with TLS, timeout 30 s, body cap 5 MB, no cookie jar, user agent `outreach-metrics/<version>`, redirects followed only within the same host.
- **Git:** clone or fetch the public repository URL into a temp folder; walk commits whose commit time falls in the period; identity = lowercase trimmed email, else name; hash with SHA-256 and a per-run random salt; count distinct hashes; discard the set; raw response = `{ "head": <commit id>, "period": ..., "count": n }`.
- **Errors:** `guard_failed`, `response_too_large`, `timeout`, `redirect_refused`, `selector_no_match`, `manual_url_missing`; per-metric failures do not stop other metrics; exit 1 if any metric failed.
- **Determinism:** N (public sources change); the `--at` timestamp is supplied, not read from the clock, so records are reproducible given the same responses.
- **Crate:** `fid-outreach`, module `metrics::collect`.

## Test fixtures

A local HTTP test server in the test harness serving fixture bodies; a bare git repository built by the test with fictional identities; fixtures under `fixtures/outr/comm/metric/collect/` (`ok`, `guard-fail`, `big`, `redirect`, `git`, `manual`), CC-BY-4.0. Tests never touch the internet.

## Build prompt

> In the Rust crate `fid-outreach`, add module `metrics::collect` and subcommand `fid-outreach metric-collect --defs <file> --sources <file> --store <dir> --at <timestamp> [--only <metric>]... [--manual <id>=<n> --manual-url <url> --by <handle>] [--json]`. It uses `metrics::def` (validation and deny-list) and `metrics::snap` (append-only `SnapshotStore`).
>
> First run the definition guard; on any finding, fetch nothing and exit 1. Methods: `http_json` (unauthenticated GET, no cookies or auth headers, user agent `outreach-metrics/<version>`, 30 s timeout, 5 MB cap, same-host redirects only, value via the JSON Pointer selector); `git_history` (clone the public repo into a temp folder, count distinct contributors with commit time in the period using SHA-256 of the lowercased email or name with a per-run random salt, held only in memory; store only head commit, period and count); `manual_public_count` (value given with `--manual`, requires `--manual-url` and `--by`). Append one snapshot per metric with raw bytes and SHA-256; failures are recorded as `not_collected` with a reason and other metrics continue. Never read the clock: use `--at`.
>
> Tests (local test HTTP server and a generated bare git repo; no internet): ok fetch, guard failure sends no request, oversize response, cross-host redirect, three distinct contributors with no email or name stored, manual entry, and no Cookie or Authorization header sent.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; never read product telemetry, crash reports or support bundles; BTreeMap rather than HashMap; allowlisted dependency licences only.

## Open questions

- Chat and forum activity counts depend on the providers chosen in OUTR.COMM.CHAN; where no public JSON endpoint exists, `manual_public_count` is the fallback.
- The repository host's public API may rate-limit unauthenticated requests [unverified]; a token would make the request authenticated, which REQ-002 forbids, so collection frequency may need to be low.

## Children

_Leaf: none._
