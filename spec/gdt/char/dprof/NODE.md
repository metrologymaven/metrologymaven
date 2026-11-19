---
id: GDT.CHAR.DPROF
title: Defaults profiles: immutable versioned objects
level: 3
parent: GDT.CHAR
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-gdt]
built_pr: 220
satisfies: [REQ-GDT.CHAR-004]
covers: []
depends_on: [CORE.JRN, CORE.STO]
side_effect: W2
determinism: D0
cli: [fid gdt publish-defaults-profile, fid gdt repoint-defaults-profile]
mcp: [gdt_publish_defaults_profile, gdt_repoint_defaults_profile]
journal: [gdt.publish_defaults_profile@1, gdt.repoint_defaults_profile@1]
updated: 2026-11-19
---

# GDT.CHAR.DPROF: Defaults profiles: immutable versioned objects

## Purpose

A defaults profile names the default choices that apply under one dialect and edition when a drawing is silent (which DMT row variants, default association keys and similar). This leaf owns the **container**: an immutable, content-addressed, semantically versioned object with a store, a publish command and a re-point command. It does not own what the defaults *are*; the keys a profile may set and their allowed values come from the DMT schema (`GDT.RES.SCHEMA`), and their content is expert-authored under D-009.

## Scope

**In:** `DefaultsProfile` object and ID grammar `dp:<dialect>-<edition>@x.y.z#sha256:<hex>`; digest over RFC 8785 canonical JSON; project-local profile store; `gdt.publish_defaults_profile@1`; `gdt.repoint_defaults_profile@1` to move named characteristics to another profile version; agent-authored profiles land on an agent branch and are provenance-tagged (C5).
**Out:** profile content and the meaning of keys (owned by `GDT.RES` rows, authored per D-009); governing-spec validation (owned by `GDT.CHAR.SPEC`); row attestation (owned by `TRUST` and `ECO`).

## Requirements

