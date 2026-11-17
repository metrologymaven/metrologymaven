---
id: OUTR.SITE.PAGES.BLOG
title: Blog posts, release-note posts and Atom feed
level: 4
parent: OUTR.SITE.PAGES
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE.PAGES-004, REQ-OUTR.SITE.PAGES-005]
covers: []
depends_on: [OUTR.SITE.GEN.BUILD, OUTR.SITE.DL]
uses_artifacts: [ECO.REL.PROC.CHANNEL]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.SITE.PAGES.BLOG: Blog posts, release-note posts and Atom feed

## Purpose

Renders the project blog: news posts and release-note posts written in Markdown, a blog index, and an Atom feed generated at build time. Release-note posts must name a release that exists in the staged release manifests and link to its entry on the download page, so a post can never point at a release that was not published.

## Scope

**In:**
- Post sources `blog/<YYYY-MM-DD>-<slug>.md` with TOML front matter (`title`, `date`, `kind` = `news` or `release_notes`, `release` for release notes, `summary`)
- Post pages at `blog/<YYYY-MM-DD>-<slug>/index.html` and the index page `blog/index.html`
- Atom feed `blog/feed.xml`
- Checks on dates, release tags and slugs

**Out:**
- Generated release notes content (`ECO.REL.PROC.NOTES`; a post may link to it)
- The download page and its per-release anchors (`OUTR.SITE.DL`)
- Claim lint and capability-tag checks on post text (`OUTR.SITE.QA.RUN`, `OUTR.SITE.CLAIM`)
- Announcement channels beyond the site (`OUTR.LAUNCH.ANN`)

## Requirements

- **REQ-OUTR.SITE.PAGES.BLOG-001** Each post page shall show the post's title and its publication date from front matter in ISO 8601 form (`YYYY-MM-DD`).
- **REQ-OUTR.SITE.PAGES.BLOG-002** If a post's front-matter `date` differs from the date in its file name, or the file name does not match `^\d{4}-\d{2}-\d{2}-[a-z0-9-]{1,60}\.md$`, then the build shall fail and name the file.
- **REQ-OUTR.SITE.PAGES.BLOG-003** A post of kind `release_notes` shall show its `release` tag and a relative link to the download-page anchor of that release, built by the anchor function owned by OUTR.SITE.DL.
- **REQ-OUTR.SITE.PAGES.BLOG-004** If a `release_notes` post names a tag that has no staged `release-manifest.json`, or a `news` post carries a `release` field, then the build shall fail and name the file and tag.
- **REQ-OUTR.SITE.PAGES.BLOG-005** The build shall write `blog/feed.xml` as an Atom feed (RFC 4287) with one entry per post, sorted by date descending then slug ascending, each with title, absolute link built from `base_url` in `site.toml`, `id`, `updated` equal to the post date and summary.
- **REQ-OUTR.SITE.PAGES.BLOG-006** The feed's own `updated` value shall be the date of the newest post, and the feed shall contain no value taken from the system clock.
- **REQ-OUTR.SITE.PAGES.BLOG-007** The blog index page shall list every post in the same order as the feed, with date, title, kind and summary.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/site/pages/blog/ok` (three news posts and one release-notes post for `v0.1.0`, whose manifest is staged), when `fid-outreach site build` runs, then it exits 0, writes four post pages, `blog/index.html` and `blog/feed.xml` equal to the golden files, and the release-notes page links `../../download/index.html#release-v0.1.0`.
- **AC-2** (feed validity) Given the AC-1 `feed.xml`, when it is parsed by an XML parser and checked against the Atom required elements (feed id, title, updated; entry id, title, updated, link), then all are present and entries are in the specified order.
- **AC-3** (date mismatch) Given fixture `date-mismatch` (`2026-11-01-hello.md` with `date = 2026-11-02`), when the build runs, then it exits 1 with `BLOG-DATE blog/2026-11-01-hello.md`.
- **AC-4** (unknown release) Given fixture `unknown-release` (release notes for `v9.9.9`, not staged), when the build runs, then it exits 1 with `BLOG-RELEASE blog/2026-11-10-v999.md v9.9.9`.
- **AC-5** (bad name) Given fixture `bad-name` (`blog/Hello World.md`), when the build runs, then it exits 1 with `BLOG-NAME`.
- **AC-6** (determinism) Building `ok` twice, and with the system clock shifted, gives a byte-identical feed.

## Design

- **Command:** none of its own; the renderer `blog` runs inside `fid-outreach site build` and registers itself in `site::renderers::all()`. Nearest class W1.
- **Data structures:** `Post { slug, date: (u16, u8, u8), kind: News | ReleaseNotes, release: Option<Tag>, title, summary, body_html }`.
- **Feed:** Atom 1.0 (RFC 4287); feed `id` = `<base_url>/blog/`; entry `id` = `<base_url>/blog/<date>-<slug>/`; times written as `<date>T00:00:00Z`; XML text escaped; no generator element carrying a version string that changes per build.
- **Release link:** `dl::anchor(tag)` from OUTR.SITE.DL returns `release-<tag>`; the link is relative from the post page.
- **Errors:** `BLOG-NAME(file)`, `BLOG-DATE(file)`, `BLOG-RELEASE(file, tag)`, `BLOG-KIND(file, kind)`, `BLOG-FRONT(file, reason)`.
- **Determinism:** D0.
- **Performance budget:** 1 000 posts in under 1 s [unverified].
- **Crate:** `fid-outreach`, module `site::pages::blog`.

## Test fixtures

Synthetic fixtures under `fixtures/outr/site/pages/blog/` (`ok`, `date-mismatch`, `unknown-release`, `bad-name`) with golden pages and feed; CC-BY-4.0.

## Build prompt

> In the `fid-outreach` Rust crate (outreach tooling), add the renderer `blog` (module `site::pages::blog`) implementing the existing `Renderer` trait and register it in `site::renderers::all()`.
>
> Read `blog/*.md`; require names `YYYY-MM-DD-<slug>.md` (slug `[a-z0-9-]{1,60}`) and TOML front matter title, date, kind (`news` or `release_notes`), summary, and `release` only for release notes. Fail BLOG-NAME, BLOG-DATE (front-matter date differs from the name), BLOG-RELEASE (release tag with no staged `<tag>/release-manifest.json`, or a news post with a release), BLOG-KIND, BLOG-FRONT. Write each post to `blog/<date>-<slug>/index.html` with title and ISO date; release-notes posts show the tag and a relative link to `download/index.html#` + `dl::anchor(tag)`. Write `blog/index.html` and `blog/feed.xml` (Atom, RFC 4287) sorted by date descending then slug, absolute links from `base_url` in `site.toml`, entry updated = post date at 00:00:00Z, feed updated = newest post date; never read the clock.
>
> Tests: AC-1..AC-6 of spec node OUTR.SITE.PAGES.BLOG with golden files. Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; escape all text in XML and HTML; bounded post size (1 MiB); deterministic output.

## Open questions

- RSS was allowed by the parent; Atom alone is chosen because its required fields make the validity test concrete.

## Children

_Leaf: none._
