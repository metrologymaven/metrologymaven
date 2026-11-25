# Contributing to the {{FAMILY_NAME}} Design System

Spec node: `UX.SYSTEM.TOOL` (REQ-UX.SYSTEM.TOOL-001 .. -005). Licence: CC-BY-4.0.

This guide explains where design files live, which design tool the project recommends, how
`design/sources.json` tracks design sources and their committed exports, and how anyone —
with or without a design-tool account — can propose a token, icon, component, pattern,
screen or report style.

## 1. Repository text files are the single source of truth

The UTF-8 text and SVG files committed under `design/` are the single source of truth for
the {{FAMILY_NAME}} design system (`UX.SYSTEM.PKG`). Every token, name index entry, icon,
component spec, pattern, screen wireframe and report style must live in the repository in
open, diffable formats (JSON, TOML, Markdown, SVG, plus committed PDF/PNG reference exports).

- **Token values are authored in `design/tokens/`**, never only inside a design tool
  (REQ-UX.SYSTEM.TOOL-001). If you use a design tool to draw screens or mockups, import or
  mirror token values from `design/tokens/`; changes to token values happen by editing the
  DTCG JSON files in `design/tokens/` and passing `fid-design color-check`.
- **Placeholder product name:** until decision D-002 names the product, every design artefact
  names the product only as `{{FAMILY_NAME}}` and never includes candidate product names
  (`design/review/checklist.md`, `C-NAME`).
- **Originality and trade dress:** follow `design/principles.md` (especially `P-FAMILIAR` and
  `P-NOCOLOR`) and `design/writing-guide.md`. Never copy another product's code, text, icon,
  screenshot or layout, and never attach competitor screenshots as references.

## 2. Recommended design tool: Penpot

The recommended visual design tool is **Penpot** (REQ-UX.SYSTEM.TOOL-001).

- **Why Penpot:** Penpot is open source, self-hostable and usable without a paid account
  [unverified current Penpot licence and export formats; check at adoption]. That keeps every
  design source retrievable and exportable to open formats even if a hosted service changes
  terms (REQ-UX.SYSTEM-006).
- **Why not Figma by default:** Figma's free tier would satisfy openness only if every token
  and visual export were mirrored into the repository, and a contributor would still need a
  proprietary account to open and edit the source file.
- **Other open tools:** Inkscape (open-source vector editor for SVG icons and diagrams) and
  plain text editors (for JSON, TOML, Markdown and hand-authored SVG) are first-class tools in
  `design/sources.json`.
- **Paid-account tools are refused:** any source whose tool row in `design/sources.json` sets
  `requires_paid_account = true` fails `fid-design sources-check` with `SOURCE-PAID-TOOL`
  (REQ-UX.SYSTEM.TOOL-004).

## 3. Contributing with only a text editor and SVG files

You do **not** need a Penpot account or any hosted design-tool account to contribute to
{{FAMILY_NAME}} (REQ-UX.SYSTEM.TOOL-005). Every artefact kind can be authored locally with
standard text and SVG tools:

- **Tokens, themes, status colours, palettes and writing rules** are plain JSON and Markdown
  files under `design/`. Edit them with any text editor.
- **Icons and GD&T symbols** are clean SVG files (`design/icons/`, `design/gdt/`). Author
  them by hand in a text editor or locally in Inkscape; no account is required.
- **Components and patterns** are Markdown and JSON specifications (`design/components/`,
  `design/patterns/`) with optional hand-authored SVG diagrams.
- **Screens and report styles** can be proposed as structured Markdown wireframes, hand-drawn
  SVG layouts, or locally exported PDF/PNG sheets registered with tool `text-svg` or `inkscape`
  in `design/sources.json`.

## 4. Design source registry (`design/sources.json`)

Every design source referenced by a UX spec node is registered in `design/sources.json`
(`design-sources@1`, REQ-UX.SYSTEM.TOOL-002).

### Format

```json
{
  "format": "design-sources@1",
  "tools": [
    {
      "id": "penpot",
      "name": "Penpot",
      "open_source": true,
      "requires_paid_account": false,
      "export_extensions": ["penpot"]
    }
  ],
  "sources": [
    {
      "id": "report-summary-v1",
      "node": "UX.REPORT.MOCK",
      "tool": "penpot",
      "location": "report/mockups/summary/summary.penpot",
      "exports": [
        {
          "path": "report/mockups/summary/summary.pdf",
          "sha256": "sha256:<64 lowercase hex digits>"
        },
        {
          "path": "report/mockups/summary/summary.penpot",
          "sha256": "sha256:<64 lowercase hex digits>"
        }
      ]
    }
  ]
}
```

### Rules enforced by `fid-design sources-check`

