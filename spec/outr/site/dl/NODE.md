---
id: OUTR.SITE.DL
title: Download page
level: 3
parent: OUTR.SITE
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE-005]
covers: []
depends_on: [OUTR.SITE.GEN.BUILD, OUTR.SITE.GEN.IMPORT]
uses_artifacts: [ECO.REL.PROC.CHANNEL, ECO.REL.SIGN.SUMS, ECO.REL.SIGN.GUIDE]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.SITE.DL: Download page

## Purpose

The download page, generated from the ECO release manifests staged for the site: every artefact of every published release, each with its SHA-256 checksum copied from the manifest, the release's signed checksum file and its signature, and links to the per-OS verification guide. It is the one page where a wrong link or checksum directly harms users, so it is generated, checked against its own source after rendering, and every link is checked against the project-controlled host allowlist. It is a renderer inside the site build, not a separate command.

## Scope

**In:**
- The `download` renderer: `download/index.html` with one section per release (anchor `release-<tag>`), newest first
- Artefact table per release grouped by artefact kind, with OS, architecture, tier, size, SHA-256 and link
- Per-release verification row: `SHA256SUMS` and `SHA256SUMS.sig` links from the manifest
- Latest-release marker and release date per channel
- Links to the verification guide sections for Windows, Linux and macOS, and to the support matrix
- The post-render completeness check and the host allowlist check
- `dl::anchor(tag)`, used by blog release notes and docs

**Out:**
- Building, signing and hosting the artefacts, the release manifest schema and the channel names (`ECO.REL.PROC.CHANNEL`, `ECO.REL.SIGN.*`)
- The verification guide text and keys (`ECO.REL.SIGN.GUIDE`)
- The host allowlist file format and staging of manifests (`OUTR.SITE.GEN.IMPORT`)
- Release notes (`OUTR.SITE.PAGES.BLOG`)
- Support matrix content (`PLAT.MATRIX`; the page only links to it)

## Requirements

- **REQ-OUTR.SITE.DL-001** The download page shall show, for every release with a staged `release-manifest.json`, every artefact the manifest lists (installers, packages, archives, the web bundle and verification files) with its file name, kind, OS, architecture, tier and size.
- **REQ-OUTR.SITE.DL-002** After rendering, the renderer shall parse its own output and fail the build, naming the release and file, if the set of artefact rows of a release differs from the manifest's artefact list or any shown SHA-256 differs from the manifest's value.
- **REQ-OUTR.SITE.DL-003** Each artefact row shall show its SHA-256 exactly as recorded in the release manifest, and each release section shall link that release's `SHA256SUMS` and `SHA256SUMS.sig` taken from the manifest, which is how every artefact's checksum is signed (ECO.REL.SIGN).
- **REQ-OUTR.SITE.DL-004** If a release manifest lists no `SHA256SUMS` or no `SHA256SUMS.sig`, then the build shall fail and name the release.
- **REQ-OUTR.SITE.DL-005** If any artefact, checksum, signature or verification-guide URL does not use `https` or its host is not listed in the site's `hosts.toml`, then the build shall fail and name the URL and the release.
- **REQ-OUTR.SITE.DL-006** The download page shall link the verification guide sections for Windows, Linux and macOS, at the URLs configured in `site.toml`, from the top of the page and from every release section.
- **REQ-OUTR.SITE.DL-007** For each channel that occurs in the staged manifests, the page shall mark the newest release of that channel, by release date then version precedence, as "Latest <channel>" and show its release date.
- **REQ-OUTR.SITE.DL-008** Release sections shall carry the anchor returned by `dl::anchor(tag)`, which shall be `release-` followed by the tag, and shall be ordered by release date descending then tag.
- **REQ-OUTR.SITE.DL-009** While no release manifest is staged, the download page shall state "No release has been published yet" and show no download link.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/site/dl/ok` (manifests for `v0.1.0-beta.1` on beta and `v0.1.0` on stable, each with six artefacts plus `SHA256SUMS`, `SHA256SUMS.sig` and an SBOM, all URLs on an allowlisted host), when `fid-outreach site build` runs, then it exits 0, `download/index.html` equals the golden page, and it shows "Latest stable" on `v0.1.0` and "Latest beta" on `v0.1.0-beta.1` with their dates.
- **AC-2** (completeness) Given the AC-1 output, when the test parses each release section, then its rows equal the manifest's artefact list and every shown SHA-256 equals the manifest value.
- **AC-3** (self-check) Given a renderer test hook that drops one row after rendering, when the self-check runs, then the build exits 1 with `DL-MISMATCH v0.1.0 <file name>`.
- **AC-4** (host allowlist) Given fixture `foreign-host` (one artefact URL on `downloads.example.net`, not in `hosts.toml`), when the build runs, then it exits 1 with `DL-HOST https://downloads.example.net/... v0.1.0`; given `http-url` (an `http://` URL on an allowlisted host), it exits 1 with `DL-HOST`.
- **AC-5** (missing signature) Given fixture `no-sig` (manifest without `SHA256SUMS.sig`), when the build runs, then it exits 1 with `DL-NO-SIG v0.1.0`.
- **AC-6** (verification links) Given the AC-1 output, then the page links the three configured guide URLs at the top and once in each release section.
- **AC-7** (no release) Given fixture `none`, then the page says "No release has been published yet" and contains no `href` to any artefact.
- **AC-8** (anchor) `dl::anchor("v0.1.0")` returns `release-v0.1.0`, and the AC-1 page has an element with that `id`.
- **AC-9** (determinism) Building `ok` twice gives a byte-identical page.

