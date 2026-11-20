---
id: OUTR.GDTC.GUARD
title: Guardrails: standards text, overdue decisions, support routing
level: 3
parent: OUTR.GDTC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.GDTC-002, REQ-OUTR.GDTC-009, REQ-OUTR.GDTC-010]
covers: []
depends_on: [OUTR.BRAND.LINT]
uses_artifacts: [GDT.RES.STATUS, OUTR.COMM.CHAN]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# OUTR.GDTC.GUARD: Guardrails: standards text, overdue decisions, support routing

## Purpose

Three rules keep the GD&T community safe to run in public. **No standards text anywhere:** ASME and ISO texts are copyrighted, so community pages, cases, profiles and suites cite designators, editions and clause numbers only (CONSTITUTION §5). **Every rule is on a decision clock:** each rule has a decision owner and a deadline, and a missed deadline escalates under the D-003 process instead of drifting. **Customer parts go to paid support:** the public forum discusses generated cases and rules, not one customer's drawing. This leaf turns the three rules into checks and texts that the other `OUTR.GDTC` leaves and the community channels use.

## Scope

**In:**
- Standards-text lint `gdt-text-lint` over every GDTC input and output (Markdown, TOML, JSON strings), used in-process by the page generators and in CI
- Overdue report: rules past their deadline, escalation level and the D-003 action due
- Support routing: the GD&T discussion template, the routing reply text and the routing record
- A product-name rule for GDTC outputs backing REQ-OUTR.GDTC-008