1. **`tools` table:** each entry declares `id`, `name`, `open_source` (`bool`),
   `requires_paid_account` (`bool`), and optional `export_extensions` (lowercase extensions
   without a leading dot, such as `["penpot"]`).
2. **`sources` entries:** each entry declares a unique `id`, a valid UX spec `node` ID
   (checked against `spec/` when `--spec` is passed; unknown nodes fail with
   `SOURCE-NODE-UNKNOWN`), a `tool` listed in `tools` (`requires_paid_account = true` fails
   with `SOURCE-PAID-TOOL`), a public `location` (a clean relative repository path or a public
   `https://` share URL), and a non-empty `exports` list.
3. **Open-format committed exports:** every source must commit at least one open-format export
   (`.svg`, `.pdf` or `.png`, plus the tool's own export file such as `.penpot` where the tool
   offers one). Missing exports fail with `SOURCE-EXPORT-MISSING`.
4. **Digest verification:** every listed export must exist as a regular file below `design/`
   whose SHA-256 matches `sha256`. A mismatch fails with `SOURCE-EXPORT-DIGEST`.
5. **Offline only:** `fid-design sources-check` inspects committed files on disk and never
   accesses the network or fetches `location` URLs.

Compute an export's digest from the repository root:

```sh
sha256sum design/path/to/export.svg | awk '{print "sha256:" $1}'
```

## 5. How to propose each artefact kind

Every design change follows four steps:

1. **Open an issue** using the **Design proposal** issue template
   (`.github/ISSUE_TEMPLATE/design-proposal.yml`) describing the artefact kind, target UX spec
   node, user need and confirmation that the work is original. Small fixes can open a PR
   directly and reference the relevant UX spec node.
2. **Add or edit the artefact files** under `design/` (and register any design source and its
   committed exports in `design/sources.json`). Update `design/CHANGELOG.md` and
   `design/deprecations.json` when changing published names (`UX.SYSTEM.PKG`).
3. **Add an `originality@1` declaration** beside each new or changed artefact as
   `<file>.origin.json` (or under `design/review/origin/<file>.origin.json`, never both).
   Follow `design/review/checklist.md`: state author, date (`YYYY-MM-DD`), references
   described in plain words only (no URLs, no image files, no competitor screenshots) and
   `"statement_version": "originality-statement@1"`.
4. **Obtain a human `design-review@1` record** under `design/review/records/` in the same PR.
   A human reviewer (never an agent or bot) checks every item in `design/review/checklist.md`
   (`C-PRINCIPLES`, `C-ORIGINALITY`, `C-DRESS`, `C-NOCOLOR`, `C-KEYBOARD`, `C-CONTRAST`,
   `C-NAME`), records each trade-dress rule result from `design/review/dress-rules.json`,
   records the SHA-256 digest of every changed artefact, and signs off as `"principal": "human"`.

### Artefact-by-artefact guide

#### 5.1 Token (colour, typography, spacing, theme, status, data palette)

- **Issue template:** `.github/ISSUE_TEMPLATE/design-proposal.yml` (`kind: token`, citing
  `UX.TOKENS.*`).
- **Files to add / edit:** DTCG JSON token files under `design/tokens/` (`primitive/`,
  `semantic/`, `component/`, `themes/`, `pairs.json`, `status.json`, `palettes/`), plus
  `design/CHANGELOG.md` and `design/deprecations.json` on published-name changes.
- **Originality declaration:** `design/review/origin/tokens/<path>.origin.json` (or beside
  the file).
- **Review record:** `design/review/records/<pr>.json` with `C-CONTRAST` backed by
  `fid-design color-check --root design` output.
- **Text-editor-only workflow:** author and edit DTCG JSON files directly in any text editor;
  no design tool is needed.

#### 5.2 Icon (UI icon or GD&T symbol)

- **Issue template:** `.github/ISSUE_TEMPLATE/design-proposal.yml` (`kind: icon`, citing
  `UX.ICONS.*`).
- **Files to add / edit:** SVG file under `design/icons/<name>.svg` or `design/gdt/<name>.svg`,
  entry in `design/icons/index.json` (with `a11y_id` whenever `labelless = true`), provenance
  file `<svg>.prov.json` (`icon-provenance@1`, plus `ATTRIBUTION.md` entry if third-party, per
  `UX.ICONS.PROV`), and an entry in `design/sources.json` if derived from a design-tool source.
- **Originality declaration:** `<svg>.origin.json` beside the SVG or under
  `design/review/origin/`.
- **Review record:** `design/review/records/<pr>.json` verifying `C-ORIGINALITY`, `C-DRESS`
  and `C-NOCOLOR`.
- **Text-editor / SVG-only workflow:** draw the icon on the grid by hand in a text editor or
  locally in Inkscape; register with tool `text-svg` or `inkscape` in `design/sources.json` if
  tracked as a source.

#### 5.3 Component specification

