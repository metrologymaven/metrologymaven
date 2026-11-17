---
id: OUTR.SITE.GEN.BUILD
title: Single-command static site build and shared page template
level: 4
parent: OUTR.SITE.GEN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE.GEN-001, REQ-OUTR.SITE.GEN-002, REQ-OUTR.SITE.GEN-003]
covers: []
depends_on: [OUTR.SITE.GEN.IMPORT, OUTR.SITE.CLAIM]
uses_artifacts: [UX.TOKENS, OUTR.BRAND.ASSET]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.SITE.GEN.BUILD: Single-command static site build and shared page template

## Purpose

The one command that turns the site repository at a given revision into a folder of static files. It verifies and stages the generated inputs (through OUTR.SITE.GEN.IMPORT), runs every registered page renderer (app, blog, status, roadmap and download pages), wraps each page in the shared template built from the UX design tokens and OUTR.BRAND assets, runs the capability-claim check (OUTR.SITE.CLAIM), and writes byte-reproducible output. It is project tooling in the `fid-outreach` crate, not a product registry command.

## Scope

**In:**
- `fid-outreach site build`: the documented single build command, its exit codes and its pinned toolchain (`rust-toolchain.toml`, `Cargo.lock`)
- Hand-written page sources (`pages/**/*.md` and `pages/**/*.html` with TOML front matter) rendered to HTML
- The renderer interface that the other SITE leaves implement (`Renderer`: name, and a pure function from staged inputs to a sorted list of `(output path, page body, title)`)
- The shared page template: layout, navigation, footer with site revision and build-manifest link; CSS generated from the UX token file (light and dark themes); OUTR.BRAND assets and self-hosted font files copied into the output
- Output rules that make the result static, relative-linked and byte-identical across runs
- Calling the capability-claim check on page sources and failing the build on any finding

**Out:**
- Verifying and staging generated inputs and writing the build manifest (`OUTR.SITE.GEN.IMPORT`)
- Self-hosting guide and generic-server test (`OUTR.SITE.GEN.HOST`)
- The content of app, blog, status, roadmap and download pages (`OUTR.SITE.PAGES.*`, `OUTR.SITE.STATUS.*`, `OUTR.SITE.DL`)
- Tracker, accessibility, lint and size gates (`OUTR.SITE.QA.*`)
- Token values and brand assets themselves (`UX.TOKENS`, `OUTR.BRAND.ASSET`)

## Requirements

