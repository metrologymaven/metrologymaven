//! [`ResultRecordBuilder`], the record it builds, and the decision binding.

use super::fields::{AlgorithmRef, EvaluationBasis, InputDigests, KernelVersion, Measurand, Quantity, RunId};
use super::RecordError;
use crate::chars::dprof::{CharId, ProfileDoc};
use crate::chars::interp::{
    register_cvalue, Interpretation, InterpretationPin, Interpreted, RegisterView, RuleUse, Uninterpreted,
};
use crate::chars::spec::GoverningSpec;
use crate::dmt_flow::status::Purpose;
use fid_canon::CValue;
use fid_gps::dmt::{DmtKey, DmtRow};
use fid_kdec::{Decision, EvidenceBasis, NotDecidableReason as UncReason, Outcome, RuleId, RuleRef};
use fid_kunc::{CoverageInterval, EvidenceClass, Expanded, Method, Uec0Reason, UncertaintyStatement};
use fid_types::Digest;
use std::collections::BTreeMap;

/// `schema` value of a canonical record.
pub const RECORD_SCHEMA: &str = "fid.gdt-result/2";
/// `decision.source` of a decision UNC returned.
pub const DECISION_SOURCE_UNC: &str = "unc";
/// `decision.source` of a `NOT_DECIDABLE` GDT records itself (no rule, or no UNC decision).
pub const DECISION_SOURCE_GDT: &str = "gdt_not_decidable";

// ------------------------------------------------------------------ uncertainty

/// The uncertainty a record carries: its evidence class and, from the UNC uncertainty
/// statement, the evidence facts and the numbers. Built only from a validated
/// [`UncertaintyStatement`] or as a bare UEC-0, so UEC-0 never carries a number and a
/// claimed value keeps its `claimed` fact.
#[derive(Debug, Clone, PartialEq)]
pub struct Uncertainty {
    class: EvidenceClass,
    uec0_reason: Option<Uec0Reason>,
    method: Option<Method>,
    statement: Option<Digest>,
    unit: Option<String>,
    u_std: Option<f64>,
    expanded: Option<Expanded>,
    interval: Option<CoverageInterval>,
    claimed: bool,
    declared_input: bool,
    uncharacterized_input: bool,
}

impl Uncertainty {
    /// Takes class, facts, unit and numbers from a validated statement.
    pub fn of(stmt: &UncertaintyStatement) -> Uncertainty {
        Uncertainty {
            class: stmt.class(),
            uec0_reason: stmt.uec0_reason(),
            method: stmt.method(),
            statement: Some(stmt.digest()),
            unit: stmt.unit().map(str::to_owned),
            u_std: stmt.u_std(),
            expanded: stmt.expanded(),
            interval: stmt.interval(),
            claimed: stmt.is_claimed(),
            declared_input: stmt.rests_on_declared_input(),
            uncharacterized_input: stmt.rests_on_uncharacterized_input(),
        }
    }

    /// UEC-0 ("not evaluated") without a statement: no number, no method, no facts.
    pub fn uec0(reason: Uec0Reason) -> Uncertainty {
        Uncertainty {
            class: EvidenceClass::Uec0,
            uec0_reason: Some(reason),
            method: None,
            statement: None,
            unit: None,
            u_std: None,
            expanded: None,
            interval: None,
            claimed: false,
            declared_input: false,
            uncharacterized_input: false,
        }
    }

    /// The evidence class.
    pub fn class(&self) -> EvidenceClass {
        self.class
    }
    /// Digest of the statement, when there is one.
    pub fn statement(&self) -> Option<Digest> {
        self.statement
    }
    /// The value is claimed (`DECLARED`, `SPEC_DERIVED`), not evaluated.
    pub fn is_claimed(&self) -> bool {
        self.claimed
    }
    /// The value rests on at least one declared input.
    pub fn rests_on_declared_input(&self) -> bool {
        self.declared_input
    }
    /// The value rests on at least one uncharacterized input.
    pub fn rests_on_uncharacterized_input(&self) -> bool {
        self.uncharacterized_input
    }
    /// Whether any uncertainty number is present.
    pub fn has_value(&self) -> bool {
        self.u_std.is_some() || self.expanded.is_some() || self.interval.is_some()
    }

    fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("class".to_owned(), CValue::Str(self.class.wire().to_owned()));
        m.insert("uec0_reason".to_owned(), opt_str(self.uec0_reason.map(Uec0Reason::wire)));
        m.insert("method".to_owned(), opt_str(self.method.map(Method::wire)));
        m.insert("statement".to_owned(), opt_str(self.statement.as_ref().map(Digest::as_str)));
        m.insert("unit".to_owned(), opt_str(self.unit.as_deref()));
        m.insert("u_std".to_owned(), self.u_std.map_or(CValue::Null, CValue::F64Bits));
        let expanded = self.expanded.map_or(CValue::Null, |e| {
            let mut x = BTreeMap::new();
            x.insert("U".to_owned(), CValue::F64Bits(e.value));
            x.insert("k".to_owned(), CValue::F64Bits(e.k));
            CValue::Obj(x)
        });
        m.insert("expanded".to_owned(), expanded);
        let interval = self.interval.map_or(CValue::Null, |i| {
            let mut x = BTreeMap::new();
            x.insert("low".to_owned(), CValue::F64Bits(i.low));
            x.insert("high".to_owned(), CValue::F64Bits(i.high));
            x.insert("p".to_owned(), CValue::F64Bits(i.p));
            x.insert("kind".to_owned(), CValue::Str(i.kind.wire().to_owned()));
            CValue::Obj(x)
        });
        m.insert("interval".to_owned(), interval);
        m.insert("claimed".to_owned(), CValue::Bool(self.claimed));
        m.insert("declared_input".to_owned(), CValue::Bool(self.declared_input));
        m.insert("uncharacterized_input".to_owned(), CValue::Bool(self.uncharacterized_input));
        CValue::Obj(m)
    }
}

fn opt_str(s: Option<&str>) -> CValue {
    s.map_or(CValue::Null, |s| CValue::Str(s.to_owned()))
}

// ------------------------------------------------------------------ decision

/// Why GDT records `NOT_DECIDABLE` itself (REQ-GDT.CHAR.RECORD-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotDecidableReason {
    /// No decision rule is bound to the characteristic.
    NoRuleBound,
    /// A rule is bound, but UNC returned no decision.
    UncNoDecision,
}

impl NotDecidableReason {
    /// The wire name.
    pub const fn wire(self) -> &'static str {
        match self {
            NotDecidableReason::NoRuleBound => "no_rule_bound",
            NotDecidableReason::UncNoDecision => "unc_no_decision",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Unc(Decision),
    NotDecidable { reason: NotDecidableReason, rule: Option<RuleRef> },
}

/// The decision a record carries: either a [`Decision`] a UNC rule returned, or
/// `NOT_DECIDABLE` with a GDT reason. Fields are private and the only way in is
/// [`ResultRecordBuilder::bind_decision`], so GDT can never write an outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordDecision {
    kind: Kind,
}

fn canonical_rule(r: &RuleRef) -> bool {
    RuleRef::parse(&r.to_string()).as_ref() == Ok(r)
}

impl RecordDecision {
    /// See [`ResultRecordBuilder::bind_decision`].
    fn bind(bound: Option<&RuleRef>, unc: Option<Decision>) -> Result<RecordDecision, RecordError> {
        let kind = match (bound, unc) {
            (None, None) => Kind::NotDecidable { reason: NotDecidableReason::NoRuleBound, rule: None },
            (None, Some(_)) => return Err(RecordError::DecisionRuleMismatch),
            (Some(r), None) => {
                if !canonical_rule(r) {
                    return Err(RecordError::InvalidDecision { reason: "rule" });
                }
                Kind::NotDecidable { reason: NotDecidableReason::UncNoDecision, rule: Some(r.clone()) }
            }
            (Some(r), Some(d)) => {
                if d.rule() != Some(r) {
                    return Err(RecordError::DecisionRuleMismatch);
                }
                if !canonical_rule(r) {
                    return Err(RecordError::InvalidDecision { reason: "rule" });
                }
                if !matches!(d.outcome(), Outcome::NotDecidable(_)) {
                    // A side, or INDETERMINATE, needs an evaluated uncertainty and a bound
                    // guard band; `fid-kdec` checks both, and so does the record.
                    if d.basis().is_none_or(|b| b.class() == EvidenceClass::Uec0) {
                        return Err(RecordError::InvalidDecision { reason: "basis" });
                    }
                    if matches!(r.id, RuleId::GuardBand { r: None }) {
                        return Err(RecordError::InvalidDecision { reason: "unbound_rule" });
                    }
                }
                Kind::Unc(d)
            }
        };
        Ok(RecordDecision { kind })
    }