**Out:**
- The owner and deadline fields themselves (owned by `OUTR.GDTC.RULES`)
- Posting comments or opening issues (owned by `ECO.CI.WF`, a thin workflow over this leaf's report)
- The RFC timer (owned by `ECO.COMM.RFC`)
- The paid support offering and partner listing (outside this tree; partners in `ECO.COMM.PARTNER`)
- Abuse moderation and its log (owned by `OUTR.COMM.MOD`)

## Requirements

- **REQ-OUTR.GDTC.GUARD-001** (GS001) The lint shall report an error for every quoted span of any length (text in quotation marks, or a Markdown or HTML block quote) that appears in the same paragraph or record as a standards designator (for example a pattern for `ASME Y14.5`, `ISO 1101` or `ISO 5459`).
- **REQ-OUTR.GDTC.GUARD-002** (GS002) The lint shall report an error for every free-text field of a GDTC data file longer than 280 characters.
- **REQ-OUTR.GDTC.GUARD-003** (GS003) The lint shall report an error for every sentence in a GDTC output that names a product from the `OUTR.BRAND.LINT` incumbent-name list, with no exception for benchmark links.
- **REQ-OUTR.GDTC.GUARD-004** The lint shall accept no suppression marker for GS001 or GS003.
- **REQ-OUTR.GDTC.GUARD-005** [escalation step pending a D-003 RFC] For an `--as-of` date, the overdue report shall list every rule whose governance deadline has passed and whose status register entry has no revision or status change with `revised_at` or approval time after the governance `deadline_set` date, as `due` up to 7 days past the deadline and `escalated` after that, with the D-003 action due: for `due`, the owner decides or extends the deadline once with a written reason; for `escalated`, a maintainer other than the owner opens an RFC to decide. The 7-day step and both actions are a proposal until confirmed by a D-003 RFC.
- **REQ-OUTR.GDTC.GUARD-006** [pending the same D-003 RFC] A deadline extension shall count only when the governance register holds a dated note on that rule beginning `extended:` with a reason, and a second extension of the same deadline shall be reported as `escalated`.
- **REQ-OUTR.GDTC.GUARD-007** The GD&T discussion template shall require the poster to confirm that the question is not about a specific customer part and shall link the paid support page, and the template check shall fail if either is missing.
- **REQ-OUTR.GDTC.GUARD-008** When a post or a case is routed as `customer_part`, the reply shall be the routing text (which links paid support and invites a re-submission as a generated case), and a routing record shall store only the date, the channel ID and the post or case ID.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/gdtc/guard/quote_fail.md` (a 20-word quoted passage in a paragraph citing an invented designator "ISO 99999"), when `fid-outreach gdt-text-lint fixtures/outr/gdtc/guard/quote_fail.md --json` runs, then it exits 1 with one GS001 finding; given `quote_short.md` (the same paragraph with a 2-word quote), then it also exits 1 with GS001; given `quote_pass.md` (the same paragraph with no quotation, and a quote in a paragraph with no designator), then it exits 0.
- **AC-2** (error path) Given `long_field.toml` (a 300-character note), then GS002.
- **AC-3** (error path) Given `product.md` (a sentence naming a listed incumbent product next to a benchmark link), then GS003; with a GS001 or GS003 suppression marker, the finding remains and the marker is reported as not allowed.
- **AC-4** Given `governance.toml` with deadlines 2027-01-10, 2027-01-01 and 2027-02-01 (each `deadline_set` 2026-12-01, no status change since) and `--as-of 2027-01-12`, when `fid-outreach gdt-overdue --governance governance.toml --status status.json --as-of 2027-01-12 --json` runs, then the first is `due` (2 days), the second `escalated` (11 days) and the third is not listed.
- **AC-5** Given `extended_twice.toml` (two `extended:` notes on one deadline), then that rule is `escalated`.
- **AC-6** (error path) Given `template_no_box.md` and `template_no_link.md`, when `fid-outreach gdt-template-check` runs on each, then it exits 1 naming the missing element.
- **AC-7** Given a routing of case `C-0042` as `customer_part`, then the routing record has exactly the fields date, channel ID and case ID, and the reply equals the routing text.
- **AC-8** (determinism) Running AC-4 twice gives byte-identical output.

## Design

- **Commands:** `fid-outreach gdt-text-lint <paths...>`, `fid-outreach gdt-overdue --governance <file> --status <file> --as-of <date>`, `fid-outreach gdt-template-check <file>`, `fid-outreach gdt-route --kind <post|case> --id <id> --channel <id> --date <date>`; all `--json`. Project tooling; nearest class W1 (`gdt-route` appends to the routing record file; the others only read).
- **Lint:** sentence and paragraph extraction from the `fid-vocab` engine (as `OUTR.BRAND.LINT`); designator patterns in a rule file `gdt-text-rules.toml` (organisation prefixes `ASME`, `ISO`, `VDA`, `DIN`, `BS`, `JIS` followed by a document number); word count on whitespace-separated tokens; free-text fields declared per data format.
- **Texts (written in the project's own words):** `community/gdt/discussion-template.md`, `community/gdt/routing-reply.md`.
- **Routing record:** `community/gdt/routing.csv` with columns `date, channel, id`; no names, no text of the post.
- **Errors:** findings are data; exit 0 clean, 1 findings, 2 unreadable input.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `gdtc::guard`.

## Test fixtures

Hand-written Markdown, TOML and template files under `fixtures/outr/gdtc/guard/` (invented designators and lorem-ipsum quotations only, never real standards text; CC-BY-4.0).

## Build prompt

> In crate `fid-outreach`, add module `gdtc::guard` with subcommands `gdt-text-lint <paths...>`, `gdt-overdue --governance <file> --status <file> --as-of <date>`, `gdt-template-check <file>` and `gdt-route --kind <post|case> --id <id> --channel <id> --date <date>` (all `--json`; exit 0, 1, 2). Lint rules over Markdown, TOML and JSON strings: GS001 a quoted span of any length (quotation marks or block quote) in the same paragraph or record as a standards designator (patterns from `gdt-text-rules.toml`); GS002 a declared free-text field over 280 characters; GS003 a sentence naming a product from the `OUTR.BRAND.LINT` incumbent-name list, with no benchmark-link exception; no suppression allowed for GS001 or GS003. Use the `fid-vocab` engine for sentence extraction. `gdt-overdue` lists rules past their governance deadline with no revision or status change in the status register after the governance `deadline_set` date, `due` up to 7 days and `escalated` after, with the D-003 action due; an extension counts only with a dated `extended:` note, and a second extension of the same deadline is `escalated`. `gdt-template-check` fails unless the template requires the not-a-customer-part confirmation and links the paid support page. `gdt-route` appends `date, channel, id` to `community/gdt/routing.csv` and prints the routing reply text. Write `community/gdt/discussion-template.md` and `community/gdt/routing-reply.md` in your own words. Tests per AC-1..AC-8 with fixtures under `fixtures/outr/gdtc/guard/` using invented designators and lorem ipsum only. Constraints: no panics, no `unsafe`; deny `unwrap`/`expect`/`panic`; no clock reads; BTreeMap only; never include or send standards text.

## Open questions

- GS001 catches quotation, not paraphrase that follows a standard too closely; review by a licence holder (D-016) remains the backstop.
- The escalation step of REQ-005 and REQ-006 (7 days, then a maintainer other than the owner) is pending a D-003 RFC; until it is confirmed the report is informational and opens no RFC by itself.
- GS001 also flags harmless quotations next to a designator (for example a quoted term from the project's own glossary); such text is reworded, never suppressed.

## Children

_Leaf: none._