- **REQ-OUTR.SITE.GEN.BUILD-001** The command `fid-outreach site build --site <dir> --out <dir>` shall build the whole site from a clean checkout with no other preparatory step, and the site repository shall pin the Rust toolchain in `rust-toolchain.toml` and every crate version in a committed `Cargo.lock`.
- **REQ-OUTR.SITE.GEN.BUILD-002** When run twice from the same site revision and the same staged inputs, on Windows, Linux or macOS, the build shall produce output trees whose file lists and file bytes are identical.
- **REQ-OUTR.SITE.GEN.BUILD-003** The build shall take every date or time it writes from the inputs or from the `--revision-date` argument, and shall never read the system clock, the user name, the host name or absolute paths into the output.
- **REQ-OUTR.SITE.GEN.BUILD-004** The build output shall contain only files with the extensions on a fixed allowlist (`.html`, `.css`, `.svg`, `.png`, `.jpg`, `.webp`, `.woff2`, `.json`, `.md`, `.xml`, `.txt`, `.mp4`, `.webm`), and the build shall fail and name the file if any other file would be written.
- **REQ-OUTR.SITE.GEN.BUILD-005** Every internal link, script, stylesheet, image and font reference in a built page shall be a relative URL, so that the output can be served from any path prefix.
- **REQ-OUTR.SITE.GEN.BUILD-006** The shared template shall take every colour, font family, font size and spacing value from the UX token file staged by the import step, and the build shall fail and name the template file and value if a template stylesheet contains a literal colour, font size or spacing value.
- **REQ-OUTR.SITE.GEN.BUILD-007** Every built page shall contain the site revision and a relative link to `build-manifest.json` in its footer.
- **REQ-OUTR.SITE.GEN.BUILD-008** If the capability-claim check reports any finding on the page sources, then the build shall exit 1, print the findings and leave no partial output in `--out`.
- **REQ-OUTR.SITE.GEN.BUILD-009** If two renderers or page sources produce the same output path, then the build shall exit 1 naming both producers and the path.
- **REQ-OUTR.SITE.GEN.BUILD-010** The build shall exit 0 on success, 1 on a content or check failure, and 2 on an unreadable or unparsable input.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/site/build/minimal` (two Markdown pages, one HTML page, a token file and staged inputs), when `fid-outreach site build --site minimal --out out --revision abc123 --revision-date 2026-11-17` runs, then it exits 0 and `out/` equals the golden tree `expected/` file by file and byte by byte.
- **AC-2** (determinism) Given the same fixture, when the build runs twice into different output folders, with page sources copied in reverse directory order the second time, then the two trees are byte-identical; the same holds between the Windows, Linux and macOS CI runners.
- **AC-3** (static output) Given fixture `bad-ext` (a page that copies `deploy.php`), when the build runs, then it exits 1 naming `deploy.php`.
- **AC-4** (relative links) Given the `minimal` output, when every `href` and `src` attribute and every CSS `url()` is parsed, then none starts with `/`, a scheme or `//` except external links in page body text that carry `rel="external"`.
- **AC-5** (tokens) Given fixture `literal-colour` (a template stylesheet containing `#ff0000`), when the build runs, then it exits 1 naming the stylesheet and `#ff0000`.
- **AC-6** (claim check) Given fixture `claim-fail` (a page tagging a capability marked not live in the staged parity file), when the build runs, then it exits 1, prints one `not_live` finding and `out/` does not exist.
- **AC-7** (collision) Given fixture `collision` (a hand-written `pages/download/index.md` and the download renderer both writing `download/index.html`), when the build runs, then it exits 1 naming both producers.
- **AC-8** (no clock) Given the `minimal` fixture built with the system clock set one year later, then the output is byte-identical to `expected/`.

## Design

- **Command:** `fid-outreach site build --site <dir> --out <dir> --revision <git sha> --revision-date <YYYY-MM-DD>`. Project tooling, not a registry command; nearest class W1 (writes only into the given local output folder). The documented one-liner in the site README is `cargo run --locked -p fid-outreach -- site build --site . --out public --revision $(git rev-parse HEAD) --revision-date $(git log -1 --format=%cs)`.
- **Pipeline:** (1) call `import::verify_and_stage(site/inputs.lock.toml)` from OUTR.SITE.GEN.IMPORT into a temporary staging folder; (2) run `cap_check` (OUTR.SITE.CLAIM) on `pages/` with the staged `parity.json` of the site's current release; (3) render hand-written pages (Markdown with `pulldown-cmark` [unverified licence MIT]; front matter `title`, `nav`, `order`); (4) run every registered `Renderer` in name order; (5) check path collisions; (6) wrap bodies in the template (`minijinja` [unverified licence Apache-2.0], templates under `site/templates/`); (7) write CSS generated from the token file; (8) copy assets; (9) write into a temporary folder next to `--out` and rename into place only on success.
- **Data structures:** `PageOut { path: RelPath, title: String, body: String, producer: &'static str }`; `trait Renderer { fn name(&self) -> &'static str; fn render(&self, inputs: &Staged) -> Result<Vec<PageOut>, RenderError>; }`. Renderers are compiled in (no plugin loading) through the list `site::renderers::all()`, which this leaf creates empty; each renderer leaf adds its own entry, so the dependency runs from renderers to this leaf and never back.
- **Determinism rules:** sorted traversal; `BTreeMap` only; LF line endings; no timestamps except inputs and `--revision-date`; file mode not recorded; HTML minification off.
- **Token CSS:** the UX token file is read only for the semantic tier names `text.*`, `surface.*`, `border.*`, `focus.*`, `brand.*` and `type.*`, and written as CSS custom properties; `prefers-color-scheme` selects the dark theme. Until UX.TOKENS publishes its package format, the site keeps a small placeholder token file with the same names [unverified format].
- **Errors:** `BUILD-EXT(file)`, `BUILD-COLLISION(path, a, b)`, `BUILD-LITERAL(file, value)`, `BUILD-CLAIM(n)`, `BUILD-INPUT(path, reason)` (exit 2), `BUILD-IO`.
- **Determinism:** D0.
- **Performance budget:** 500 pages in under 10 s on a PLAT reference machine [unverified until measured].
- **Crate:** `fid-outreach` (outreach tooling workspace member), module `site::build`.

