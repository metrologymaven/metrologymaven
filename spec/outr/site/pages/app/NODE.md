---
id: OUTR.SITE.PAGES.APP
title: Home page app cards and per-app pages with generated status blocks
level: 4
parent: OUTR.SITE.PAGES
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE.PAGES-001, REQ-OUTR.SITE.PAGES-002, REQ-OUTR.SITE.PAGES-003]
covers: []
depends_on: [OUTR.SITE.GEN.BUILD, OUTR.SITE.GEN.IMPORT]
uses_artifacts: [ECO.APPS.LIST, ECO.PARITY.STATUS]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.SITE.PAGES.APP: Home page app cards and per-app pages with generated status blocks

## Purpose

Renders the home page and one page per focused app. Each app page joins a short hand-written source (`pages/apps/<id>.md`) with a generated block (phase, status, live against in-scope seed rows, known gaps, link to the full parity page) taken from the staged app list and the status report of the site's current release. The home page shows one card per app with the same generated status, so no status word on the site is hand-written. It is a renderer inside the site build, not a separate command.

## Scope

**In:**
- The `apps` renderer: home page (`index.html`) with hand-written intro from `pages/home.md` plus generated app cards; one page per app at `apps/<id>/index.html`
- The one-to-one check between the app list and `pages/apps/*.md`
- The generated status block and its status-label mapping
- The pre-release case (no release published yet)

**Out:**
- Blog and feed (`OUTR.SITE.PAGES.BLOG`)
- The full parity and status page that app pages link to (`OUTR.SITE.STATUS.PARITY`)
- Capability tags inside the hand-written text and their check (`OUTR.SITE.CLAIM`)
- Producing the app list and status report (`ECO.APPS.LIST`, `ECO.PARITY.STATUS`)

## Requirements

