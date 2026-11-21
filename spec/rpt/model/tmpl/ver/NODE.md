---
id: RPT.MODEL.TMPL.VER
title: Template versions, immutability and template commands
level: 4
parent: RPT.MODEL.TMPL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-report]
built_pr: 391
satisfies: [REQ-RPT.MODEL.TMPL-005, REQ-RPT.MODEL.TMPL-006]
covers: [GOM.RPT.02, PCD.RPT.01, PW.RPT.02, SA.RPT.01]
depends_on: [RPT.MODEL.TMPL.FORMAT, RPT.MODEL.TMPL.SLOT, RPT.MODEL.DOC, CORE.STO.CAS, CORE.JRN]
side_effect: [W2, W2, R0, R0]
determinism: D0
cli: [fid report save-template, fid report apply-template, fid report show-template, fid report list-template]
mcp: [report_save_template, report_apply_template, report_show_template, report_list_template]
journal: [report.save_template@1, report.apply_template@1, report.show_template@1, report.list_template@1]
updated: 2026-11-19
---

# RPT.MODEL.TMPL.VER: Template versions, immutability and template commands

## Purpose

Saving a report as a template, versioning it, and creating reports from it. Each save of a template ID creates the next version, stored immutably in the content-addressed store and addressed by its digest; a report created from a template records the ID, version and digest it came from. Locked templates (the fixed forms) can be applied but never saved over.

## Scope

**In:** `report.save_template@1` (from a report, or from a template file through an import-root handle), `report.apply_template@1`, `report.show_template@1`, `report.list_template@1`; version numbering per template ID; immutability; `TemplateRef { id, version, digest }`; refusal of locked templates; registration API for built-in templates (starters and forms).

**Out:** file format and parser (`RPT.MODEL.TMPL.FORMAT`); slot validation (`RPT.MODEL.TMPL.SLOT`); generation (`RPT.MODEL.BIND.GEN`); a user template library shared across projects (see Open questions).

## Requirements

