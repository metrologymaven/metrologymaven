---
id: OUTR.SITE.STATUS.ROAD
title: Roadmap page from spec-tree phases and the milestone-date check
level: 4
parent: OUTR.SITE.STATUS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE.STATUS-003, REQ-OUTR.SITE.STATUS-004]
covers: []
depends_on: [OUTR.SITE.GEN.BUILD, OUTR.SITE.GEN.IMPORT]
uses_artifacts: [ECO.COMM.SCOPE, OUTR.LAUNCH.PLAN]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.SITE.STATUS.ROAD: Roadmap page from spec-tree phases and the milestone-date check

## Purpose

Renders the roadmap page from the spec tree's generated index: for each phase from P0 to later, the level-1 and level-2 nodes tagged with it and their current spec status. The only dates the page may show are milestone dates recorded in the OUTR.LAUNCH launch plan; a check scans the page source and the rendered page body and fails the build on any other date, so the roadmap can never promise a date the launch plan does not hold.

## Scope

**In:**
- The `roadmap` renderer: `roadmap/index.html` from hand-written intro `pages/roadmap.md`, the staged spec-tree `INDEX.md` and the staged launch-plan milestone file
- Parsing the index lines of `INDEX.md` (ID, title, phase, size, status mark, level by indent)
- A milestone section listing app, milestone and date from the launch plan
- The date check over the roadmap source and rendered main content

**Out:**
- Generating `INDEX.md` (`ECO.COMM.SCOPE`, `tools/scope.py`)
- Defining milestones, their criteria and dates (`OUTR.LAUNCH.PLAN`)
- The status and parity pages (`OUTR.SITE.STATUS.PARITY`)

## Requirements

