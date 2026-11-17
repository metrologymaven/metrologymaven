---
id: OUTR.DOCS.FRAME.ED
title: Release editions, version list and latest alias
level: 4
parent: OUTR.DOCS.FRAME
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS.FRAME-001, REQ-OUTR.DOCS.FRAME-002, REQ-OUTR.DOCS.FRAME-003]
covers: []
depends_on: [OUTR.DOCS.FRAME.NAV]
uses_artifacts: [ECO.REL.PROC, ECO.REL.LTS]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.DOCS.FRAME.ED: Release editions, version list and latest alias

## Purpose

Arranges the built editions of the portal into one published tree: a version list page that shows supported editions first and the rest under an archive heading, a "latest" alias that always points at the newest stable supported edition, and a notice on every page of an out-of-support edition linking the latest one. It runs at publish time over all editions, so old editions get their notice without being rebuilt.

## Scope

**In:** the `docs-editions` subcommand; the support list input; `versions.html`; the `latest/` alias; the out-of-support notice.
**Out:** building an edition (`OUTR.DOCS.FRAME.NAV`); the release support policy and channel feeds (`ECO.REL.PROC`, `ECO.REL.LTS`); offline bundles (`OUTR.DOCS.FRAME.BUNDLE`).

## Requirements

- **REQ-OUTR.DOCS.FRAME.ED-001** When `docs-editions` runs, it shall write `versions.html` listing every edition marked supported in the support list under a "Supported" heading and every other edition present in the site under an "Archive" heading, newest first.
- **REQ-OUTR.DOCS.FRAME.ED-002** The tool shall write `latest/index.html` as a static redirect (meta refresh plus a visible link) to the newest edition whose channel is `stable` and which is supported, and shall never choose an edition whose version has a pre-release suffix.
- **REQ-OUTR.DOCS.FRAME.ED-003** If no stable supported edition exists, then the tool shall write no `latest/` alias and shall report `no_stable_edition`.
- **REQ-OUTR.DOCS.FRAME.ED-004** For every page of an edition that is not supported, the tool shall replace the frame's edition-notice region with a notice naming the edition and linking the latest edition by a relative URL, and for every page of a supported edition it shall leave the region empty.
- **REQ-OUTR.DOCS.FRAME.ED-005** Running the tool twice on its own output with the same support list shall change no byte.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/docs/ed/site` (editions `v0.1.0`, `v0.2.0`, `v0.3.0-beta.1`) and a support list marking `v0.2.0` stable and supported, `v0.1.0` unsupported and `v0.3.0-beta.1` beta and supported, when `fid-outreach docs-editions --site site --support support.toml` runs, then `latest/index.html` targets `v0.2.0/index.html`, `versions.html` lists `v0.3.0-beta.1` and `v0.2.0` as supported and `v0.1.0` under Archive.
- **AC-2** Given the AC-1 output, when a test reads any page of `v0.1.0`, then it shows the notice with a relative link to `latest/index.html`; pages of `v0.2.0` show no notice.
- **AC-3** Given a support list with only the beta edition supported, when the tool runs, then there is no `latest/` directory and the report has `no_stable_edition`.
- **AC-4** (idempotence) Given the AC-1 output, when the tool runs again, then no file changes.

## Design

- **Command:** `fid-outreach docs-editions --site <dir> --support <file> [--json]`. Project tooling; nearest class W1 (rewrites files in the local site tree).
- **Support list** (`support.toml`): `[[release]] tag, channel (stable | beta | nightly), supported (bool)`, generated in the release workflow from the ECO.REL channel feeds and support policy [unverified field source until ECO.REL.PROC and ECO.REL.LTS are leaves].
- **Notice region:** the frame's `<!-- edition-notice -->` placeholder becomes `<!-- edition-notice:begin -->...<!-- edition-notice:end -->` so later runs replace it in place.
- **Version order:** semantic-version precedence; pre-release suffixes (`-beta.N`, `-nightly.YYYYMMDD`) sort before the release.
- **Errors:** `SupportParse`, `SiteUnreadable`, finding `no_stable_edition`, finding `edition_not_in_site` (support list names a missing edition).
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `docs_editions`.

## Test fixtures

Synthetic site trees and support lists under `fixtures/outr/docs/ed/`. CC-BY-4.0.

## Build prompt

> Build `fid-outreach docs-editions --site <dir> --support <file> [--json]` in the Rust outreach tooling crate `fid-outreach`. Each top-level directory of `--site` named like a version tag is an edition built by `docs-build`. `support.toml` has `[[release]] tag, channel, supported`. Write `versions.html` (Supported heading, then Archive heading; newest first by semver precedence). Write `latest/index.html` as a meta-refresh redirect with a visible link to the newest edition with channel `stable`, `supported = true` and no pre-release suffix; if none, write no alias and report `no_stable_edition`. In every page of an unsupported edition, replace `<!-- edition-notice -->` (or an existing `<!-- edition-notice:begin -->...<!-- edition-notice:end -->` region) with a notice naming the edition and a relative link to `latest/index.html`; clear the region in supported editions. Re-running must change nothing.
>
> Acceptance tests under `fixtures/outr/docs/ed/` as listed in the spec (latest choice, archive listing, notices, no stable edition, idempotence).
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads; write files atomically; no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- How long a release stays supported is ECO's call; this leaf only reads the resulting list (carried from OUTR.DOCS).

## Children

_Leaf: none._