    /// The outcome wire name (`CONFORMS`, `NONCONFORMS`, `INDETERMINATE`, `NOT_DECIDABLE`).
    pub fn outcome(&self) -> &'static str {
        match &self.kind {
            Kind::Unc(d) => d.outcome().wire(),
            Kind::NotDecidable { .. } => "NOT_DECIDABLE",
        }
    }

    /// The `NOT_DECIDABLE` reason code (UNC's or GDT's), if any.
    pub fn reason(&self) -> Option<&'static str> {
        match &self.kind {
            Kind::Unc(d) => d.outcome().reason().map(|r| r.wire()),
            Kind::NotDecidable { reason, .. } => Some(reason.wire()),
        }
    }

    /// GDT's own `NOT_DECIDABLE` reason, when the record carries no UNC decision.
    pub fn gdt_reason(&self) -> Option<NotDecidableReason> {
        match &self.kind {
            Kind::Unc(_) => None,
            Kind::NotDecidable { reason, .. } => Some(*reason),
        }
    }

    /// The rule reference (`None` exactly for `no_rule_bound`).
    pub fn rule(&self) -> Option<&RuleRef> {
        match &self.kind {
            Kind::Unc(d) => d.rule(),
            Kind::NotDecidable { rule, .. } => rule.as_ref(),
        }
    }

    /// The UNC decision, when there is one.
    pub fn unc(&self) -> Option<&Decision> {
        match &self.kind {
            Kind::Unc(d) => Some(d),
            Kind::NotDecidable { .. } => None,
        }
    }

    /// The evidence basis of the UNC decision.
    pub fn basis(&self) -> Option<&EvidenceBasis> {
        self.unc().and_then(Decision::basis)
    }

    fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        let source = if self.unc().is_some() { DECISION_SOURCE_UNC } else { DECISION_SOURCE_GDT };
        m.insert("source".to_owned(), CValue::Str(source.to_owned()));
        m.insert("rule".to_owned(), self.rule().map_or(CValue::Null, |r| CValue::Str(r.to_string())));
        m.insert("outcome".to_owned(), CValue::Str(self.outcome().to_owned()));
        m.insert("reason".to_owned(), opt_str(self.reason()));
        let basis = self.basis().map_or(CValue::Null, |b| {
            let mut x = BTreeMap::new();
            x.insert("class".to_owned(), CValue::Str(b.class().wire().to_owned()));
            x.insert("method".to_owned(), opt_str(b.method().map(Method::wire)));
            x.insert("claimed".to_owned(), CValue::Bool(b.is_claimed()));
            x.insert("declared_input".to_owned(), CValue::Bool(b.rests_on_declared_input()));
            x.insert("uncharacterized_input".to_owned(), CValue::Bool(b.rests_on_uncharacterized_input()));
            x.insert("statement".to_owned(), CValue::Str(b.statement().as_str().to_owned()));
            CValue::Obj(x)
        });
        m.insert("basis".to_owned(), basis);
        CValue::Obj(m)
    }
}

// ------------------------------------------------------------------ record