- **REQ-OUTR.SITE.STATUS.ROAD-001** The roadmap page shall have one section per phase in the order P0, P1a, P1b, P2, P3, P4, later, each listing every level-1 and level-2 node from the staged `INDEX.md` whose phase is that phase, with its ID, title and spec status (open, split, leaf, decision or frozen).
- **REQ-OUTR.SITE.STATUS.ROAD-002** Each phase section shall show the count of listed nodes per spec status, and the page shall name the spec-tree source revision of `INDEX.md` from `build-manifest.json`.
- **REQ-OUTR.SITE.STATUS.ROAD-003** If an `INDEX.md` tree line does not match the expected line grammar, or names a phase outside CONSTITUTION §7, then the build shall fail and name the line number.
- **REQ-OUTR.SITE.STATUS.ROAD-004** The roadmap page shall list every milestone that has a date in the staged launch-plan milestone file, with its app, milestone name and date, sorted by date then app.
- **REQ-OUTR.SITE.STATUS.ROAD-005** If `pages/roadmap.md` or the rendered `<main>` content of the roadmap page (excluding node titles copied from `INDEX.md`, which may name standard editions) contains a date expression (ISO date, year-month, month-name with year, quarter with year, or half-year with year) whose day, month, quarter or half-year does not correspond to a milestone date in the launch-plan file, then the build shall fail and name the expression and its location.
- **REQ-OUTR.SITE.STATUS.ROAD-006** While no launch-plan milestone file is staged, the roadmap page shall show no milestone section and the date check shall allow no date at all.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/site/status/road/ok` (an `INDEX.md` excerpt with 6 level-1 and 14 level-2 nodes across P0, P1a, P2 and later, and a milestone file with two dated milestones), when `fid-outreach site build` runs, then it exits 0 and `roadmap/index.html` equals the golden page: seven phase sections in order, empty phases showing "No nodes", per-status counts and two milestone rows.
- **AC-2** (bad line) Given fixture `bad-line` (an index line with phase `P9`), when the build runs, then it exits 1 with `ROAD-LINE 17`.
- **AC-3** (stray date) Given fixture `stray-date` (`pages/roadmap.md` line 3 says "Probe ships in Q3 2027" and no milestone falls in that quarter), when the build runs, then it exits 1 with `ROAD-DATE "Q3 2027" pages/roadmap.md:3`.
- **AC-4** (allowed date) Given fixture `allowed-date` (the source mentions "2027-03-31", which is a milestone date), when the build runs, then it exits 0.
- **AC-5** (no plan) Given fixture `no-plan` (no milestone file, source contains "2027"), when the build runs, then it exits 1 with `ROAD-DATE "2027"`; with that word removed, it exits 0 and the page has no milestone section.
- **AC-6** (determinism) Building `ok` twice gives byte-identical roadmap pages.

## Design

- **Command:** none of its own; the renderer `roadmap` runs inside `fid-outreach site build` and registers itself in `site::renderers::all()`. Nearest class W1.
- **Index grammar (from `tools/scope.py`, owned by ECO.COMM.SCOPE):** tree lines `^(  )*- \[(.)\] \[([A-Z0-9.]+)\]\([^)]*\) (.*), (P0|P1a|P1b|P2|P3|P4|later), (S|M|L|XL|XXL)$`; level = indent / 2; marks: space open, `+` split, `x` leaf, `?` decision, `=` frozen. Titles may contain commas, so phase and size are taken from the right. Lines outside the "Tree" section are ignored.
- **Milestone-file contract (assumed, owned by OUTR.LAUNCH.PLAN):** `milestones.toml` with `[[milestone]] id, app, name, date?` (`date` as `YYYY-MM-DD`). Only these fields are read [unverified: OUTR.LAUNCH.PLAN has not fixed its format].
- **Date check:** scanner over the source (outside fenced code) and the text of the rendered `<main>` for these forms: `YYYY-MM-DD`, `YYYY-MM`, English month name or three-letter abbreviation with a four-digit year, `Q[1-4] YYYY`, `H[12] YYYY`, and a bare four-digit year between 2000 and 2099. An expression is allowed when a milestone date falls inside the period it names (a bare year is allowed when any milestone falls in that year). Spec IDs and version tags are excluded by token boundaries. Node titles are rendered inside elements marked `data-src="index"` and skipped by the scan, because titles such as standard designators carry edition years that are not schedule claims.
- **Errors:** `ROAD-LINE(n)`, `ROAD-PHASE(n, value)`, `ROAD-DATE(expr, location)`, `ROAD-PLAN(reason)` (unparsable milestone file).
- **Determinism:** D0.
- **Performance budget:** a 5 000-line `INDEX.md` in under 200 ms [unverified].
- **Crate:** `fid-outreach`, module `site::status::road`.

## Test fixtures

Synthetic fixtures under `fixtures/outr/site/status/road/` (`ok`, `bad-line`, `stray-date`, `allowed-date`, `no-plan`), with `INDEX.md` excerpts in the format `tools/scope.py` writes today and a small `milestones.toml`; CC-BY-4.0.

## Build prompt

> In the `fid-outreach` Rust crate (outreach tooling), add the renderer `roadmap` (module `site::status::road`) implementing the existing `Renderer` trait and register it in `site::renderers::all()`.
>
> Parse the staged spec-tree `INDEX.md`: only the lines of its "Tree" section, each matching `^(  )*- \[(.)\] \[([A-Z0-9.]+)\]\([^)]*\) (.*), (P0|P1a|P1b|P2|P3|P4|later), (S|M|L|XL|XXL)$` (level = indent/2; marks ' ' open, '+' split, 'x' leaf, '?' decision, '=' frozen); fail ROAD-LINE with the line number otherwise. Render `roadmap/index.html`: intro from `pages/roadmap.md`, then one section per phase in order P0, P1a, P1b, P2, P3, P4, later listing level-1 and level-2 nodes (ID, title, status) with per-status counts ("No nodes" when empty), the INDEX.md source revision from `build-manifest.json`, and a milestone section from staged `milestones.toml` (`[[milestone]] id, app, name, date?`) listing dated milestones sorted by date then app.
>
> Date check: scan `pages/roadmap.md` (outside code) and the rendered `<main>` text (skipping node titles, which you render inside elements with `data-src="index"`) for YYYY-MM-DD, YYYY-MM, month name or abbreviation with year, Q1-Q4 YYYY, H1-H2 YYYY and bare years 2000-2099; allow an expression only if a milestone date falls in the period it names; otherwise fail ROAD-DATE naming the expression and file:line. With no milestone file, allow no date.
>
> Tests: AC-1..AC-6 of spec node OUTR.SITE.STATUS.ROAD with golden output. Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded line length; deterministic output.

## Open questions

- OUTR.LAUNCH.PLAN has not fixed the machine-readable milestone format; the `milestones.toml` fields above are an assumed contract to confirm when that node is expanded.
- `INDEX.md` is a Markdown report, not a data format. A JSON index from `tools/scope.py` (ECO.COMM.SCOPE) would be sturdier; until then the line grammar above is the contract.

## Children

_Leaf: none._
