---
id: UNC.DEC
title: Typed conformance decision rules
level: 2
parent: UNC
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-UNC-004]
covers: []
depends_on: [UNC.UEC, UNC.PROP]
cli: [fid decide *]
mcp: [decide_*]
journal: [decide.*]
updated: 2026-11-16
---

# UNC.DEC: Typed conformance decision rules

## Purpose

The decision rule engine. Conformance decisions are made only by **typed, versioned decision rule objects** (`dr:*`), never by ad-hoc comparisons. A rule takes a result value, its uncertainty statement and its tolerance and returns `CONFORMS | NONCONFORMS | INDETERMINATE | NOT_DECIDABLE`, with conformance probability and specific risk where the rule needs them (JCGM 106:2012). It owns the rule catalog, dialect defaults, plan binding and the audit of rule changes.

## Scope

**In:**
- DecisionRule type: ID, version, standards reference (designator, edition, clause), kind (conformance probability, simple acceptance, guard band), parameters, outcomes
- Built-in rules: `dr:iso14253-1:2017/default`, `dr:simple-acceptance`, `dr:guard-band(r=x)`, customer rules `dr:customer/<id>`
- Label-only `dr:as-reported-by-source` for transcribed results (never a kernel decision)
- Conformance probability from MC samples or the normal approximation
- Specific consumer and producer risk; optional global risk with a declared prior
- Dialect defaults: ISO 14253-1 under ISO GPS; explicit rule required under ASME
- Rule binding in plans, tenant defaults and the "rule changed after measurement" event
- Capability warning when U/T exceeds a threshold

**Out** (and which node owns it instead):
- Producing U (owned by `UNC.PROP` and `UNC.UEC`)
- Dialect selection per characteristic (owned by `GDT`)
- Report display of rules (owned by `RPT`)
- Process priors from SPC (owned by `RPT`)

## Requirements

- **REQ-UNC.DEC-001** The system shall produce a conformance outcome only by evaluating a decision rule object with an ID and version, and every ResultRecord with an outcome shall carry that rule ID and version.
- **REQ-UNC.DEC-002** Every rule evaluation shall return exactly one of `CONFORMS`, `NONCONFORMS`, `INDETERMINATE`, `NOT_DECIDABLE`.
- **REQ-UNC.DEC-003** If a rule needs uncertainty and the result is UEC-0, or no rule is bound, then the system shall return `NOT_DECIDABLE` with a typed reason (`u_missing`, `rule_missing`).
- **REQ-UNC.DEC-004** While a characteristic is evaluated under ISO GPS and no rule is bound, the system shall apply `dr:iso14253-1:2017/default` with a minimum conformance probability of 0.95 [unverified: default value per ISO 14253-1:2017].
- **REQ-UNC.DEC-005** If a characteristic is evaluated under ASME and neither the plan nor a tenant default binds a rule, then the system shall return `NOT_DECIDABLE` with reason `rule_missing` and shall not fall back to any implicit rule.
- **REQ-UNC.DEC-006** When a conformance-probability rule is evaluated, the system shall compute the probability from MC samples when available and otherwise from the normal approximation only if `lpu_validated_by_mc` is set or the method selection permits it.
- **REQ-UNC.DEC-007** When simple acceptance is used, the system shall still report U and the conformance probability at the result, and shall raise a capability warning when U/T exceeds the configured threshold (default 0.25 [unverified]).
- **REQ-UNC.DEC-008** When a bound rule is changed after any result under it exists, the system shall journal a `decide.change_rule` event that marks every affected result and report "rule changed after measurement".
- **REQ-UNC.DEC-009** If an agent selects or changes a rule, then the system shall record it as a proposal that a human must adopt before results under it are released.
- **REQ-UNC.DEC-010** Any replay in which a conformance outcome differs shall be classified `DIVERGENT`.

## Interfaces

- **Offers:** `DecisionRule` type and catalog; `decide.evaluate_rule@1` (R1), `decide.bind_rule@1` (W2), `decide.change_rule@1` (W2), `decide.list_rule@1` (R0), `decide.explain_decision@1` (R0); journal `decide.*`.
- **Consumes:** `UncertaintyStatement` from `UNC.UEC`; MC samples from `UNC.PROP`; tolerances and dialect from `GDT`.

## Decomposition hints

TYPE (rule schema and catalog), EVAL (outcome and probability), RISK (specific and global risk), DEFAULT (dialect defaults and tenant defaults), BIND (plan binding and change audit).

## Open questions

- Current edition and reaffirmation status of ASME B89.7.3.1 needs confirmation [unverified].

## Split rationale

Split by **layer and purpose**: the kernel side holds the rule type and catalog (TYPE), the pure evaluation that produces outcomes and conformance probability (EVAL) and the risk figures (RISK), all in `fid-kdec` because outcomes are values of record; the command side (`fid-dec`) holds rule resolution with dialect defaults and the read/evaluate commands (DEFAULT), and the audited write path for bindings, changes, adoption and tenant defaults (BIND). Every requirement lands in exactly one place: outcome semantics in EVAL, "which rule" in DEFAULT, "who changed it and when" in BIND.

Alternatives considered:
- **Following the decomposition hints literally** (DEFAULT holding only defaults, BIND holding bindings and queries). Adjusted: the read commands need resolution, so they sit with it, and BIND stays a pure write path.
- **Risk inside EVAL.** Rejected: global risk needs a declared prior and numerical integration that most evaluations never use; keeping it separate keeps EVAL small and testable.
- **Rule types in the command crate.** Rejected: the kernel evaluates rules, and the kernel cannot depend on crates above it.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.DEC.TYPE | Decision rule type and built-in catalog (leaf) | 001 002 | none |
| UNC.DEC.EVAL | Rule evaluation and conformance probability (leaf) | 002 003 006 007 010 | none |
| UNC.DEC.RISK | Specific and global conformity risk (leaf) | 006 | none |
| UNC.DEC.DEFAULT | Rule resolution, dialect defaults and decide queries (leaf) | 001 003 004 005 | none |
| UNC.DEC.BIND | Rule binding, change audit and adoption (leaf) | 008 009 | none |