- **REQ-RPT.MODEL.TMPL.VER-001** When `report.save_template@1` runs for a template ID, the system shall validate the template, assign version n+1 where n is the highest existing version of that ID (1 for a new ID), store the canonical bytes in the content-addressed store and journal one event with ID, version and digest.
- **REQ-RPT.MODEL.TMPL.VER-002** No command shall change or delete the stored bytes of a saved template version.
- **REQ-RPT.MODEL.TMPL.VER-003** If the template ID belongs to a built-in template with `locked = true`, then `report.save_template@1` shall fail with error `report.template_locked` and journal nothing.
- **REQ-RPT.MODEL.TMPL.VER-004** When `report.apply_template@1` creates a report from a template, the report shall record the template ID, version and digest, and its blocks shall carry the template's slots.
- **REQ-RPT.MODEL.TMPL.VER-005** If saving would store bytes identical to the latest version of that ID, then the command shall return that version without creating a new one and journal nothing.
- **REQ-RPT.MODEL.TMPL.VER-006** `report.list_template@1` shall list built-in and project templates sorted by ID then version, marking locked ones; `report.show_template@1` shall return one version's canonical document.
- **REQ-RPT.MODEL.TMPL.VER-007** Agents and scripts shall pass template files only as import-root handles, and a raw path from them shall fail with error `raw_path_forbidden`.
- **REQ-RPT.MODEL.TMPL.VER-008** When `report.save_template@1` or `report.apply_template@1` runs, the system shall check every block of the template against the block-kind registry (registered kind and kind version, else `report.unknown_block_kind`), validate its payload with that kind (else `report.invalid_block_payload`) and apply the value-field rule of `RPT.MODEL.DOC` (else `report.value_literal_refused` or `report.value_binding_invalid`), and on any failure shall journal nothing; the template file format (`RPT.MODEL.TMPL.FORMAT`) checks kind IDs for shape only and relies on these checks.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/bracket_3stage` with a report R, when `fid report save-template --from-report R --template acme-char --json` runs twice with an edit in between, then versions 1 and 2 exist with different digests and two events are journaled; a third save without edits returns version 2 and journals nothing.
- **AC-2** Given AC-1, when `fid report apply-template --template acme-char@1` runs, then the new report records `acme-char`, version 1 and its digest.
- **AC-3** (error path) Given built-in `tmpl:fai-form3`, when saved over, then it fails with `report.template_locked`; when applied, it succeeds.
- **AC-4** Given AC-1, when `fid report list-template --json` runs, then built-ins and `acme-char` versions appear sorted with locked flags; `show-template acme-char@1` returns bytes hashing to version 1's digest.
- **AC-5** (error path) Given an agent principal and a raw path to a template file, then it fails with `raw_path_forbidden`; an invalid file through a handle fails with `report.template_invalid` and journals nothing.
- **AC-6** (replay) Given AC-1's journal, when replayed in exact mode, then the outcome is `IDENTICAL` with the same digests.

## Design

- **Registry commands:**
  - `report.save_template@1(template: TemplateId, source: ReportId | ImportHandle) -> TemplateRef` (W2)
  - `report.apply_template@1(template: TemplateId, version: Option<u32>, title: Text) -> ReportSummary` (W2)
  - `report.show_template@1(template: TemplateId, version: Option<u32>) -> Template` (R0)
  - `report.list_template@1() -> Vec<TemplateInfo>` (R0)
- **Data structures:** `TemplateRef { id, version: u32, digest: Sha256 }`; a project index `BTreeMap<TemplateId, Vec<TemplateRef>>`; `BuiltinRegistry::register(Template)` for crates that ship templates.
- **Errors:** `report.template_locked`, `report.template_invalid`, `report.not_found`, `raw_path_forbidden`.
- **Determinism:** D0.
- **Performance budget:** save a 1 MiB template in under 50 ms [unverified until measured].
- **Crate:** `fid-report` (module `tmpl::ver`).

## Test fixtures

`fixtures/rpt/model/bracket_3stage` (shared) and the format fixtures of `RPT.MODEL.TMPL.FORMAT`.

## Build prompt

> In crate `fid-report`, add module `tmpl::ver` with commands `report.save_template@1`, `report.apply_template@1` (W2) and `report.show_template@1`, `report.list_template@1` (R0); CLI `fid report save-template | apply-template | show-template | list-template --json`. Saving validates (format and slot leaves), assigns version n+1 per template ID, stores canonical bytes in the content-addressed store and journals ID, version, digest; identical bytes return the latest version without a new event; locked built-ins -> `report.template_locked`; stored versions are immutable. Applying records the template ref in the new report. Template files come only through import-root handles for agents and scripts (`raw_path_forbidden`). Provide a built-in registry for crates that ship templates. Acceptance: AC-1..AC-6 in `spec/rpt/model/tmpl/ver/NODE.md`. Constraints: CONSTITUTION C6, §3, §5, §8.

## Open questions

- A user template library shared across projects (stored by digest in user settings) is assumed wanted but not specified; it needs a settings scope from `CORE.STO.SET` and is left for a later leaf.
- **Build notes (fid-report `tmpl::ver`, 2026-11-21).** Template IDs are the template document's `template_id` (UUIDv7, `RPT.MODEL.TMPL.FORMAT`); `acme-char` and `tmpl:fai-form3` in the ACs are illustrative names. Built-in and project versions are separate number spaces: `builtin:<id>@<version>` (version given explicitly by the shipping crate to `BuiltinRegistry::register`) and `<id>@<version>` (project versions 1, 2, 3, ... folded from the journal); REQ-001's n is the highest *project* version, so a release that adds or revises a built-in never collides with a project save. The CLI selector is `[builtin:]<uuid>[@<version>]`. `RPT.MODEL.DOC`'s stored `template_ref` was widened from `{id, version}` to `{builtin, digest, id, version}` (REQ-004 needs the digest; no stored report held a non-null `template_ref` before); the DOC node's Design text still says `{id, version}` and needs a follow-up in that lane. Saves commit under a compare-and-set on the ID's latest project version (`conflict/head_moved`), so two concurrent saves cannot both become n+1. Save events carry their source (report and its digest, or the file handle) so exact replay can rebuild saves from reports.
- **D-045 (labels).** The guarantee is architectural: every label, the provisional one included, is produced by `RPT.MODEL.FMT` from the stored record a value binding names, and a template holds no value, value text or number (`RPT.MODEL.TMPL.FORMAT`), so no template content reaches the label code and a template cannot hide a label. As defence in depth, VER refuses a payload key that, after casefolding and dropping `_`, `-` and spaces, is exactly one of `labels`, `hidelabels`, `showlabels`, `suppresslabels`, `labelmode`, `valuelabels`, `labelvisibility`, `interpretation`, `provisional`, and the provisional label's key or text (`report.template_invalid`, reason `label_control`). The match is exact so specced keys such as `x_label`, `y_label` (`RPT.MODEL.BLOCKS.CHART`) and `show_label` (`RPT.FORMS.LAYOUT.MAP`) stay allowed.
- **REQ-004 partly met.** The applied report records template ID, version and digest (AC-2), and its blocks keep the template's block IDs; the slots stay in the recorded template version rather than in the report. Open question for `RPT.MODEL.BIND.GEN`: resolve a report's slots from the template version named by `template_ref.digest` (read from the content-addressed store), so REQ-004's "blocks carry the template's slots" is met end to end.
- **Known limit: built-in provenance on replay.** So that a release can drop a built-in version and old reports still replay, an apply event naming `builtin:<id>@<v>` that the replaying release no longer ships is replayed from the stored bytes after the digest check. A forged journal can therefore relabel a project version as such a built-in (with a matching report digest) and replay `IDENTICAL`; it changes only the provenance label and grants no authority. Oracle case `apply_relabelled_unshipped_builtin` pins this. Closing it needs a signed or journaled record of the built-ins each release shipped.
- **Deferred: project-level locking.** Only built-ins can be locked (REQ-003). A project template file with `locked: true` is refused (`report.template_invalid`, reason `project_template_locked`), because a lock folded from journal events is authority-bearing state with no origin binding. Any reintroduction must follow REQ-CORE.RPL.RECON.FOLD-008 (`spec/core/rpl/recon/fold/NODE.md`, "Authority-bearing folds"): bind each lock event to its verified principal, command and branch, reserve its effects in `fid-registry` `RESERVED_EFFECTS`, and ship forged-event oracle cases.

## Children

_Leaf: none._
