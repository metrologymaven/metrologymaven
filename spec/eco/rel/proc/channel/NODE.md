---
id: ECO.REL.PROC.CHANNEL
title: Channels, promotion, release manifest and channel feeds
level: 4
parent: ECO.REL.PROC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.PROC-004, REQ-ECO.REL.PROC-005]
covers: []
depends_on: [ECO.REL.PROC.VERSION]
uses_artifacts: [ECO.REL.LTS]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.PROC.CHANNEL: Channels, promotion, release manifest and channel feeds

## Purpose

Defines the four channels (nightly, beta, stable, lts), the rule that promotes a beta to stable, the per-release `release-manifest.json` that lists every published artefact, the distribution-venue list, and the channel feed each installed app may read. Feeds are what `ECO.REL.UPD` verifies, and the release manifest is what OUTR's download pages render, so both formats are owned here and versioned.

## Scope

**In:**
- Channel definitions and `release/venues.toml` (where artefacts are published, with an `eu_distribution` flag per venue)
- Promotion rule from beta to stable; lts entry rule delegated to `ECO.REL.LTS`
- `release-manifest.json` schema v1 and its generator
- Channel feed payload schema v1 (`feeds/<channel>.json`, payload of a DSSE envelope), its generator, a monotonic sequence number and an expiry
- `cargo xtask rel promote-check`, `rel manifest`, `rel feed`

**Out:**
- Signing the feed and manifest (owned by `ECO.REL.SIGN.CUSTODY`)
- Uploading to venues (owned by `ECO.REL.PROC.FLOW`)
- Which LTS lines exist and their windows (owned by `ECO.REL.LTS`)
- Reading feeds in the product (owned by `ECO.REL.UPD`)

## Requirements