## Test fixtures

Synthetic fixtures under `fixtures/outr/site/build/` (`minimal`, `bad-ext`, `literal-colour`, `claim-fail`, `collision`) with a golden `expected/` tree; CC-BY-4.0 with the spec text (D-001). The placeholder token file and a one-glyph test font are generated in the fixture; no third-party asset is needed.

## Build prompt

> Build the `site build` subcommand of the `fid-outreach` Rust crate (outreach tooling, not part of the product or its command registry). Signature: `fid-outreach site build --site <dir> --out <dir> --revision <sha> --revision-date <YYYY-MM-DD>`.
>
> Pipeline: verify and stage generated inputs by calling the existing `import::verify_and_stage` function (module `site::import`) on `<site>/inputs.lock.toml`; run the existing `cap_check` module on `<site>/pages` with the staged `parity.json` and fail on any finding; render Markdown and HTML page sources (TOML front matter: title, nav, order) to HTML; define the `Renderer` trait (`name()`, `render(&Staged) -> Result<Vec<PageOut>>`) and an initially empty registration list `site::renderers::all()`, and call every registered renderer in name order; fail on any duplicate output path naming both producers; wrap each body in the shared template (layout, navigation, footer with the revision and a relative link to `build-manifest.json`); generate CSS custom properties from the staged UX token file (semantic names text.*, surface.*, border.*, focus.*, brand.*, type.*; dark theme under `prefers-color-scheme`); fail if any template stylesheet contains a literal colour, font size or spacing value; copy brand assets and self-hosted fonts; write to a temporary folder and rename into `--out` only on success.
>
> Rules: output only files with extensions .html .css .svg .png .jpg .webp .woff2 .json .md .xml .txt .mp4 .webm (fail and name any other); every internal reference is a relative URL; never read the system clock, user, host or absolute paths into output; sorted traversal, BTreeMap only, LF endings, so two builds of the same inputs are byte-identical on Windows, Linux and macOS. Exit 0 success, 1 content or check failure, 2 unreadable or unparsable input.
>
> Acceptance tests (fixtures under `fixtures/outr/site/build/`): `minimal` matches a golden tree; two runs and three OSes give identical bytes; `bad-ext` fails naming `deploy.php`; no absolute or scheme URLs in internal references; `literal-colour` fails naming `#ff0000`; `claim-fail` exits 1 with no output folder; `collision` names both producers; a shifted system clock does not change output.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded file sizes (reject page sources over 1 MiB); no network access; dependencies only under licences on the project SPDX allowlist (MIT OR Apache-2.0 compatible, no MPL or GPL family). Do not copy text, styles or assets from any other website.

## Open questions

- **Existing site project evaluated (2026-11-17):** `../../metrology maven/site` is a hand-written HTML site for a separate consultancy brand. It loads fonts from a third-party origin (fails REQ-OUTR.SITE-007), carries multi-megabyte 3D assets and vendor names on every page, and has no generator or data import. It is not reused; at most its colour-scheme handling may inform the template. A maintainer may overrule this in review.
- The generator is written in Rust inside `fid-outreach` to share code with the claim check; CONSTITUTION §2 does not say whether the Rust-first rule binds project tooling, so this is a choice, not a constraint.
- The UX token package format is not yet fixed by UX.SYSTEM; the placeholder token file must be replaced when it is.

## Children

_Leaf: none._