/// A finished ResultRecord: every CONSTITUTION §5 field plus the three DMT identifiers, the
/// interpretation block of `GDT.CHAR.INTERP` (`interpretation`, `interpretation_rules`,
/// `interpretation_register`), its canonical JSON and the SHA-256 of that JSON. Built only by
/// [`ResultRecordBuilder::finish`]; the canonical JSON always holds `interpretation`, and
/// there is no other serialisation.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultRecord {
    run: RunId,
    characteristic: CharId,
    measurand: Measurand,
    evaluation_basis: EvaluationBasis,
    value: Quantity,
    uncertainty: Uncertainty,
    decision: RecordDecision,
    spec: GoverningSpec,
    algorithm: AlgorithmRef,
    kernel_version: KernelVersion,
    input_digests: InputDigests,
    journal_entry: Digest,
    dmt_row: DmtKey,
    operator_chain_digest: Digest,
    dmt_digest: Digest,
    interpretation: Interpretation,
    interpretation_rules: Vec<RuleUse>,
    interpretation_register: Option<Digest>,
    interpretation_pin: InterpretationPin,
    canonical: Vec<u8>,
    digest: Digest,
}

impl ResultRecord {
    /// The run.
    pub fn run(&self) -> &RunId {
        &self.run
    }
    /// The characteristic.
    pub fn characteristic(&self) -> &CharId {
        &self.characteristic
    }
    /// The measurand.
    pub fn measurand(&self) -> &Measurand {
        &self.measurand
    }
    /// The evaluation basis, exactly as declared.
    pub fn evaluation_basis(&self) -> EvaluationBasis {
        self.evaluation_basis
    }
    /// The value with its unit.
    pub fn value(&self) -> Quantity {
        self.value
    }
    /// The uncertainty.
    pub fn uncertainty(&self) -> &Uncertainty {
        &self.uncertainty
    }
    /// The decision.
    pub fn decision(&self) -> &RecordDecision {
        &self.decision
    }
    /// The governing spec (standard, edition, defaults profile).
    pub fn spec(&self) -> &GoverningSpec {
        &self.spec
    }
    /// The algorithm ID and version.
    pub fn algorithm(&self) -> &AlgorithmRef {
        &self.algorithm
    }
    /// The kernel version.
    pub fn kernel_version(&self) -> &KernelVersion {
        &self.kernel_version
    }
    /// The input digests, sorted.
    pub fn input_digests(&self) -> &[Digest] {
        self.input_digests.as_slice()
    }
    /// The journal entry hash.
    pub fn journal_entry(&self) -> Digest {
        self.journal_entry
    }
    /// The DMT row ID.
    pub fn dmt_row(&self) -> &DmtKey {
        &self.dmt_row
    }
    /// The operator-chain digest.
    pub fn operator_chain_digest(&self) -> Digest {
        self.operator_chain_digest
    }
    /// The DMT digest.
    pub fn dmt_digest(&self) -> Digest {
        self.dmt_digest
    }
    /// `provisional` or `signed_off` (`GDT.CHAR.INTERP` REQ-004).
    pub fn interpretation(&self) -> Interpretation {
        self.interpretation
    }
    /// The rules used, sorted by (kind, rule ID) (`GDT.CHAR.INTERP` REQ-001).
    pub fn interpretation_rules(&self) -> &[RuleUse] {
        &self.interpretation_rules
    }
    /// Digest of the status register consulted, `None` for `none`.
    pub fn interpretation_register(&self) -> Option<Digest> {
        self.interpretation_register
    }
    /// What the evaluating command pins in its journal entry (`GDT.CHAR.INTERP` REQ-007).
    pub fn interpretation_pin(&self) -> &InterpretationPin {
        &self.interpretation_pin
    }
    /// The canonical RFC 8785 bytes.
    pub fn canonical_json(&self) -> &[u8] {
        &self.canonical
    }
    /// SHA-256 of [`Self::canonical_json`].
    pub fn digest(&self) -> Digest {
        self.digest
    }

