---
id: GDT.RES.STATUS
title: Interpretation status register and sign-off binding
level: 3
parent: GDT.RES
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.RES-010, REQ-GDT.RES-011]
covers: []
depends_on: [GDT.RES.ROWFLOW.STATE, GDT.RES.ROWFLOW.ROSTER, GDT.RES.SCHEMA, CORE.JRN]
side_effect: W1
determinism: D0
cli: [fid gdt export-rule-status]
mcp: [gdt_export_rule_status]
journal: [gdt.export_rule_status@1]
updated: 2026-11-19
---

# GDT.RES.STATUS: Interpretation status register and sign-off binding

## Purpose

D-045 says a result that used any interpretation rule without expert sign-off is `interpretation: provisional`. This leaf decides, from the `GDT.RES.ROWFLOW` records alone, which rules are signed off: it builds the **interpretation status register**, one entry per revision of every DMT row key and every defaults profile, with its content digest, its status (`draft`, `provisional` or `signed_off`) and the journaled sign-offs that justify it. It also offers the one lookup that every result builder (`GDT.CHAR.INTERP`) and every public rule page (`OUTR.GDTC.RULES`) uses, so the status shown to users and the status stamped into results cannot disagree.

## Scope

**In:**
- Register format `gdt-rule-status/v1` and its digest (SHA-256 over RFC 8785 canonical JSON)
- Status derivation per revision from ROWFLOW revisions and `gdt.review_dmt_row@1` events
- Binding of a sign-off to the content digest it names; loss of sign-off on any later revision
- Profiles as rules: a defaults profile is `signed_off` only through the row-like sign-off key that `GDT.RES.ROWASME.DPROF` proposes
- `status_of(register, rule, content_digest)` lookup with reasons
- `gdt.export_rule_status@1` (W1, write to an approved local root)

**Out:**
- The review command and its stale-digest check (owned by `GDT.RES.ROWFLOW.REVIEW`)
- Stamping statuses into results and refusing `draft` rules (owned by `GDT.CHAR.INTERP`)
- Rendering rule pages, owners and deadlines (owned by `OUTR.GDTC.RULES`)
- Who may review (owned by `GDT.RES.ROWFLOW.ROSTER` and `OUTR.GDTC.REVIEW`)

## Requirements