- **REQ-GDT.CHAR.DPROF-001** A defaults profile shall be identified by dialect, edition, semantic version and the SHA-256 digest of its RFC 8785 canonical JSON form.
- **REQ-GDT.CHAR.DPROF-002** If a publish request carries an ID and version that already exist with a different digest, then the system shall reject it with `profile_immutable`.
- **REQ-GDT.CHAR.DPROF-003** If a publish request reuses an existing version with an identical digest, then the system shall return the existing profile and journal nothing.
- **REQ-GDT.CHAR.DPROF-004** When a profile is published, existing characteristics shall keep their current profile reference.
- **REQ-GDT.CHAR.DPROF-005** When `gdt.repoint_defaults_profile@1` runs, the system shall change only the named characteristics, journal one event listing each characteristic with its old and new reference, and refuse the change if any named characteristic's dialect differs from the target profile's dialect.
- **REQ-GDT.CHAR.DPROF-006** While the principal is an agent, the publish command shall write the profile to the agent branch with agent provenance, and the profile shall not be selectable by a working-branch characteristic until a human adopts it.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/dprof/asme_placeholder_1.0.0.json`, when `fid gdt publish-defaults-profile --file <handle> --json` runs, then the result ID is `dp:asme-y14.5-2018@1.0.0#sha256:<d>` where `<d>` equals the SHA-256 of the file's RFC 8785 form, and one event is journaled.
- **AC-2** (error path) Given AC-1 then fixture `asme_placeholder_1.0.0_modified.json` with the same version, when published, then it fails with `profile_immutable` and journals nothing.
- **AC-3** Given AC-1 re-published unchanged, then the existing ID is returned and the journal length is unchanged.
- **AC-4** Given three characteristics on 1.0.0 and a published 1.1.0, when `fid gdt repoint-defaults-profile --chars c1,c2 --to <1.1.0 ref> --json` runs, then c1 and c2 reference 1.1.0, c3 still references 1.0.0, and one event lists both changes.
- **AC-5** (error path) Given an ISO profile as target and an ASME characteristic, when re-pointed, then it fails with `dialect_mismatch` and nothing changes.
- **AC-6** (agent) Given principal `agent`, when a profile is published, then it exists only on the agent branch with agent provenance and `gdt.set_governing_spec@1` on the working branch rejects it with `defaults_profile_unknown`.
- **AC-7** (replay) Given the journal of AC-1 and AC-4, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `gdt.publish_defaults_profile@1` (W2; params `file: ImportHandle`; result `ProfileRef`); `gdt.repoint_defaults_profile@1` (W2; params `chars: [CharId]`, `to: ProfileRef`; result list of changes). Both support `--json` and `--dry-run`. Agents pass handles, never raw paths.
- **Data structures:** `DefaultsProfile { dialect, edition, version, keys: BTreeMap<DmtKey, Value>, provenance }`, keys validated against the DMT schema's allowed key set.
- **Algorithm:** canonicalise (RFC 8785), hash (SHA-256), look up by (ID, version), compare digests, store.
- **Errors:** `profile_immutable`, `dialect_mismatch`, `unknown_key`, `defaults_profile_unknown`, `malformed_profile`.
- **Determinism:** D0.
- **Performance budget:** publish < 50 ms for a 1 MB profile [unverified].
- **Crate:** `fid-gdt`.
- **Pinned choices (as built in `fid-gdt`, module `chars::dprof`):**
  - *Document:* `{schema: "fid.defaults-profile/1", dialect, edition, version, keys}`, every field required and no other; JSON with no duplicate key, at most 2 MiB and nesting depth 3. The digest is SHA-256 over the RFC 8785 form of the whole document.
  - *Values:* string (at most 280 characters), boolean, integer with magnitude at most 2^53 - 1, or a flat list of at most 256 of these. Every number must be written as an integer literal in the source text (an optional minus and digits; no `.` and no exponent), so `1.0`, `1e2` and `7.0000000000000001` are `malformed_profile{bad_number}` even though their values are integral. The same check applies to store snapshots. The magnitude is checked on the RFC 8785 text, with no float arithmetic. Null, objects and nested lists are `malformed_profile`. At most 4096 keys.
  - *Keys:* grammar `[a-z0-9][a-z0-9_.:/+@-]{0,127}`; the allowed set and the value each key allows come from `GDT.RES.SCHEMA` (`fid_gps::dmt::profile_vocabulary`, built with `KeyVocabulary::from_dmt_table(table, dialect)`); `KeyVocabulary::new` remains for explicit key lists that accept any value (tests). All structure is checked before the vocabulary; the first unknown key in byte order is named in `unknown_key`. With a schema vocabulary, after all structural checks (fields, schema, dialect, edition, version, keys and values), a profile of another dialect is `malformed_profile{vocabulary_dialect}`, and after the key check a value its key does not allow (a variant the row lacks, or a non-text value) is `malformed_profile{value_not_allowed}`.
  - *Grammars:* dialect `asme` or `iso`; edition 1 to 32 bytes of `[a-z0-9.-]`, alphanumeric at both ends, no two separators in a row; version `x.y.z`, each part `0` or up to nine digits without a leading zero, no pre-release or build suffix; the dialect ends at the first `-` of the reference.
  - *Uniqueness (per branch, C5):* each branch holds at most one digest per (dialect, edition, version). An agent publish never blocks or changes anything on the working branch. An identical re-publish on the same branch is a no-op with no event.
  - *Branches:* a human writes the working branch. Agents and scripts write the agent branch, which holds profiles with the author as provenance plus an overlay of characteristic bindings. The agent branch can also select working-branch profiles; the working branch can select only its own. `adopt` (human only, otherwise `human_only`) copies an agent-branch profile onto the working branch, keeping its agent provenance and marking it `adopted`. It is refused with `adopt_conflict` when the working branch already holds that version with a different digest; if the working branch holds the identical profile, nothing changes.
  - *Re-point:* checks run in this order and all run before any change: list (`bad_request`: empty, more than 10 000, bad ID, duplicate), reference (`bad_profile_ref`, then `defaults_profile_unknown` on the principal's branch, digest included), then each characteristic in ID order (`char_unknown`, `dialect_mismatch`). The event lists every named characteristic sorted by ID, including any already on the target.
  - *Events:* canonical JSON without the profile content; replay fetches content by digest and compares re-derived event bytes (`IDENTICAL` / `DIVERGENT` / `NOT_REPLAYABLE`), stopping at the first step that is not identical.
  - *Snapshot:* the writer refuses output over the loader's 64 MiB limit with `store_invalid{too_large}`, so every snapshot it writes can be loaded again.
  - *Added error codes:* `adopt_conflict`, `bad_profile_ref`, `bad_request`, `char_unknown`, `char_exists`, `human_only`, `bad_vocabulary`, `store_invalid` and `event_invalid`, plus the import-handle codes of `fid-io-reg`.

## Test fixtures

Synthetic placeholder profiles whose keys are drawn from the DMT schema test vocabulary and whose values carry no standards content; CC0, in-repo.

## Build prompt

> In crate `fid-gdt`, implement defaults profiles as immutable content-addressed objects. ID grammar `dp:<dialect>-<edition>@<semver>#sha256:<hex>`, digest = SHA-256 of RFC 8785 canonical JSON. Commands: `gdt.publish_defaults_profile@1` (W2, CLI `fid gdt publish-defaults-profile`, MCP `gdt_publish_defaults_profile`, param is an approved import handle, never a raw path) and `gdt.repoint_defaults_profile@1` (W2, CLI `fid gdt repoint-defaults-profile`, MCP `gdt_repoint_defaults_profile`). Rules: same ID+version with a different digest is `profile_immutable`; identical re-publish is a no-op with no journal event; publishing never changes existing characteristics; re-point changes only named characteristics, journals one event listing old and new refs, and refuses a dialect mismatch; when the principal is an agent, write to the agent branch with provenance and keep it unselectable on the working branch until adopted by a human. Validate keys against the DMT schema key set (`unknown_key`). Tests per the acceptance criteria above, including exact replay = IDENTICAL. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`, bounded fail-closed parsing), `BTreeMap` not `HashMap`, no standards text in code or fixtures.

## Open questions

- Who may adopt a published profile on the working branch (any human, or only a DMT reviewer per D-009)? This leaf requires a human; a stricter role check can be added by `TRUST.PROFILE` without changing the command.

## Children

_Leaf: none._