    fn to_cvalue(&self) -> CValue {
        let s = |x: &str| CValue::Str(x.to_owned());
        let mut m = BTreeMap::new();
        m.insert("schema".to_owned(), s(RECORD_SCHEMA));
        m.insert("run".to_owned(), s(self.run.as_str()));
        m.insert("characteristic".to_owned(), s(self.characteristic.as_str()));
        m.insert("measurand".to_owned(), s(self.measurand.as_str()));
        m.insert("evaluation_basis".to_owned(), s(self.evaluation_basis.wire()));
        m.insert("value".to_owned(), self.value.to_cvalue());
        m.insert("uncertainty".to_owned(), self.uncertainty.to_cvalue());
        m.insert("decision".to_owned(), self.decision.to_cvalue());
        m.insert("spec".to_owned(), self.spec.to_cvalue());
        m.insert("algorithm".to_owned(), s(self.algorithm.as_str()));
        m.insert("kernel_version".to_owned(), s(self.kernel_version.as_str()));
        m.insert(
            "input_digests".to_owned(),
            CValue::Arr(self.input_digests.as_slice().iter().map(|d| s(d.as_str())).collect()),
        );
        m.insert("journal_entry".to_owned(), s(self.journal_entry.as_str()));
        m.insert("dmt_row".to_owned(), s(self.dmt_row.as_str()));
        m.insert("operator_chain_digest".to_owned(), s(self.operator_chain_digest.as_str()));
        m.insert("dmt_digest".to_owned(), s(self.dmt_digest.as_str()));
        m.insert("interpretation".to_owned(), s(self.interpretation.as_str()));
        m.insert(
            "interpretation_rules".to_owned(),
            CValue::Arr(self.interpretation_rules.iter().map(RuleUse::to_cvalue).collect()),
        );
        m.insert("interpretation_register".to_owned(), register_cvalue(self.interpretation_register));
        CValue::Obj(m)
    }
}

// ------------------------------------------------------------------ builder

/// Collects the fields of one ResultRecord. The run and the characteristic are fixed at
/// [`ResultRecordBuilder::new`]; every other field is set once by its method. Setting a
/// field again to an equal value is a no-op; to a different value it is an error
/// (`basis_changed`, `uncertainty_changed`, `uec0_with_value`, `field_changed{field}`). The
/// first error is kept and returned by [`ResultRecordBuilder::finish`]; later calls do not
/// replace it.
///
/// The type parameter is the interpretation state (`GDT.CHAR.INTERP`): a builder starts
/// [`Uninterpreted`], which has no `finish`; [`ResultRecordBuilder::interpretation`] turns it
/// into a builder holding the [`Interpreted`] block, which can finish. So no code path can
/// build or serialise a record without its `interpretation` field.
#[derive(Debug, Clone)]
pub struct ResultRecordBuilder<I = Uninterpreted> {
    run: RunId,
    characteristic: CharId,
    measurand: Option<Measurand>,
    evaluation_basis: Option<EvaluationBasis>,
    value: Option<Quantity>,
    uncertainty: Option<Uncertainty>,
    decision: Option<RecordDecision>,
    spec: Option<GoverningSpec>,
    algorithm: Option<AlgorithmRef>,
    kernel_version: Option<KernelVersion>,
    input_digests: Option<InputDigests>,
    journal_entry: Option<Digest>,
    dmt_row: Option<DmtKey>,
    operator_chain_digest: Option<Digest>,
    dmt_digest: Option<Digest>,
    error: Option<RecordError>,
    interp: I,
}

fn put<T: PartialEq>(slot: &mut Option<T>, v: T, changed: RecordError, error: &mut Option<RecordError>) {
    match slot {
        Some(old) if *old != v => {
            if error.is_none() {
                *error = Some(changed);
            }
        }
        Some(_) => {}
        None => *slot = Some(v),
    }
}

fn need<T>(v: Option<T>, field: &'static str) -> Result<T, RecordError> {
    v.ok_or(RecordError::IncompleteResult { field })
}

impl ResultRecordBuilder<Uninterpreted> {
    /// Starts the record of `characteristic` in `run`.
    pub fn new(run: RunId, characteristic: CharId) -> ResultRecordBuilder {
        ResultRecordBuilder {
            run,
            characteristic,
            measurand: None,
            evaluation_basis: None,
            value: None,
            uncertainty: None,
            decision: None,
            spec: None,
            algorithm: None,
            kernel_version: None,
            input_digests: None,
            journal_entry: None,
            dmt_row: None,
            operator_chain_digest: None,
            dmt_digest: None,
            error: None,
            interp: Uninterpreted,
        }
    }

