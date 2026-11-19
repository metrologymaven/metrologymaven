---
id: UNC.UEC.CLASS
title: Evidence class assignment rules
level: 3
parent: UNC.UEC
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-kunc]
built_pr: 252
satisfies: [REQ-UNC.UEC-002, REQ-UNC.UEC-005, REQ-UNC.UEC-006]
covers: []
depends_on: [UNC.UEC.STMT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# UNC.UEC.CLASS: Evidence class assignment rules

## Purpose

A pure, versioned function that decides which evidence class a result may carry, from the facts about how its uncertainty was obtained. It is the only place that can grant UEC-2 or UEC-3, so the claim "every contributor is sourced" or "validated task-specific" cannot be made by any producer on its own.

## Scope

**In:**
- Rule set `uec-assign@1`: inputs are the evaluation basis (none, declared, specification-derived, modelled, ISO 15530-3 substitution, ISO/TS 15530-4 simulation), the model's contributor source kinds and the validation record reference
- Choosing the UEC-0 reason when nothing was evaluated
- Downgrading to the highest class the evidence supports, with a typed downgrade reason

**Out:**
- The statement types (owned by `UNC.UEC.STMT`)
- Whether a source record is genuine or signed (owned by `TRUST`); this node checks that a record with the claimed digest exists and has the claimed kind and origin
- Suggested fixes for UEC-0 (owned by `UNC.UEC.EXPL`)

## Requirements

- **REQ-UNC.UEC.CLASS-001** When no evaluated or declared uncertainty exists for a result, the assignment function shall return UEC-0 with reason `model_missing` if no model is bound, `instrument_missing` if a bound model has an unbound instrument input, and `u_missing` otherwise.
- **REQ-UNC.UEC.CLASS-002** The assignment function shall return UEC-2 only when the basis is a modelled LPU or MCM evaluation (or an unvalidated substitution or simulation, REQ-UNC.UEC.CLASS-004) and every contributor references a source of kind `Dcc`, `Qualification` or `TypeA` whose kind is confirmed by a record looked up by digest (REQ-UNC.UEC-005). An IEMPS-origin `InstrumentModel` is not a characterized source in `uec-assign@1` (see Design, decision D1).
- **REQ-UNC.UEC.CLASS-003** If any contributor of a modelled evaluation has no source, source kind `Declared` or a specification-derived instrument model, then the assignment function shall return UEC-1 with downgrade reason `unsourced_contributor` naming the first such contributor (other uncharacterized or unverified contributors are downgraded likewise with the reasons pinned in Design).
- **REQ-UNC.UEC.CLASS-004** The assignment function shall return UEC-3 only when the basis is ISO 15530-3 substitution or ISO/TS 15530-4 simulation and a validation record reference (calibrated-workpiece comparison) is present; otherwise it shall return at most UEC-2 with downgrade reason `not_validated`.
- **REQ-UNC.UEC.CLASS-005** Declared and specification-derived bases shall always yield UEC-1.
- **REQ-UNC.UEC.CLASS-006** Every assignment shall record the rule-set version (`uec-assign@1`) in the statement's provenance.

## Acceptance criteria

- **AC-1** Given the decision table `fixtures/unc/class/table.csv` (every combination of basis, source-kind mix and validation flag, about 60 rows), when the function is evaluated on each row, then the class, UEC-0 reason and downgrade reason equal the expected columns.
- **AC-2** Given a modelled MCM evaluation whose 5 contributors are all `Dcc` except one `Declared` (row `mcm_one_declared`), when assigned, then the class is UEC-1 and the downgrade reason names that contributor.
- **AC-3** Given an ISO 15530-3 evaluation without a validation record (row `subst_no_validation`), when assigned, then the class is UEC-2 with reason `not_validated`.
- **AC-4** (error path) Given an input claiming basis `modelled` with an empty contributor list, when assigned, then the call fails with `empty_model` and returns no class.
- **AC-5** Given the same input twice, when assigned, then the outputs are identical, including provenance.

## Design

- **Registry command(s):** none; called by `UNC.PROP.SELECT`, `UNC.UEC.DECL`, `UNC.MODEL.SUBST` and `UNC.INST` producers before a statement is built. Algorithm ID `unc.uec.assign@1.0.0`, rule set `uec-assign@1`.
- **Crate and module:** `fid-kunc`, module `uec` (`fid_kunc::uec::assign`). Kernel, KNP-1: no new dependency, no `unsafe`, no panics, no hashing containers, no arithmetic.
- **Data structures:**
  - `enum Basis { None, Declared, SpecDerived, ModelledLpu, ModelledMcm, Iso15530_3, Iso15530_4 }`, wire names `none`, `declared`, `spec_derived`, `modelled_lpu`, `modelled_mcm`, `iso15530_3`, `iso15530_4`; `Basis::parse` refuses anything else (case-sensitive) with `unknown_basis`. `Basis::method()` maps to the statement method tag.
  - `enum Origin { Iemps, Specification }`; `SourceClaim { kind: SourceKind, origin: Option<Origin>, digest }` (origin set exactly for `instrument_model`); `ContributorSource { input_id, source: Option<SourceClaim> }` (`None` = references no source).
  - `AssignInput { basis, contributor_sources: Vec<ContributorSource>, validation_record: Option<RecordRef { digest }>, model_bound, instruments_bound }`.
  - `trait SourceRecords { fn lookup(&self, &Digest) -> Option<RecordFacts> }` with `RecordFacts::Source { kind, origin } | Validation`; `RecordTable` is the in-memory implementation (sorted `Vec`, binary search, at most 2^20 records, a digest listed twice is `duplicate_record`).
  - `AssignOutput { basis, class, uec0_reason, downgrade: Option<Downgrade { reason, input_id: Option<String> }>, is_claimed, rests_on_declared_input, rests_on_uncharacterized_input, citable_sources: Vec<(SourceKind, Digest)>, contributor_ids: Vec<String> }`. `citable_sources` holds every claim (any kind) whose record confirmed its kind and origin, sorted and unique (two contributors sharing one record give one entry); `contributor_ids` holds the sorted contributor input IDs of an evaluated basis and is empty for `none`, `declared` and `spec_derived` (a claim carries no contributors); `rule() = "uec-assign@1"`, `algorithm() = "unc.uec.assign@1.0.0"`.
  - Downgrade reasons: `unsourced_contributor`, `uncharacterized_contributor`, `unverified_source`, `not_validated`.
- **Source verification (maintainer rule: `SourceKind` is self-asserted).** Each claim is looked up by its content digest. A `dcc`, `qualification` or `type_a` claim counts as characterized only when the record exists and its kind and origin equal the claim; otherwise it is `unverified_source`. Claims that can never be characterized report their own weakness whatever the record says: no source, `declared` or a specification-origin instrument model is `unsourced_contributor`, an IEMPS-origin instrument model is `uncharacterized_contributor`. A record never upgrades a claim (a `declared` claim whose record says `dcc` stays unsourced). The validation record counts only when the lookup returns `Validation`; anything else is treated as absent. Genuineness and signatures remain `TRUST`'s.
- **Algorithm (pinned order; first failure returned):**
  1. more than 10 000 contributors: `invalid_input{contributors}`; an `input_id` that is not a statement token: `invalid_input{input_id}`;
  2. contributors sorted by `input_id` bytes; equal IDs: `duplicate_input`;
  3. origin present on a non-instrument claim or missing on an instrument claim: `inconsistent_input{origin}`;
  4. basis `none`: contributors (`inconsistent_input{contributors}`) or a validation record (`inconsistent_input{validation_record}`) are refused; else UEC-0 with `model_missing` / `instrument_missing` / `u_missing` (REQ-001);
  5. basis `declared` / `spec_derived`: a validation record is `inconsistent_input{validation_record}`; else UEC-1 with no downgrade, whatever the sources (REQ-005); `model_bound` and `instruments_bound` are not consulted;
  6. evaluated bases: `inconsistent_input{model_bound}`, `inconsistent_input{instruments_bound}`, `empty_model` (no contributors; also for substitution and simulation, whose contributors carry the calibrated-workpiece and repeatability sources), then a validation record on a modelled basis is `inconsistent_input{validation_record}`;
  7. scan in `input_id` order; the first contributor that is not a verified characterized source gives UEC-1 with its reason and `input_id` (REQ-UNC.UEC-005, REQ-003). This applies to substitution and simulation too, so UEC-3 also needs only characterized sources (as `UNC.UEC.STMT` requires);
  8. `iso15530_3` / `iso15530_4`: UEC-3 with a verified validation record, else UEC-2 with `not_validated` (no `input_id`) (REQ-004); `modelled_lpu` / `modelled_mcm`: UEC-2 (REQ-002).
- **Labels (maintainer rule: claimed values never look measured).** `is_claimed` = basis `declared` or `spec_derived`; `rests_on_declared_input` = some claim or its record has kind `declared`; `rests_on_uncharacterized_input` = some contributor is not a verified characterized source. UEC-0 sets none. `AssignOutput::statement(parts)` is the one path from an assignment to a statement and the only constructor of a UEC-2 or UEC-3 statement (`UNC.UEC.STMT` seals its public constructor with `class_requires_assignment`). In order it refuses: a `method` other than `basis.method()` (`inconsistent_input{method}`); any `parts.sources` entry whose (kind, digest) is not in `citable_sources` (`source_not_verified`), so an unverified, re-labelled or forged source is never cited; contributor input IDs that differ (as a set) from `contributor_ids` (`contributor_mismatch`); then it sets `class` and `uec0_reason`, adds `uec-assign@1` to `provenance.rule_sets` (REQ-006), validates like `UncertaintyStatement::new` (`statement_refused`, detail = the statement error code) and refuses a statement whose labels are weaker than the assignment's (`label_dropped{is_claimed | rests_on_declared_input | rests_on_uncharacterized_input}`), for example one that leaves an unverified or declared input out. The labels reach `fid-kdec`'s `EvidenceBasis` and `Decision` unchanged.
- **Errors:** `empty_model`, `unknown_basis`, `duplicate_input`, `duplicate_record`, `invalid_input{field}`, `inconsistent_input{field}`, `statement_refused{code}`, `label_dropped{field}`, `source_not_verified`, `contributor_mismatch`. No class is returned with an error (fail closed: an ambiguous input is refused, not guessed).
- **Decisions pinned by this build:**
  - **D1** An IEMPS-origin instrument model is not a characterized source in `uec-assign@1`: the parent REQ-UNC.UEC-005 lists calibration certificates, qualification records and type-A datasets only, and `UNC.UEC.STMT` refuses UEC-2/3 with an `instrument_model` source. Such a contributor downgrades to UEC-1 with `uncharacterized_contributor`. Admitting it later is a new rule-set version.
  - **D2** Contradictory inputs (basis `none` with contributors, a validation record on a non-substitution basis, an evaluated basis with no model or an unbound instrument, origins on the wrong kind, duplicate IDs) are refused rather than resolved.
  - **D3** Only one downgrade is recorded: the contributor downgrade wins over `not_validated`.
- **Determinism:** D0. No arithmetic; contributors scanned in byte order of `input_id`, so input order does not change the output.
- **Bounds:** at most 10 000 contributors per call, 2^20 records per table, one lookup per contributor plus one for the validation record; O(n log n).
- **Performance budget:** trivial (< 1 µs per call for small models) [unverified until measured].

## Test fixtures

`fixtures/unc/class/table.csv`, a project-authored decision table (CC-BY-4.0, 101 rows) listing every basis and source-kind mix, record outcome (confirmed, absent, other kind, a record shared by two contributors of the same kind or of different kinds) and validation flag with expected class, reasons, labels or error. It is written by the independent stdlib Python reference `fixtures/unc/class/make_table.py` (with hand labels it asserts against); CI runs `make_table.py --check`. Digests are derived in the test (`sha256("source:" + label)`, label defaulting to the `input_id`, `sha256("validation:" + case)`).

## Build prompt

> In crate `fid-kunc` (kernel, KNP-1, no panics, no `unwrap`), add `uec::assign(input: AssignInput) -> Result<AssignOutput, AssignError>` implementing rule set `uec-assign@1`, algorithm ID `unc.uec.assign@1.0.0`:
> 1. No evaluated or declared uncertainty: UEC-0 with reason `model_missing` (no model bound), `instrument_missing` (model bound, an instrument input unbound) or `u_missing` (otherwise).
> 2. Basis ISO 15530-3 substitution or ISO/TS 15530-4 simulation: UEC-3 only with a validation record; else UEC-2 with downgrade `not_validated`.
> 3. Basis modelled (LPU or MCM): UEC-2 only if every contributor source is `Dcc`, `Qualification` or `TypeA` confirmed by its record (an IEMPS-origin `InstrumentModel` is not, decision D1; the signature takes a `&dyn SourceRecords` lookup, see Design); else UEC-1 with downgrade `unsourced_contributor` naming the first offending contributor in `input_id` order. Empty contributor list fails `empty_model`.
> 4. Basis declared or specification-derived: UEC-1.
> Record `uec-assign@1` in provenance. Tests: every row of `fixtures/unc/class/table.csv` matches; the named cases `mcm_one_declared` and `subst_no_validation`; the `empty_model` error; repeated calls give identical output. Use the `EvidenceClass` and `Uec0Reason` types from module `statement` (UNC.UEC.STMT).

## Open questions

- Resolved (decision D1): IEMPS-origin instrument models are not characterized sources in `uec-assign@1`. Whether a future rule set may admit them (with a record kind for the performance-test evidence and a matching change to `UNC.UEC.STMT`) is open.

## Children

_Leaf: none._
