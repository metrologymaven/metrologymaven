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
built_in: [fid-gdt]
built_pr: 326
satisfies: [REQ-GDT.RES-010, REQ-GDT.RES-011]
covers: []
depends_on: [GDT.RES.ROWFLOW.STATE, GDT.RES.ROWFLOW.ROSTER, GDT.RES.SCHEMA, CORE.JRN]
uses_artifacts: [OUTR.GDTC.REVIEW, OUTR.GDTC.PROFILE]
side_effect: W1
determinism: D0
cli: [fid gdt export-rule-status]
mcp: [gdt_export_rule_status]
journal: [gdt.export_rule_status@1]
updated: 2026-11-20
---

# GDT.RES.STATUS: Interpretation status register and sign-off binding

## Purpose

D-045 says a result that used any interpretation rule without expert sign-off is `interpretation: provisional`. This leaf decides, from the `GDT.RES.ROWFLOW` journal alone, which rules are signed off: it builds the **interpretation status register**, one entry per revision of every DMT row key and every defaults profile, with its content digest, its status (`draft`, `provisional` or `signed_off`) and the journaled sign-offs that justify it. It also defines the one content-digest function, the one verification of a register file and the one lookup that every result builder (`GDT.CHAR.INTERP`), comparison (`GDT.CHAR.PDIFF`), pacing gate (`GDT.RES.PACE`) and public rule page (`OUTR.GDTC.RULES`) use. A register is a file anyone can edit, so it counts only after it has been rebuilt from the journal and matched against the digests the project publishes.

## Scope

**In:**
- The rule content digest: which fields it covers, and the function `row_content_digest`
- Register format `gdt-rule-status/v1` and its digest (SHA-256 over RFC 8785 canonical JSON)
- Status derivation per revision from ROWFLOW revisions and `gdt.review_dmt_row@1` events, counting only human, roster-held, conflict-free approvals that name the revision's digest
- Profiles as rules, signed off through the row-like key that `GDT.RES.ROWASME.DPROF` proposes
- `verify_register`: rebuild from the journal and compare with the file and with the project-published digests of the register and the roster
- `status_of(register, rule, content_digest)` with reasons
- `gdt.export_rule_status@1` (W1, write to an approved local root)

**Out:**
- The review command and its stale-digest check (owned by `GDT.RES.ROWFLOW.REVIEW`)
- Stamping statuses into results and refusing `draft` rules (owned by `GDT.CHAR.INTERP`)
- Conflict-of-interest rules themselves and the reviewer register (owned by `OUTR.GDTC.REVIEW`, read as an artifact)
- Rendering rule pages, owners and deadlines (owned by `OUTR.GDTC.RULES`)

## Requirements