    /// The interpretation block (`GDT.CHAR.INTERP`): `rows` are the resolved DMT rows the
    /// evaluation used (primary row plus datum-frame, size and material-condition rows),
    /// `profile` the characteristic's defaults profile, `register` the status register view
    /// (verified for [`Purpose::Evaluate`]; any other view labels every rule `provisional`).
    /// Each digest is recomputed here from the row or profile; no digest is taken from the
    /// caller. Rows over [`MAX_RULE_ROWS`](crate::chars::interp::MAX_RULE_ROWS) are
    /// `invalid_field{interpretation_rules}`. [`ResultRecordBuilder::finish`] checks the rest.
    pub fn interpretation(
        self,
        rows: &[&DmtRow],
        profile: Option<&ProfileDoc>,
        register: &RegisterView,
    ) -> ResultRecordBuilder<Interpreted> {
        self.interpretation_for(rows, profile, register, Purpose::Evaluate)
    }

    /// [`ResultRecordBuilder::interpretation`] for an evaluation needing a register verified
    /// for `purpose`: replay passes [`Purpose::Replay`] with the view of the pinned register
    /// (`GDT.DIM.RADIAL`'s exact replay, as `GDT.DRF.BUILD`'s), so a record built against a
    /// register that was the latest when it was evaluated replays to the same bytes.
    pub(crate) fn interpretation_for(
        self,
        rows: &[&DmtRow],
        profile: Option<&ProfileDoc>,
        register: &RegisterView,
        purpose: Purpose,
    ) -> ResultRecordBuilder<Interpreted> {
        let interp = Interpreted::compute(rows, profile, register, purpose);
        ResultRecordBuilder {
            run: self.run,
            characteristic: self.characteristic,
            measurand: self.measurand,
            evaluation_basis: self.evaluation_basis,
            value: self.value,
            uncertainty: self.uncertainty,
            decision: self.decision,
            spec: self.spec,
            algorithm: self.algorithm,
            kernel_version: self.kernel_version,
            input_digests: self.input_digests,
            journal_entry: self.journal_entry,
            dmt_row: self.dmt_row,
            operator_chain_digest: self.operator_chain_digest,
            dmt_digest: self.dmt_digest,
            error: self.error,
            interp,
        }
    }
}

impl ResultRecordBuilder<Uninterpreted> {
    /// Test hook: a builder holding a hand-made block (the signed_off branch no register can
    /// reach today).
    #[cfg(test)]
    pub(crate) fn with_interpreted(self, interp: Interpreted) -> ResultRecordBuilder<Interpreted> {
        ResultRecordBuilder {
            run: self.run,
            characteristic: self.characteristic,
            measurand: self.measurand,
            evaluation_basis: self.evaluation_basis,
            value: self.value,
            uncertainty: self.uncertainty,
            decision: self.decision,
            spec: self.spec,
            algorithm: self.algorithm,
            kernel_version: self.kernel_version,
            input_digests: self.input_digests,
            journal_entry: self.journal_entry,
            dmt_row: self.dmt_row,
            operator_chain_digest: self.operator_chain_digest,
            dmt_digest: self.dmt_digest,
            error: self.error,
            interp,
        }
    }
}

impl<I> ResultRecordBuilder<I> {
    fn fail(&mut self, e: RecordError) {
        if self.error.is_none() {
            self.error = Some(e);
        }
    }

    /// The measurand.
    pub fn measurand(mut self, m: Measurand) -> Self {
        put(&mut self.measurand, m, RecordError::FieldChanged { field: "measurand" }, &mut self.error);
        self
    }

    /// The evaluation basis as the evaluating child declares it. A later different basis is
    /// `basis_changed` (REQ-GDT.CHAR.RECORD-005); the first one stays.
    pub fn evaluation_basis(mut self, b: EvaluationBasis) -> Self {
        put(&mut self.evaluation_basis, b, RecordError::BasisChanged, &mut self.error);
        self
    }

    /// The evaluated value with its unit.
    pub fn value(mut self, q: Quantity) -> Self {
        put(&mut self.value, q, RecordError::FieldChanged { field: "value" }, &mut self.error);
        self
    }