- **REQ-OUTR.SITE.PAGES.APP-001** The renderer shall produce exactly one page `apps/<id>/index.html` for each app `id` in the staged `apps.toml`, from the source `pages/apps/<id>.md`.
- **REQ-OUTR.SITE.PAGES.APP-002** If an app in `apps.toml` has no source file, or a file in `pages/apps/` has a stem that is not an app `id`, then the build shall fail and name the app or the file.
- **REQ-OUTR.SITE.PAGES.APP-003** Each app page shall show the app's phase from `apps.toml` and its status label from the `status.json` of the release named `current_release` in `site.toml`, mapped as `planned` to "pre-alpha", `alpha` to "alpha", `beta` to "beta" and `ga` or `stable` to "stable".
- **REQ-OUTR.SITE.PAGES.APP-004** If `status.json` gives an app a status value outside that mapping, or omits an app that `apps.toml` lists, then the build shall fail and name the app and the value.
- **REQ-OUTR.SITE.PAGES.APP-005** Each app page shall show the app's live and total in-scope seed-row counts and list every not-live in-scope row with its reason code, in the order given by `status.json`, plus a relative link to that release's parity page anchor `status/<release>/index.html#app-<id>`.
- **REQ-OUTR.SITE.PAGES.APP-006** While `current_release` is `none`, each app page and card shall show the status "pre-alpha", the text "No release yet" and no live or total counts.
- **REQ-OUTR.SITE.PAGES.APP-007** The home page shall show one card per app, in `apps.toml` order, with the app's name, its one-line job from `apps.toml`, its status label and a relative link to its page.
- **REQ-OUTR.SITE.PAGES.APP-008** If a hand-written app source contains one of the status words "pre-alpha", "alpha", "beta", "stable", "1.0" or "GA" outside a capability-tagged sentence, then the build shall fail and name the file and line, so that status is only stated by the generated block.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/site/pages/app/ok` (an `apps.toml` with inspect, probe, network and report, four sources, `current_release = "v0.1.0"` and a `status.json` giving report `alpha` 12/40 with 28 not-live rows and the others `planned`), when `fid-outreach site build` runs, then it exits 0, writes four app pages and an `index.html` equal to the golden files, and the report page lists 28 not-live rows.
- **AC-2** (missing page) Given fixture `missing` (no `pages/apps/network.md`), when the build runs, then it exits 1 with `APP-NO-PAGE network`.
- **AC-3** (stray page) Given fixture `stray` (an extra `pages/apps/scanner.md`), when the build runs, then it exits 1 with `APP-UNKNOWN pages/apps/scanner.md`.
- **AC-4** (bad status) Given fixture `bad-status` (`status.json` gives inspect the status `released`), when the build runs, then it exits 1 with `APP-STATUS inspect released`.
- **AC-5** (pre-release) Given fixture `no-release` (`current_release = "none"`, no status file), when the build runs, then every app page and card shows "pre-alpha" and "No release yet" and no counts.
- **AC-6** (hand-written status word) Given fixture `status-word` (`pages/apps/inspect.md` line 4 says "Inspect is now in beta."), when the build runs, then it exits 1 with `APP-STATUS-WORD pages/apps/inspect.md:4`.
- **AC-7** (determinism) Building `ok` twice gives byte-identical app pages and home page.

## Design

- **Command:** none of its own; the renderer `apps` runs inside `fid-outreach site build` (OUTR.SITE.GEN.BUILD) and registers itself in `site::renderers::all()`. Nearest class W1 (writes only into the build output).
- **Inputs (staged by OUTR.SITE.GEN.IMPORT):** `apps.toml` (ECO.APPS.LIST: `[[app]] id, job, owner, phase`); `<release>/status.json`; `site.toml` (`current_release`, display names per app id).
- **Status-report contract:** reads only `apps[]` with `id`, `status`, `live`, `total` and `not_live[]` (`id`, `reason`) from `status.json`. ECO.PARITY.STATUS has not fixed its schema; if its field names differ, a one-line mapping is added here [unverified schema].
- **Status words check:** case-insensitive whole-word match on the listed words, skipping fenced code, inline code and lines carrying a capability tag; bounded line length 4 KiB.
- **Output:** status block as a `<section aria-labelledby>` with a definition list for phase, status and counts and an ordered list for gaps; the block's text is generated, never taken from the source.
- **Errors:** `APP-NO-PAGE(id)`, `APP-UNKNOWN(file)`, `APP-STATUS(id, value)`, `APP-MISSING-STATUS(id)`, `APP-STATUS-WORD(file:line)`.
- **Determinism:** D0.
- **Performance budget:** negligible against the build (under 100 ms for 10 apps and 5 000 gap rows) [unverified].
- **Crate:** `fid-outreach`, module `site::pages::app`.

## Test fixtures

Synthetic fixtures under `fixtures/outr/site/pages/app/` (`ok`, `missing`, `stray`, `bad-status`, `no-release`, `status-word`) with golden pages; CC-BY-4.0. App ids follow the four D-004 apps; seed IDs in `status.json` are illustrative.

## Build prompt

> In the `fid-outreach` Rust crate (outreach tooling), add the renderer `apps` (module `site::pages::app`) implementing the existing `Renderer` trait of `site::build`, and register it in `site::renderers::all()`.
>
> Inputs from the staging folder: `apps.toml` (`[[app]]` id, job, owner, phase), `site.toml` (`current_release` tag or `none`, display name per app id) and `<current_release>/status.json` (read only `apps[]` id, status, live, total, not_live[] id and reason).
>
> Output: `index.html` from `pages/home.md` plus one card per app in apps.toml order (name, job, status label, relative link); `apps/<id>/index.html` from `pages/apps/<id>.md` plus a generated block with phase, status label (planned→pre-alpha, alpha, beta, ga or stable→stable), live/total counts, the not-live rows with reason codes in input order, and a relative link to `status/<release>/index.html#app-<id>`. With `current_release = none`, show "pre-alpha" and "No release yet" with no counts.
>
> Failures (exit 1 via the build): APP-NO-PAGE for an app without a source; APP-UNKNOWN for a source whose stem is not an app id; APP-STATUS for an unmapped status; APP-MISSING-STATUS for an app absent from status.json; APP-STATUS-WORD for the words pre-alpha, alpha, beta, stable, 1.0 or GA in a hand-written app source outside code and outside lines with a capability tag.
>
> Tests: AC-1..AC-7 of spec node OUTR.SITE.PAGES.APP with fixtures under `fixtures/outr/site/pages/app/` and golden HTML. Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; BTreeMap only; deterministic output; semantic HTML that passes the site accessibility gate.

## Open questions

- Editorial: how much hand-written text a pre-alpha app page carries is for maintainers; the renderer only guarantees the page and its generated block exist.
- Status vocabulary differs across nodes: OUTR.SITE uses pre-alpha, alpha, beta, stable; ECO.REL.PROC.CHANNEL uses alpha, beta, ga; ECO.PARITY.STATUS uses `planned` for apps without a manifest. The mapping in REQ-003 bridges them; ECO should settle one vocabulary.
- D-017 says offline subsets ship as beta labelled by scope. If ECO.PARITY.STATUS adds a scope label per app, the block should show it next to the status.

## Children

_Leaf: none._