- **REQ-GDT.RES.STATUS-001** A DMT row's content digest shall be the SHA-256 of the RFC 8785 canonical JSON of every `DmtRow` field except `review`, that is `key`, `dialect`, `editions`, `characteristic`, `modifiers`, `clauses`, `variants` (with their operator chains), `default_variant`, `actual_value`, `decision_rule_hint`, `ors_cases`, `status`, `limitations`, `notes` and `synthetic`, and any field the schema adds later unless a decision record excludes it. A profile's content digest shall be its `GDT.CHAR.DPROF` digest. `row_content_digest` shall be the only function that computes either.
- **REQ-GDT.RES.STATUS-002** The register shall hold one entry per (rule ID, revision) for every DMT row key and every published defaults profile, each with kind (`dmt_row` or `profile`), revision, revision timestamp (`revised_at`, UTC, from the journal event that created the revision), content digest, author kind, status, reason, and the reviewer IDs and journal entry hashes of the counted approvals, sorted by (rule ID, revision).
- **REQ-GDT.RES.STATUS-003** An approval shall count only when its `gdt.review_dmt_row@1` event names the revision's content digest, its principal kind is `human`, the reviewer held the role in the roster at the event time, and it breaks none of the conflict-of-interest rules of `OUTR.GDTC.REVIEW` (same organisation ID as the other approver, an approver's organisation sponsoring the profile, a declared recusal).
- **REQ-GDT.RES.STATUS-004** A revision shall be `signed_off` only when it is the latest revision of its rule, holds two counted approvals from distinct reviewers and has no later `request_changes`; a latest revision authored by an agent with no human re-proposal shall be `draft`; every other revision shall be `provisional`.
- **REQ-GDT.RES.STATUS-005** When a rule gets a new revision, every earlier revision of that rule shall be `provisional` with reason `superseded`, whatever sign-off it held.
- **REQ-GDT.RES.STATUS-006** A defaults profile shall be `signed_off` only when a signed-off revision of its sign-off key names the profile's own digest; until that key exists every profile shall be `provisional` with reason `no_signoff_path`.
- **REQ-GDT.RES.STATUS-007** `verify_register(file, journal, roster, coi_inputs, published)` shall rebuild the register from the journal, and shall return `verified` only when the rebuilt register is byte-identical to the file, its digest is listed in the project-published digest file `gdt-published/v1`, and the roster digest it used is listed there too; otherwise it shall return `unverified` with `register_mismatch`, `register_not_published` or `foreign_roster`.
- **REQ-GDT.RES.STATUS-008** `status_of(register, rule, content_digest)` shall return the entry's status and reason; if the register is not verified, then it shall return `provisional` with reason `unverified_register`; if no register is loaded, `provisional` with `no_register`; if the verified register holds no entry for that rule and digest, `provisional` with `not_in_register`. It shall never return `signed_off` without a matching entry in a verified register.
- **REQ-GDT.RES.STATUS-009** `gdt.export_rule_status@1` shall write the register and its digest to an approved local export root, journal one event with the register digest, the roster digest and the journal head it was built from, and produce byte-identical output for the same inputs.
- **REQ-GDT.RES.STATUS-010** `gdt-published/v1` shall be an append-only ordered list, and `verify_register` shall take a purpose `evaluate` or `replay`: for `evaluate` it shall return `verified` only for the latest listed register (otherwise `unverified` with `stale_register`); for `replay` any listed register shall verify.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/status/happy/` (one human-authored row revision with two human approvals naming its digest, reviewers of different organisation IDs), when `fid gdt export-rule-status --out <handle> --json` runs, then the entry is `signed_off` with both reviewer IDs, journal hashes and `revised_at`, and one event carries the register and roster digests.
- **AC-2** Given `one_approval/`, then the entry is `provisional` with reason `awaiting_review`.
- **AC-3** Given `agent_draft/` (agent-authored latest revision, no human re-proposal), then the entry is `draft`; given `agent_rewritten/` (a human re-proposal with `derived_from`), then it is `provisional`.
- **AC-4** Given `changed/` (revision 1 signed off, revision 2 with a different content digest and no reviews), then revision 2 is `provisional` and revision 1 is `provisional` with reason `superseded`.
- **AC-5** (error path) Given `wrong_digest/` (approvals naming another digest), `agent_approval/` (an approval event whose principal kind is `agent`), `same_org/` (both approvers with one organisation ID) and `recused/` (an approver who declared a recusal for the rule), then in each case the entry is `provisional`, not `signed_off`.
- **AC-6** Given a profile with no sign-off key in the fixture set, then its entry is `provisional` with reason `no_signoff_path`; given `profile_signed/`, then `signed_off`.
- **AC-7** (forged register) Given `forged/` (the AC-1 register file edited so a `provisional` entry reads `signed_off`), when `verify_register` runs with the journal, roster and `gdt-published/v1`, then it returns `unverified` with `register_mismatch`, and `status_of` on every rule returns `provisional` with `unverified_register`.
- **AC-8** (foreign roster) Given `foreign_roster/` (a register correctly rebuilt from the journal with a roster whose digest is not in `gdt-published/v1`), then `verify_register` returns `unverified` with `foreign_roster`.
- **AC-9** Given a verified register, when `status_of` is called with an unknown rule and with a known rule at a digest not listed, then both return `provisional` with `not_in_register`; with no register loaded, `no_register`.
- **AC-10** Given a fixture row and a copy whose `notes` field differs, then `row_content_digest` differs; given a copy whose `review` field differs, then it is equal. Given the signed-off row of AC-1 re-proposed with only `synthetic` flipped, then the digest differs, the new revision is `provisional` with no approvals, the signed-off revision is `superseded`, and `status_of` at the new digest never answers `signed_off`.
- **AC-11** (determinism and replay) Running AC-1 twice gives byte-identical files; exact replay of its journal gives `IDENTICAL`.
- **AC-12** (register currency) Given `gdt-published/v1` listing registers R1 then R2, when `verify_register` runs on R1 with purpose `evaluate`, then `unverified` with `stale_register` and `status_of` answers `provisional`/`unverified_register` for every rule; with purpose `replay`, R1 verifies; R2 verifies for both purposes.

## Design

- **Registry command(s):** `gdt.export_rule_status@1`, W1, `fs_effect: write_root` (CONSTITUTION §3.1); params `out: ExportHandle`, optional `at: JournalHash` (default: head); result `{ register_digest, roster_digest, entries, path }`. CLI `fid gdt export-rule-status`, MCP `gdt_export_rule_status`, `--json`. Agents may run it: it publishes facts and changes no status.
- **Data structures:** `RuleStatusRegister { schema: "gdt-rule-status/v1", built_from: { journal_head, roster_digest, coi_digest }, entries: Vec<RuleStatusEntry> }`; `RuleStatusEntry { rule: RuleId, kind: DmtRow | Profile, revision: u32, revised_at: UtcTimestamp, content_digest: Sha256, author_kind, status: Draft | Provisional | SignedOff, reason: Option<Reason>, approvals: Vec<{ reviewer: ReviewerId, journal_entry: Sha256 }> }`; `Reason { awaiting_review, superseded, not_in_register, no_signoff_path, no_register, unverified_register }` (a `stale_register` verification yields `unverified_register`); `Purpose { Evaluate, Replay }`; `Verification { Verified{digest} | Unverified{register_mismatch | register_not_published | foreign_roster | stale_register} }`; `StatusAnswer { status, reason, entry: Option<&RuleStatusEntry> }`. For a DMT row, `RuleId` is the DMT key (which carries its major version) and `revision` is the ROWFLOW revision number; for a profile, `RuleId` is the full profile ID with its semantic version and `revision` is always 1, because profiles are immutable and a new version is a new rule.
- **Published digests (`gdt-published/v1`):** a JSON file in the DMT data repository holding an ordered, append-only list of the register digests (each with the roster digest it was built from) that the project's CI produced on its main branch; it is appended by that CI only, and its last entry is the latest register.
- **Algorithm:** fold ROWFLOW revisions and review events in journal order; per rule keep revisions in order; apply STATE's rules; count an approval per REQ-003 using the roster and the conflict inputs (reviewer register `gdt-reviewer-register/v1` with organisation IDs and recusals, profile registry `gdt-profiles/v1` sponsors); mark all but the latest revision `superseded`; sort; canonicalise. `verify_register` runs the same fold and compares bytes and digests.
- **Errors:** `export_root_denied`, `journal_unreadable`, `roster_unreadable`, `coi_inputs_unreadable`; findings and verification outcomes are data.
- **Determinism:** D0; no clock (timestamps come from journal events), sorted maps only.
- **Performance budget:** 5 000 rules with 20 revisions each in < 500 ms [unverified].
- **Crate:** `fid-gdt` (module `dmt_flow::status`); `row_content_digest` in `fid-gps` (module `dmt`) so the resolver can call it.

## Test fixtures

Synthetic revisions, review events, rosters, reviewer and profile registries and a `gdt-published/v1` file under `fixtures/gdt/status/` (invented keys, clause numbers and organisation IDs, no standards text; CC0), hand-labelled from the rules above.

## Build prompt

> In crate `fid-gps`, module `dmt`, add `row_content_digest(&DmtRow) -> Sha256`: SHA-256 of the RFC 8785 canonical JSON of every row field except review state (every `DmtRow` field except `review`: key, dialect, editions, characteristic, modifiers, clauses, variants with chains, default_variant, actual_value, decision_rule_hint, ors_cases, status, limitations, notes, synthetic). In crate `fid-gdt`, add module `dmt_flow::status`. Build the register `gdt-rule-status/v1` from DMT row revisions and `gdt.review_dmt_row@1` events and from published defaults profiles: one entry per (rule ID, revision), sorted, with kind, revision, `revised_at` (from the journal event), content digest, author kind, status (`draft` | `provisional` | `signed_off`), reason (`awaiting_review`, `superseded`, `not_in_register`, `no_signoff_path`, `no_register`, `unverified_register`) and counted approvals. Count an approval only if its event names the revision's digest, its principal kind is `human`, the reviewer held the roster role at that time, and it breaks no conflict rule (two approvers with one organisation ID; an approver's organisation sponsoring the profile; a declared recusal) read from `gdt-reviewer-register/v1` and `gdt-profiles/v1`. `signed_off` only for the latest revision with two counted approvals from distinct reviewers and no later `request_changes`; latest agent-authored revision without human re-proposal is `draft`; earlier revisions are `provisional` with `superseded`; profiles are `signed_off` only through a signed-off sign-off key naming their digest (else `no_signoff_path`); profile revision is always 1. Provide `verify_register(file, journal, roster, coi, published, purpose)` that rebuilds and returns `Verified` only if the rebuild is byte-identical to the file and both the register digest and the roster digest are listed in `gdt-published/v1` (else `register_mismatch`, `register_not_published` or `foreign_roster`), and for purpose `Evaluate` only if the register is the list's last entry (else `stale_register`; `Replay` accepts any listed register), and `status_of(register, verification, rule, digest)` returning `provisional` with `unverified_register`, `no_register` or `not_in_register` in those cases and never `signed_off` without a matching entry in a verified register. Register `gdt.export_rule_status@1` (W1, write_root; CLI `fid gdt export-rule-status`, MCP `gdt_export_rule_status`, `--json`), one journal event with the register digest, roster digest and journal head; byte-identical output for the same inputs. Tests per AC-1..AC-12 with synthetic fixtures under `fixtures/gdt/status/`. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`, never store or emit standards text.