    /// The uncertainty, from the UNC statement ([`Uncertainty::of`]) or a bare UEC-0
    /// ([`Uncertainty::uec0`]). A second, different uncertainty is `uec0_with_value` when
    /// one side is UEC-0 and the other carries a number (REQ-GDT.CHAR.RECORD-004), else
    /// `uncertainty_changed`.
    pub fn uncertainty(mut self, u: Uncertainty) -> Self {
        match &self.uncertainty {
            None => self.uncertainty = Some(u),
            Some(old) if *old == u => {}
            Some(old) => {
                let uec0_vs_value = (old.class == EvidenceClass::Uec0 && u.has_value())
                    || (u.class == EvidenceClass::Uec0 && old.has_value());
                self.fail(if uec0_vs_value { RecordError::Uec0WithValue } else { RecordError::UncertaintyChanged });
            }
        }
        self
    }

    /// Binds the decision (REQ-GDT.CHAR.RECORD-002, -003). `bound` is the rule bound to the
    /// characteristic, `unc` what the UNC decision rule returned:
    ///
    /// - no rule, no decision: `NOT_DECIDABLE(no_rule_bound)`;
    /// - a rule, no decision: `NOT_DECIDABLE(unc_no_decision)` naming the rule;
    /// - a rule and a decision: the decision, which must name exactly that rule;
    /// - a decision without a bound rule, or naming another rule: `decision_rule_mismatch`.
    ///
    /// A decision stating `CONFORMS`, `NONCONFORMS` or `INDETERMINATE` must rest on an
    /// evaluated (UEC-1..3) basis and a bound rule (`invalid_decision{basis | unbound_rule}`);
    /// `fid-kdec` already refuses both, the record checks again. There is no other way to
    /// set a decision, and no way to set an outcome.
    pub fn bind_decision(mut self, bound: Option<&RuleRef>, unc: Option<Decision>) -> Self {
        match RecordDecision::bind(bound, unc) {
            Ok(d) => put(&mut self.decision, d, RecordError::FieldChanged { field: "decision" }, &mut self.error),
            Err(e) => self.fail(e),
        }
        self
    }

    /// The governing spec (standard, edition, defaults profile).
    pub fn spec(mut self, s: GoverningSpec) -> Self {
        put(&mut self.spec, s, RecordError::FieldChanged { field: "spec" }, &mut self.error);
        self
    }

    /// The algorithm ID and version.
    pub fn algorithm(mut self, a: AlgorithmRef) -> Self {
        put(&mut self.algorithm, a, RecordError::FieldChanged { field: "algorithm" }, &mut self.error);
        self
    }

    /// The kernel version.
    pub fn kernel_version(mut self, v: KernelVersion) -> Self {
        put(&mut self.kernel_version, v, RecordError::FieldChanged { field: "kernel_version" }, &mut self.error);
        self
    }

    /// The input digests.
    pub fn input_digests(mut self, d: InputDigests) -> Self {
        put(&mut self.input_digests, d, RecordError::FieldChanged { field: "input_digests" }, &mut self.error);
        self
    }

    /// The journal entry hash.
    pub fn journal_entry(mut self, d: Digest) -> Self {
        put(&mut self.journal_entry, d, RecordError::FieldChanged { field: "journal_entry" }, &mut self.error);
        self
    }

    /// The DMT row ID (from `GDT.RES`).
    pub fn dmt_row(mut self, k: DmtKey) -> Self {
        put(&mut self.dmt_row, k, RecordError::FieldChanged { field: "dmt_row" }, &mut self.error);
        self
    }

    /// The operator-chain digest (from `GDT.RES`).
    pub fn operator_chain_digest(mut self, d: Digest) -> Self {
        let changed = RecordError::FieldChanged { field: "operator_chain_digest" };
        put(&mut self.operator_chain_digest, d, changed, &mut self.error);
        self
    }

    /// The DMT digest (from `GDT.RES`).
    pub fn dmt_digest(mut self, d: Digest) -> Self {
        put(&mut self.dmt_digest, d, RecordError::FieldChanged { field: "dmt_digest" }, &mut self.error);
        self
    }