- **REQ-GDT.RES.STATUS-001** The register shall hold one entry per (rule ID, revision) for every DMT row key and every published defaults profile, each with kind (`dmt_row` or `profile`), content digest, author kind, status, and the reviewer IDs and journal entry hashes of the approvals that count towards sign-off, sorted by (rule ID, revision).
- **REQ-GDT.RES.STATUS-002** A revision shall have status `signed_off` only when it is the latest revision of its rule and it holds approvals from two distinct roster reviewers whose `gdt.review_dmt_row@1` events name that revision's content digest and no later `request_changes`.
- **REQ-GDT.RES.STATUS-003** A latest revision authored by an agent with no human re-proposal shall have status `draft`; every other revision that is not `signed_off` shall have status `provisional`.
- **REQ-GDT.RES.STATUS-004** When a rule gets a new revision, every earlier revision of that rule shall have status `provisional` with reason `superseded`, whatever sign-off it held.
- **REQ-GDT.RES.STATUS-005** A defaults profile shall have status `signed_off` only when a signed-off revision of its sign-off key names the profile's own digest; until that key exists every profile shall be `provisional` with reason `no_signoff_path`.
- **REQ-GDT.RES.STATUS-006** `status_of(register, rule, content_digest)` shall return the entry's status and reason, and if the register holds no entry for that rule and digest, then it shall return `provisional` with reason `not_in_register`; it shall never return `signed_off` without a matching entry.
- **REQ-GDT.RES.STATUS-007** `gdt.export_rule_status@1` shall write the register and its digest to an approved local export root, journal one event with the register digest and the journal head it was built from, and produce byte-identical output for the same inputs.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/status/happy/` (one human-authored row revision with two approvals naming its digest), when `fid gdt export-rule-status --out <handle> --json` runs, then the entry is `signed_off` with both reviewer IDs and journal hashes, and one event carries the register digest.
- **AC-2** Given `fixtures/gdt/status/one_approval/`, then the entry is `provisional`.
- **AC-3** Given `fixtures/gdt/status/agent_draft/` (agent-authored latest revision, no human re-proposal), then the entry is `draft`; given `agent_rewritten/` (a human re-proposal with `derived_from`), then it is `provisional`.
- **AC-4** Given `fixtures/gdt/status/changed/` (revision 1 signed off, revision 2 with a different content digest and no reviews), then revision 2 is `provisional` and revision 1 is `provisional` with reason `superseded`.
- **AC-5** (error path) Given `fixtures/gdt/status/wrong_digest/` (two approvals whose events name a digest other than the revision's), then the entry is `provisional`, not `signed_off`.
- **AC-6** Given a profile with no sign-off key in the fixture set, then its entry is `provisional` with reason `no_signoff_path`; given `profile_signed/`, then `signed_off`.
- **AC-7** Given the AC-1 register, when `status_of` is called with an unknown rule and with a known rule at a digest not listed, then both return `provisional` with reason `not_in_register`.
- **AC-8** (determinism and replay) Running AC-1 twice gives byte-identical files; exact replay of its journal gives `IDENTICAL`.

## Design

- **Registry command(s):** `gdt.export_rule_status@1`, W1, `fs_effect: write_root` (CONSTITUTION §3.1); params `out: ExportHandle`, optional `at: JournalHash` (default: head); result `{ register_digest, entries, path }`. CLI `fid gdt export-rule-status`, MCP `gdt_export_rule_status`, `--json`. Agents may run it: it publishes facts and changes no status.
- **Data structures:** `RuleStatusRegister { schema: "gdt-rule-status/v1", built_from: { journal_head, roster_digest }, entries: Vec<RuleStatusEntry> }`; `RuleStatusEntry { rule: RuleId, kind: DmtRow | Profile, revision: u32, content_digest: Sha256, author_kind, status: Draft | Provisional | SignedOff, reason: Option<Reason>, approvals: Vec<{ reviewer: ReviewerId, journal_entry: Sha256 }> }`; `Reason { superseded, not_in_register, no_signoff_path, awaiting_review }`; `StatusAnswer { status, reason, entry: Option<&RuleStatusEntry> }`. For a DMT row, `RuleId` is the DMT key (which carries its major version) and `revision` is the ROWFLOW revision; for a profile, `RuleId` is the profile ID with its semantic version.
- **Algorithm:** fold ROWFLOW revisions and review events in journal order; per rule keep revisions in order; apply STATE's rules; count an approval only if its event names the revision's digest and the reviewer had the role at the event time (ROSTER); mark all but the latest revision `superseded`; sort; canonicalise.
- **Errors:** `export_root_denied`, `journal_unreadable`, `roster_unreadable`; findings are data.
- **Determinism:** D0; no clock, sorted maps only.
- **Performance budget:** 5 000 rules with 20 revisions each in < 500 ms [unverified].
- **Crate:** `fid-gdt` (module `dmt_flow::status`).

## Test fixtures

Synthetic revisions, review events and a synthetic roster under `fixtures/gdt/status/` (invented keys and clause numbers, no standards text; CC0), hand-labelled from the rules above.

## Build prompt

> In crate `fid-gdt`, add module `dmt_flow::status`. Build an interpretation status register `gdt-rule-status/v1` from DMT row revisions and `gdt.review_dmt_row@1` events (from `dmt_flow::state` and the journal) and from published defaults profiles. One entry per (rule ID, revision), sorted, with kind (`dmt_row` | `profile`), content digest, author kind, status (`draft` | `provisional` | `signed_off`), optional reason (`superseded`, `not_in_register`, `no_signoff_path`, `awaiting_review`) and the counted approvals (reviewer ID, journal entry hash). Rules: `signed_off` only for the latest revision with two distinct roster reviewers whose events name that revision's content digest and no later `request_changes`; latest agent-authored revision without human re-proposal is `draft`; all else `provisional`; earlier revisions are `provisional` with `superseded`; a profile is `signed_off` only through a signed-off sign-off key naming its digest, otherwise `provisional` with `no_signoff_path`. Provide `status_of(register, rule, digest) -> StatusAnswer` returning `provisional` with `not_in_register` when no entry matches, never `signed_off`. Register command `gdt.export_rule_status@1` (W1, write_root; CLI `fid gdt export-rule-status`, MCP `gdt_export_rule_status`, `--json`), one journal event with the register digest and journal head; output byte-identical for the same inputs. Tests per AC-1..AC-8 with synthetic fixtures under `fixtures/gdt/status/`. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`, never store or emit standards text.

## Open questions

- The profile sign-off key depends on how `GDT.RES.ROWASME.DPROF` settles profile review; REQ-005 fails closed (`no_signoff_path`) until then.
- Whether to keep `superseded` sign-offs visible on rule pages as history is for `OUTR.GDTC.RULES`; the register keeps them as data either way.

## Children

_Leaf: none._