## Design

- **Command:** none of its own; the renderer `download` runs inside `fid-outreach site build` (OUTR.SITE.GEN.BUILD) and registers itself in `site::renderers::all()`. Nearest class W1 (writes only into the build output).
- **Release-manifest contract (schema v1, ECO.REL.PROC.CHANNEL):** reads `version`, `channel`, `release_date` (`YYYY-MM-DD` prefix of the recorded date) and `artefacts[]` with `name`, `kind`, `os`, `arch`, `tier`, `url`, `size`, `sha256`. If the schema names these fields differently, a one-line mapping is added here [unverified field names].
- **Grouping:** kinds in the fixed order `installer`, `package`, `archive`, `web_bundle`, then any other kind alphabetically; verification files (`SHA256SUMS`, `SHA256SUMS.sig`, SBOMs) in a final "Verification files" group. Rows sorted by OS, architecture, then name.
- **Version precedence:** semantic-version precedence of the tag without the leading `v`.
- **Configuration (`site.toml`):** `verify_guide.windows`, `verify_guide.linux`, `verify_guide.macos` (URLs into ECO.REL.SIGN.GUIDE's `VERIFY.md`), `support_matrix_url` (PLAT.MATRIX page); all checked against `hosts.toml` (format owned by OUTR.SITE.GEN.IMPORT).
- **Untrusted text:** every manifest string is HTML-escaped; URLs are emitted only after the host check.
- **Errors:** `DL-MISMATCH(release, name)`, `DL-NO-SUMS(release)`, `DL-NO-SIG(release)`, `DL-HOST(url, release)`, `DL-CONFIG(key)`.
- **Determinism:** D0.
- **Performance budget:** 50 releases with 40 artefacts each in under 500 ms [unverified].
- **Crate:** `fid-outreach`, module `site::dl` (exports `anchor`).

## Test fixtures

Synthetic release manifests under `fixtures/outr/site/dl/` (`ok`, `foreign-host`, `http-url`, `no-sig`, `none`) that validate against `release-manifest.v1.json` from ECO.REL.PROC.CHANNEL, with dummy digests; a `hosts.toml` and `site.toml`; golden page. CC-BY-4.0.

## Build prompt

> In the `fid-outreach` Rust crate (outreach tooling, not a product command), add the renderer `download` (module `site::dl`) implementing the existing `Renderer` trait of `site::build`, register it in `site::renderers::all()`, and export `pub fn anchor(tag: &str) -> String` returning `release-<tag>`.
>
> Inputs from the staging folder: each `<tag>/release-manifest.json` (read `version`, `channel`, `release_date`, `artefacts[]` name, kind, os, arch, tier, url, size, sha256); `hosts.toml` (allowed hosts, exact, case-insensitive); `site.toml` keys `verify_guide.windows|linux|macos` and `support_matrix_url`.
>
> Output `download/index.html`: top links to the three verification guide sections and the support matrix; one section per release ordered by release date descending then tag, with `id = anchor(tag)`; per channel, mark the newest release (release date, then semver precedence) as "Latest <channel>" with its date; an artefact table grouped by kind (installer, package, archive, web_bundle, others alphabetically, then a "Verification files" group for SHA256SUMS, SHA256SUMS.sig and SBOMs) with name, kind, OS, arch, tier, size, SHA-256 and link; links to the release's SHA256SUMS and SHA256SUMS.sig and the three guide sections in every section. With no manifests, say "No release has been published yet" and emit no artefact links. HTML-escape every manifest string.
>
> Checks (fail the build, exit 1): DL-HOST for any URL that is not https or whose host is not in hosts.toml; DL-NO-SUMS / DL-NO-SIG when a manifest lacks SHA256SUMS or SHA256SUMS.sig; after rendering, parse your own output and fail DL-MISMATCH if any release's rows differ from its manifest artefacts or any shown SHA-256 differs; DL-CONFIG for a missing guide URL.
>
> Tests: AC-1..AC-9 of spec node OUTR.SITE.DL with fixtures under `fixtures/outr/site/dl/` and a golden page. Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; BTreeMap only; no network access; deterministic output; dependencies under the project SPDX allowlist.

## Open questions

- REQ-OUTR.SITE.DL-002 (old wording) asked for a signature link per artefact. ECO.REL signs the checksum file (`SHA256SUMS.sig`) and DSSE statements, not each artefact, so each artefact's checksum is covered by the release's signed `SHA256SUMS`; the requirements above follow ECO.REL. If ECO.REL later publishes per-artefact signatures, add a column.
- Whether nightly releases appear on the download page depends on whether `site import fetch` is run for nightly tags; the renderer shows whatever is staged. Maintainers decide which channels to stage.
- Release-manifest field names (`release_date`, `arch`) are assumed from ECO.REL.PROC.CHANNEL REQ-002 prose; confirm against `schemas/release-manifest.v1.json` when it exists.

## Children

_Leaf: none._
