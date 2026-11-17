---
id: QUAL.THREAT.REG
title: Threat model document, register schema and register check
level: 3
parent: QUAL.THREAT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-QUAL.THREAT-001, REQ-QUAL.THREAT-002, REQ-QUAL.THREAT-003, REQ-QUAL.THREAT-006, REQ-QUAL.THREAT-007]
covers: []
depends_on: []
uses_artifacts: [QUAL.VULN.ADVIS]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.THREAT.REG: Threat model document, register schema and register check

## Purpose

Defines the machine-readable threat register and the human threat-model document generated around it, and the CI check that keeps the register honest: every mandatory coverage area has a threat, every threat names its boundary, vector, controls and residual risk, every control cites requirement IDs that exist in the spec tree, every `high` residual risk links an open issue, and the published rendering never discloses exploit detail for unfixed vulnerabilities.

## Scope

**In:**
- `security/threat/register.toml` schema `qual-threat-register/1`: threat ID, coverage area, trust boundary, vector, controls (each with requirement IDs and owning node), residual risk `low|medium|high`, owner, issue link, optional `advisory` and `embargoed` flag
- Closed list of mandatory coverage areas: `mcp`, `control_channel`, `plugins_wasm`, `driver_hosts`, `file_cad_import`, `motion_approval`, `signing_keys`, `build_supply_chain`, `journal_admin_tamper`, `export_controlled_data`
- `security/threat/MODEL.md` preamble (assets, boundaries diagram, method) plus a generated register section
- Register check: coverage, field completeness, requirement-ID resolution against the spec tree, `high` entries have an issue link
- Summary file `threat/register-summary.json` with the count of `high` entries (read by `ECO.REL.PROC.NOTES`)
- Public rendering that omits vector and control detail for `embargoed` entries and links the advisory instead

**Out** (and which node owns it instead):
- Per-minor review records (owned by `QUAL.THREAT.REVIEW`)
- PR-level change triggers (owned by `QUAL.THREAT.TRIGGER`)
- Implementing controls (owned by the nodes cited in each row)
- Advisory content (owned by `QUAL.VULN.ADVIS`)

## Requirements

- **REQ-QUAL.THREAT.REG-001** If any mandatory coverage area has no register entry, then the register check shall fail and name the area.
- **REQ-QUAL.THREAT.REG-002** If an entry lacks a trust boundary, a vector, at least one control with at least one requirement ID, or a residual risk in `low|medium|high`, then the check shall fail and name the entry and field.
- **REQ-QUAL.THREAT.REG-003** If an entry cites a requirement ID that does not appear as a requirement line in the spec tree given to the check, then the check shall fail and name the entry and ID.
- **REQ-QUAL.THREAT.REG-004** If an entry with residual risk `high` has no issue link, then the check shall fail and name the entry.
- **REQ-QUAL.THREAT.REG-005** The tool shall write `threat/register-summary.json` with the register SHA-256, the entry count per residual-risk level and the IDs of `high` entries.
- **REQ-QUAL.THREAT.REG-006** When the public model is rendered, every entry marked `embargoed` shall show only its ID, coverage area and residual risk, and no vector or control text.
- **REQ-QUAL.THREAT.REG-007** If an entry names an advisory ID that is not in the published advisory feed while `embargoed` is false, then the check shall fail, so that unpublished vulnerabilities cannot be described in the public register.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/threat/reg/ok/` (register with one entry per area, a 20-node spec tree, advisory feed), when `cargo xtask qual threat check --register register.toml --spec spec/ --advisories feed.json --out out/` runs, then it exits 0 and `out/threat/register-summary.json` equals the golden file.
- **AC-2** (error path) Given `missing-area/` (no `driver_hosts` entry), then it exits 1 naming `driver_hosts`.
- **AC-3** (error path) Given `bad-req/` (control cites `REQ-CORE.BUS-999`), then it exits 1 naming the entry and the ID.
- **AC-4** (error path) Given `high-no-issue/`, then it exits 1 naming the entry.
- **AC-5** Given `ok/` with one entry `embargoed = true`, when `qual threat render` runs, then `MODEL.md`'s generated section shows that entry's ID, area and risk and contains none of its vector or control strings.
- **AC-6** (error path) Given `leak/` (entry names advisory `FID-2027-0003` absent from the feed, `embargoed = false`), then the check exits 1.
- **AC-7** (determinism) Given AC-1 twice, then the summary files are byte-identical.

## Design

- **Registry command(s):** none; repository tools `cargo xtask qual threat check --register <toml> --spec <dir> --advisories <json> --out <dir>` (W1) and `cargo xtask qual threat render --register <toml> --preamble <md> --out <dir>` (W1).
- **Data structures:** `Entry { id: "TM-NNN", area: Area, boundary, vector: Untrusted<String>, controls: Vec<Control{ text, req_ids, node }>, residual: Low|Medium|High, owner, issue: Option<Url>, advisory: Option<String>, embargoed: bool }`; `Summary { schema: "qual-threat-summary/1", register_sha256, counts, high_ids }`.
- **Algorithm:** parse register; collect requirement IDs from `spec/**/NODE.md` lines matching the requirement pattern; check rules; render by sorting entries by ID.
- **Errors:** `THREAT-PARSE`, `THREAT-AREA(area)`, `THREAT-FIELD(entry, field)`, `THREAT-REQ(entry, id)`, `THREAT-ISSUE(entry)`, `THREAT-LEAK(entry)`. Exit 0/1/2.
- **Determinism:** D0.
- **Performance budget:** under 3 s with a 2,000-node spec tree.
- **Crate:** `fid-xtask` (module `qual::threat`).

## Test fixtures

`fixtures/qual/threat/reg/{ok,missing-area,bad-req,high-no-issue,leak}/` with a small synthetic spec tree and register; CC-BY-4.0. The first real register is written from the concept doc's threat table after re-checking it against the current spec.

## Build prompt

> In `fid-xtask`, add module `qual::threat` with `qual threat check --register <toml> --spec <dir> --advisories <json> --out <dir>` and `qual threat render --register <toml> --preamble <md> --out <dir>`. Register schema `qual-threat-register/1`: entries with id, area (closed list: mcp, control_channel, plugins_wasm, driver_hosts, file_cad_import, motion_approval, signing_keys, build_supply_chain, journal_admin_tamper, export_controlled_data), boundary, vector, controls (text, requirement IDs, node), residual low|medium|high, owner, issue, advisory, embargoed. Check: each area covered; required fields present; every requirement ID exists as a `- **REQ-...**` line under the spec dir; high entries have an issue; an advisory reference not in the published feed requires `embargoed = true`. Write `threat/register-summary.json` (canonical JSON: register SHA-256, counts per level, high IDs). Render the register into MODEL.md, hiding vector and controls of embargoed entries. Treat all text fields as untrusted; no panics, deny unwrap/expect, BTreeMap only. Tests: AC-1..AC-7 of spec node QUAL.THREAT.REG.

## Open questions

- The concept doc's threat table (01 §10.2) needs re-checking against the current spec before import [unverified currency].
- `ECO.REL.PROC.NOTES` should read `threat/register-summary.json` for the count of `high` entries (cross-domain; it names the register, not this file).

## Children

_Leaf: none._