## Open questions

- The profile sign-off key depends on how `GDT.RES.ROWASME.DPROF` settles profile review; REQ-006 fails closed (`no_signoff_path`) until then.
- The conflict inputs are public files owned by `OUTR.GDTC`; reading them as artifacts keeps GDT below OUTR in the layer order. Until they exist, the build uses empty conflict inputs, which count every approval that passes the other checks; the project-published register is only produced with real conflict inputs.
- **Built ahead (PR #320):** `row_content_digest` (REQ-001) and `DmtRow::content_cvalue` were built in `fid-gps` (module `dmt`) by `GDT.RES.ROWFLOW.STATE`, which needs the same digest; this leaf re-exports that function from `dmt_flow::status` and adds no second one.
- **STATE's `signed_off` is not a counted sign-off (PR #320 review):** `GDT.RES.ROWFLOW.STATE` reaches `signed_off` when two distinct roster reviewers hold live approvals and applies no conflict-of-interest rule. This leaf shall never copy that state into the register: it recounts approvals from the journal under REQ-003 (organisation, sponsorship, recusal) and derives the status itself. It may rebuild revisions with STATE's `replay`, which re-checks the workflow invariants from the journaled records.
- **Trusted rosters for STATE's `replay` and `transition` (PR #320 re-review; built):** `Published::trusted_rosters` is the only constructor of `TrustedRosters` and refuses any roster whose digest `gdt-published/v1` does not list (`foreign_roster`); STATE's `replay` looks rosters up in it by digest and `transition` checks reviewers against `TrustedRosters::current`, the roster of the latest published register. A new roster therefore admits reviewers only once a register built with it is published; the register's own `built_from.roster_digest` (the current roster, which may be new) is checked by `verify_register`, not when building, so the first register of a new roster can be produced.
- **Build choices (first build):** the register reads a journal extract `gdt-rowflow-journal/v1` (entry hash, UTC time, command, header principal, payload, and the row on proposals) because `GDT.RES.ROWFLOW.PROPOSE` and `.REVIEW`, which will journal these entries, are unbuilt; they should journal the STATE record as the payload and the proposed row with it. Review entries STATE's `replay` refuses (non-human principal, stale digest, no roster role, a roster not in `TrustedRosters`, ...) are not applied rather than failing the whole build; a refused proposal or a revision number out of order fails it (`journal_unreadable`). Each counted approval also carries its journal entry time (`at`) so `OUTR.GDTC.GUARD` can compare approval times with a governance `deadline_set` date (that date lives in `OUTR.GDTC.RULES`'s governance register, not here). With a reviewer register loaded, an approver it does not list is not counted (fail closed); with no conflict inputs every approval that passes the other checks counts, as above, and `coi_digest` records which. Profiles are taken from `gdt.publish_defaults_profile@1` entries on the working branch.
- **Not built (needs `GDT.RES.ROWASME.DPROF`):** the profile sign-off path (AC-6 `profile_signed/`) and with it the sponsor conflict rule, which applies only to profile sign-off; every profile stays `provisional`/`no_signoff_path`. The sponsoring organisations are already read from `gdt-profiles/v1`.
- `GDT.RES.RESOLVE` (built) should expose `row_content_digest` of the row it resolved in `Resolution::Resolved`; until it does, `GDT.CHAR.INTERP` calls `row_content_digest` on the resolved row itself.

## Children

_Leaf: none._