- **REQ-ECO.REL.PROC.CHANNEL-001** When a stable tag `vX.Y.Z` is checked, the promote check shall pass only if a tag `vX.Y.Z-beta.N` exists on the same commit, was published to the beta channel at least 7 days earlier [unverified period], and the exported issue list has no open issue labelled `release-blocker` for `X.Y.Z`.
- **REQ-ECO.REL.PROC.CHANNEL-002** The release manifest shall list, for every published artefact, its file name, kind, OS, architecture, tier, URL, size in bytes and SHA-256, plus the release version, channel, commit, release date, the app status label (`alpha`, `beta` or `ga`) of each app and the venues it is published to.
- **REQ-ECO.REL.PROC.CHANNEL-003** Each channel feed payload shall contain `schema_version`, `channel`, `sequence`, `published_at`, `expires_at`, and the latest release's version, date, release-notes URL, download-page URL, advisory IDs and artefact list with URLs and SHA-256.
- **REQ-ECO.REL.PROC.CHANNEL-004** When a feed is regenerated for a channel, its `sequence` shall be greater than the sequence of the previously published feed for that channel, and generation shall fail otherwise.
- **REQ-ECO.REL.PROC.CHANNEL-005** The lts feed shall carry one entry per supported LTS line listed by `ECO.REL.LTS`, each naming that line's latest patch release, and shall omit lines past their end of support.
- **REQ-ECO.REL.PROC.CHANNEL-006** The generators shall emit RFC 8785 canonical JSON so that the same inputs give byte-identical files.
- **REQ-ECO.REL.PROC.CHANNEL-007** The nightly feed and nightly artefacts shall be marked for signing with the nightly key, and the manifest generator shall refuse to mark any beta, stable or lts artefact for the nightly key.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rel/channel/promote-ok/` (beta tag on commit C published 8 days before, stable tag on C, empty issue export), when `cargo xtask rel promote-check --tag v1.4.0 --history history.json --issues issues.json` runs, then it exits 0.
- **AC-2** (error path) Given `promote-early/` (beta published 6 days before), then it exits 1 with `PROMOTE-SOAK 6<7`.
- **AC-3** (error path) Given `promote-blocker/` (one open `release-blocker` issue), then it exits 1 naming the issue number; given `promote-commit/` (stable tag on a different commit), it exits 1 with `PROMOTE-COMMIT`.
- **AC-4** Given fixture `fixtures/rel/channel/artefacts/` (six artefact files and a venues file), when `rel manifest` runs, then `release-manifest.json` validates against `schemas/release-manifest.v1.json` and is byte-identical to `expected-manifest.json`.
- **AC-5** (error path) Given a previous feed with sequence 12 and a generation request with sequence 12, then `rel feed` exits 1 with `FEED-SEQUENCE`.
- **AC-6** Given fixture `lts-lines.toml` with lines 1.2 (supported) and 1.0 (ended), when the lts feed is generated, then it contains exactly one line entry, `1.2`.

## Design

- **Registry command(s):** none; repository tools `cargo xtask rel promote-check`, `rel manifest --artefacts <dir> --venues <file> --out <dir>`, `rel feed --channel <c> --manifest <file> --previous <feed> --out <dir>`. They write only into a local output root (W1, `fs_effect: write_root`).
- **Data structures:** `venues.toml`: `[[venue]] id, kind (release_host|mirror|store), url_prefix, eu_distribution (bool), channels`. Manifest v1 and feed v1 as JSON Schemas under `schemas/`. Feed DSSE payload type `application/vnd.fid.channel-feed+json` (working name until D-002). `expires_at` = `published_at` + 365 days for beta, stable and lts and + 14 days for nightly [unverified suitability].
- **Algorithm:** promotion: look up beta tags on the same commit in the exported tag history (`history.json`: tag, commit, channel, published_at), take the earliest, compare days elapsed using the stable tag's timestamp (no wall clock). Manifest: hash every artefact, sort by file name. Feed: copy the latest release from the manifest, set sequence = previous + 1.
- **Errors:** `PROMOTE-SOAK`, `PROMOTE-BLOCKER(issue)`, `PROMOTE-COMMIT`, `PROMOTE-NO-BETA`, `FEED-SEQUENCE`, `MANIFEST-NIGHTLY-KEY`, `INPUT-PARSE` (exit 2).
- **Determinism:** D0 (no clock reads; times come from inputs).
- **Performance budget:** hashing 5 GB of artefacts in under 60 s on the reference CI machine [unverified].
- **Crate:** `fid-xtask` (module `rel::channel`); JSON Schemas in `schemas/`.

## Test fixtures

`fixtures/rel/channel/` with synthetic tag histories, issue exports, small dummy artefact files and expected outputs; CC-BY-4.0.

## Build prompt

> Add module `rel::channel` to the `fid-xtask` crate with three subcommands. (1) `rel promote-check --tag vX.Y.Z --history <json> --issues <json>`: pass only if a `vX.Y.Z-beta.N` tag exists on the same commit, its earliest beta publication is at least 7 days before the stable tag's timestamp, and no open issue labelled `release-blocker` targets X.Y.Z; errors PROMOTE-SOAK, PROMOTE-BLOCKER, PROMOTE-COMMIT, PROMOTE-NO-BETA. (2) `rel manifest --artefacts <dir> --venues release/venues.toml --out <dir>`: write `release-manifest.json` (schema v1: version, channel, commit, date, apps with status label alpha|beta|ga, venues, artefacts with name, kind, os, arch, tier, url, size, sha256, signing key class `release` or `nightly`); refuse nightly key class for non-nightly channels (MANIFEST-NIGHTLY-KEY). (3) `rel feed --channel <c> --manifest <file> --previous <feed> [--lts-lines release/lts.toml] --out <dir>`: write the feed payload (schema_version, channel, sequence = previous + 1, published_at, expires_at = +365 days or +14 for nightly, latest version, date, notes URL, download URL, advisory IDs, artefacts); for lts, one entry per supported line with its latest patch. All JSON in RFC 8785 canonical form; no wall-clock reads. Add JSON Schemas `schemas/release-manifest.v1.json` and `schemas/channel-feed.v1.json`. Tests: AC-1..AC-6 of spec node ECO.REL.PROC.CHANNEL. Constraints: no panics, deny unwrap/expect, bounded parsing, BTreeMap only.

## Open questions

- OUTR.SITE.DL mentions a "preview" channel; this node names the channels nightly, beta, stable and lts. OUTR should use these names.
- The 365-day feed expiry means a re-sign (human-approved) at least yearly even without a release; maintainers may prefer a different period.

## Children

_Leaf: none._