    /// The three DMT identifiers of a resolved chain at once.
    pub fn resolved(self, r: &fid_gps::resolve::Resolved) -> Self {
        self.dmt_row(r.row().clone()).operator_chain_digest(r.chain_digest()).dmt_digest(r.dmt_digest())
    }
}

impl ResultRecordBuilder<Interpreted> {
    /// Finishes the record. In order: the first error kept by a setter; the first missing
    /// field (`incomplete_result{field}`) in the order `measurand`, `evaluation_basis`,
    /// `value`, `uncertainty`, `decision`, `spec`, `algorithm`, `kernel_version`,
    /// `input_digests`, `journal_entry`, `dmt_row`, `operator_chain_digest`, `dmt_digest`;
    /// `unit_mismatch` when the statement's unit differs from the value's; and
    /// `decision_basis_mismatch` when the UNC decision rests on a statement other than the
    /// record's, or rests on none while the record carries one (except `NOT_DECIDABLE` with
    /// `rule_missing` or `label_only`; GDT's own `no_rule_bound` and `unc_no_decision` carry
    /// no UNC decision and so no basis). Then the interpretation checks of `GDT.CHAR.INTERP`:
    /// `incomplete_result{interpretation_rules}` when no rows were passed, the primary row
    /// (`dmt_row`) is not among them, or the defaults profile is missing or is not the spec's;
    /// `rule_not_admitted{rule}` for a `draft` rule. Then the canonical JSON and its digest.
    pub fn finish(self) -> Result<ResultRecord, RecordError> {
        if let Some(e) = self.error {
            return Err(e);
        }
        let measurand = need(self.measurand, "measurand")?;
        let evaluation_basis = need(self.evaluation_basis, "evaluation_basis")?;
        let value = need(self.value, "value")?;
        let uncertainty = need(self.uncertainty, "uncertainty")?;
        let decision = need(self.decision, "decision")?;
        let spec = need(self.spec, "spec")?;
        let algorithm = need(self.algorithm, "algorithm")?;
        let kernel_version = need(self.kernel_version, "kernel_version")?;
        let input_digests = need(self.input_digests, "input_digests")?;
        let journal_entry = need(self.journal_entry, "journal_entry")?;
        let dmt_row = need(self.dmt_row, "dmt_row")?;
        let operator_chain_digest = need(self.operator_chain_digest, "operator_chain_digest")?;
        let dmt_digest = need(self.dmt_digest, "dmt_digest")?;
        if uncertainty.unit.as_deref().is_some_and(|u| u != value.unit()) {
            return Err(RecordError::UnitMismatch);
        }
        if let Some(d) = decision.unc() {
            match d.basis() {
                Some(b) if uncertainty.statement != Some(b.statement()) => {
                    return Err(RecordError::DecisionBasisMismatch);
                }
                Some(_) => {}
                // A UNC decision that ignores the record's statement; only the reasons that
                // need no uncertainty (`rule_missing`, `label_only`) may come without one.
                None if uncertainty.statement.is_some()
                    && !matches!(d.outcome(), Outcome::NotDecidable(UncReason::RuleMissing | UncReason::LabelOnly)) =>
                {
                    return Err(RecordError::DecisionBasisMismatch);
                }
                None => {}
            }
        }
        let block = self.interp.finish(dmt_row.as_str(), operator_chain_digest, spec.defaults_profile())?;
        let mut record = ResultRecord {
            run: self.run,
            characteristic: self.characteristic,
            measurand,
            evaluation_basis,
            value,
            uncertainty,
            decision,
            spec,
            algorithm,
            kernel_version,
            input_digests,
            journal_entry,
            dmt_row,
            operator_chain_digest,
            dmt_digest,
            interpretation: block.interpretation,
            interpretation_rules: block.rules,
            interpretation_register: block.register,
            interpretation_pin: block.pin,
            canonical: Vec::new(),
            digest: Digest::TOMBSTONE,
        };
        let canonical =
            fid_canon::to_canonical_bytes(&record.to_cvalue()).map_err(|_| RecordError::CanonicalEncoding)?;
        record.digest = Digest::from_bytes(fid_canon::digest_bytes(&canonical).0);
        record.canonical = canonical;
        Ok(record)
    }
}
