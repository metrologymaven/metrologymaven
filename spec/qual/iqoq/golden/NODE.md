---
id: QUAL.IQOQ.GOLDEN
title: Golden-journal registration for upgrade OQ
level: 3
parent: QUAL.IQOQ
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.IQOQ-005]
covers: []
depends_on: [CORE.JRN.ENTRY, CORE.REG.DEF]
uses_artifacts: []
side_effect: W2
determinism: D0
cli: [fid validate register-golden]
mcp: [validate_register_golden]
journal: [validate.register_golden@1]
built_in: [fid-qual]
updated: 2026-11-22
---

# QUAL.IQOQ.GOLDEN: Golden-journal registration for upgrade OQ

## Purpose

Lets a customer person mark a recorded run in their own project as a golden journal: a branch range whose results define the expected outcome that OQ replays on every new installation. Because it sets an acceptance baseline, the command is `human_only: true` (D-031); an agent may suggest a golden but cannot register one.

## Scope

**In:**
- `validate.register_golden@1` (W2, `human_only: true`): branch, entry range, label; computes the expected outcome digest (canonical list of ResultRecord digests in the range)
- Supersede: registering with `supersedes: <golden_id>` retires the earlier golden
- The golden registry as journaled state, read by OQ

**Out** (and which node owns it instead):
- Replaying goldens (owned by `QUAL.IQOQ.OQ`)

## Requirements

- **REQ-QUAL.IQOQ.GOLDEN-001** When `validate.register_golden@1` runs with a branch and entry range, the system shall store a golden with ID, label, range, the product and kernel versions that recorded it, and the expected outcome digest.
- **REQ-QUAL.IQOQ.GOLDEN-002** The command shall declare `human_only: true`, and if the principal is an agent or script, then the bus shall refuse it and commit nothing.
- **REQ-QUAL.IQOQ.GOLDEN-003** If the range contains no committed ResultRecord, then the command shall fail with `validate.golden_empty`.
- **REQ-QUAL.IQOQ.GOLDEN-004** When a golden is registered with `supersedes`, the earlier golden shall be marked retired and excluded from later OQ runs.

## Acceptance criteria

- **AC-1** Given project `fixtures/qual/iqoq/golden/plate.fid` (a run with 6 results), when a human runs `fid validate register-golden --branch main --from 10 --to 42 --label plate-v1 --json`, then a golden is stored and its expected digest equals the pinned value.
- **AC-2** (error path) Given the same call from an MCP agent session, then it is refused with `policy_refused` and nothing is journaled.
- **AC-3** (error path) Given a range with no results, then it fails with `validate.golden_empty`.
- **AC-4** Given a second registration with `--supersedes <id>`, then the first golden is retired.

## Design

- **Registry command(s):** `validate.register_golden@1` (W2, `human_only: true`, request-only on MCP; params `{ branch, from_seq, to_seq, label, supersedes? }`; result `Golden`).
- **Data structures:** `Golden { id: UUIDv7, label: Untrusted<String>, branch, from_seq, to_seq, product_version, kernel_manifest_digest, expected_digest, retired }`.
- **Algorithm:** collect ResultRecord digests in entry order; expected digest = SHA-256 of their canonical JSON array.
- **Errors:** `validate.golden_empty`, `validate.golden_unknown` (bad `supersedes`).
- **Determinism:** D0.
- **Performance budget:** under 1 s for 10,000 entries.
- **Crate:** `fid-qual` (module `golden`).

## Test fixtures

`fixtures/qual/iqoq/golden/plate.fid`, built by the test harness from synthetic fits; CC-BY-4.0.

## Build prompt

> In crate `fid-qual`, add module `golden` and register `validate.register_golden@1` (W2, `human_only: true`, MCP request-only; CLI `fid validate register-golden --branch --from --to --label [--supersedes]`). Store a Golden {id UUIDv7, label untrusted, branch, from_seq, to_seq, product_version, kernel_manifest_digest, expected_digest = SHA-256 of the canonical JSON array of ResultRecord digests in the range, retired}. Fail with `validate.golden_empty` if there are no results; retire a superseded golden. Provide `active_goldens()` for module `oq`. No panics, deny unwrap/expect. Tests: AC-1..AC-4 of spec node QUAL.IQOQ.GOLDEN.

## Open questions

- Golden journals may contain export-controlled geometry; they never leave the project, and `QUAL.IQOQ.BUNDLE` exports only digests and outcomes.

## Children

_Leaf: none._