- **Issue template:** `.github/ISSUE_TEMPLATE/design-proposal.yml` (`kind: component`, citing
  `UX.COMP.*`).
- **Files to add / edit:** component spec JSON/Markdown under `design/components/`, any
  reference PNG/SVG export under `design/components/`, and a `design/sources.json` entry
  linking the source and committed exports with SHA-256 digests.
- **Originality declaration:** `<file>.origin.json` beside each spec and export file (or in
  `design/review/origin/`).
- **Review record:** `design/review/records/<pr>.json` verifying `C-KEYBOARD` (focus order
  and keyboard path), `C-NOCOLOR`, `C-CONTRAST` (using token names only, no literal values)
  and `C-DRESS`.
- **Text-editor / SVG-only workflow:** write the component spec in JSON/Markdown and any
  anatomy diagram as a hand-authored SVG; register with tool `text-svg`.

#### 5.4 Interaction or guidance pattern

- **Issue template:** `.github/ISSUE_TEMPLATE/design-proposal.yml` (`kind: pattern`, citing
  `UX.PATTERN.*`).
- **Files to add / edit:** pattern spec Markdown/JSON under `design/patterns/`, optional flow
  diagrams in SVG, and a `design/sources.json` entry if a design-tool file backs the diagrams.
- **Originality declaration:** `<file>.origin.json` (or under `design/review/origin/`).
- **Review record:** `design/review/records/<pr>.json` checking `C-PRINCIPLES` (including
  `P-EXPLAIN` and `P-KEYS`), `C-KEYBOARD`, `C-ORIGINALITY` and `C-DRESS`.
- **Text-editor / SVG-only workflow:** author the pattern text in Markdown/JSON and state
  diagrams as plain SVG in a text editor.

#### 5.5 Screen wireframe or mockup

- **Issue template:** `.github/ISSUE_TEMPLATE/design-proposal.yml` (`kind: screen`, citing
  `UX.SCREENS.*`).
- **Files to add / edit:** screen spec and committed open exports (`.svg`, `.pdf`, `.png`,
  and `.penpot` when drawn in Penpot) under `design/screens/<app>/<screen>/`, plus an entry in
  `design/sources.json` listing the tool, public location and every committed export with its
  SHA-256 digest.
- **Originality declaration:** `<file>.origin.json` (or under `design/review/origin/`) for
  each committed file.
- **Review record:** `design/review/records/<pr>.json` checking every checklist item and
  trade-dress rule, verifying token names (no hard-coded colours or sizes) and keyboard paths.
- **Text-editor / SVG-only workflow:** contributors without Penpot can submit a structured
  Markdown wireframe and a hand-authored SVG wireframe registered with tool `text-svg` (or
  `inkscape`) in `design/sources.json`.

#### 5.6 Report style or reference report mockup

- **Issue template:** `.github/ISSUE_TEMPLATE/design-proposal.yml` (`kind: report style`,
  citing `UX.REPORT.*`).
- **Files to add / edit:** style definitions under `design/report/`, reference mockup exports
  (colour and greyscale `.pdf`/`.png`, plus `.penpot` when drawn in Penpot) under
  `design/report/mockups/<id>/`, and a `design/sources.json` entry with SHA-256 digests.
- **Originality declaration:** `<file>.origin.json` (or under `design/review/origin/`).
- **Review record:** `design/review/records/<pr>.json` verifying `C-NOCOLOR` (greyscale
  legibility), `C-CONTRAST`, `C-ORIGINALITY` and every trade-dress rule in `C-DRESS`.
- **Text-editor / SVG-only workflow:** edit report style JSON/TOML files directly in a text
  editor and commit SVG or locally generated PDF/PNG sheets registered with tool `text-svg`.

## 6. Local checks before opening a pull request

Run the relevant `fid-design` checks from the repository root before opening or updating a PR:

```sh
# Design sources and export digests (UX.SYSTEM.TOOL)
cargo run -q -p fid-design -- sources-check --root design --spec spec

# Originality declarations across the package (UX.SYSTEM.REVIEW)
cargo run -q -p fid-design -- review-check --all --root design

# PR review gate over changed paths relative to design/ (UX.SYSTEM.REVIEW)
cargo run -q -p fid-design -- review-check --changed <changed.txt> --root design

# Colour, contrast, status and palette release gate (UX.TOKENS.CHECK)
cargo run -q -p fid-design -- color-check --root design

# Icon provenance and attribution (UX.ICONS.PROV)
cargo run -q -p fid-design -- icons-prov --root design --attribution ATTRIBUTION.md

# Design principles structure (UX.SYSTEM.PRINC)
cargo run -q -p fid-design -- princ-check
```

Sign off every commit with `git commit -s` (DCO) and follow the repository's pull-request
template (`.github/PULL_REQUEST_TEMPLATE.md`).
