//! Certificate type, certify pipeline and digest rule (spec node KRN.SOLV.CERT.CT).
//!
//! [`certify`] turns a converged conic solution into a [`Certified`] result or a typed
//! `certificate_failed` error:
//!
//! 1. **Checks**, in this order: the recomputation config and the KKT tolerances
//!    (`InvalidInput { field: "config" }` / `{ field: "tolerances" }`), and
//!    [`ActiveSetProblem::data_scale`] (its own error, `data_scale_unset` by default, or
//!    `InvalidInput { field: "data_scale" }` for a value that is not finite and positive). These
//!    fail the call; they are not candidate failures.
//! 2. **Extraction** ([`super::active::extract`], KRN.SOLV.CERT.AS) with the problem's
//!    [`ActiveSetProblem::row_labels`] as the labels of the conic rows **in that order** (row `i`
//!    of the conic problem is the constraint labelled `row_labels()[i]`) and
//!    [`ActiveSetProblem::expected_active`] as the set size. Its errors (including
//!    `ActiveSetUndetermined`) fail the call.
//! 3. **Candidates in tie-break order** (REQ-KRN.SOLV.CERT.CT-001): for each candidate set,
//!    [`super::recomp::recompute`] then [`super::kkt::check`]. The first candidate whose
//!    recomputation succeeds and whose KKT report is [`KktStatus::Certified`] with every one of
//!    the four conditions [`Verdict::Satisfied`] is certified. A candidate whose recomputation
//!    or check returns an error, or whose report has any condition `Violated` or `Undecidable`,
//!    is not certified (a certificate is proven, never estimated) and the next one is tried.
//! 4. **None passes** (REQ-002): [`CertError::CertificateFailed`] with the number of candidates
//!    tried and the first candidate's outcome: its KKT report, or the error that stopped it
//!    before a report existed. No value is returned.
//!
//! # The certificate
//!
//! A [`Certificate`] holds everything a verifier needs to repeat the decision: the active set,
//! the recomputed parameters and value with their proven bounds, the condition estimate, the
//! multipliers (`G^T lambda = -grad f`, the sign convention of KRN.SOLV.CERT.KKT) with their
//! proven bounds, every KKT condition's proven enclosure `[lo, hi]` and threshold, the per-row
//! dual thresholds `tol_d * lo||grad f|| / hi||grad g_i||` (never only their minimum) with the
//! per-row `hi||grad g_i||` they divide by, every tolerance and scale, whether the problem is
//! linear, the extraction thresholds, the flags and the algorithm ID.
//!
//! **Iterate exclusion** (REQ-003). No field comes from a solver iterate: the recomputation and
//! the KKT check read only the candidate set and the problem data, and the extraction
//! contributes only the caller's relative thresholds. Which and how many candidate sets the
//! extraction listed depends on the solution's slacks, so `tie_broken` (more than one
//! candidate) is returned on the in-memory [`Certified`] as a diagnostic and is **not** part of
//! the certificate or its digest. The extraction's scales (`max |y|` is an iterate) and
//! diagnostic row lists, the conic `x`, `y`, `s`, objective, iteration count and residuals, and
//! which candidate index won, are not recorded either.
//!
//! **Self-consistency** ([`Certificate::check_consistency`]) is checked before a certificate
//! is returned and when one is parsed: lengths, finiteness, ordering, `lo <= hi <= threshold`
//! for primal feasibility, stationarity and complementarity, each of their thresholds equal to
//! `tol * scale` as KRN.SOLV.CERT.KKT forms it, and the dual condition recomputed row by row
//! from the recorded multipliers, bounds and per-row thresholds, each per-row threshold
//! recomputed bit for bit as `down(down(tol_d * gradient_lo) / ghi_i)` (0 when the rounded
//! numerator is not positive, in particular when `gradient_lo` is 0), and `steps` matching
//! `is_linear` (1, or `gn_steps`). It does not need the problem
//! data; checking the recorded values against the data is the verifier's part (`TRUST`).
//!
//! # Serialisation and digest (schema `fid.cert@1`)
//!
//! The bytes of `fid.cert@1` are defined by `fid-kci`'s kernel canonical form (RFC 8785 JSON
//! restricted to strings, arrays and objects: keys in UTF-16 order, no whitespace), and the
//! digest is computed over those bytes. Every value is a string: floats in the
//! CORE.JRN.CANON `F64Bits` form `0x` followed by the 16 lowercase hex digits of their
//! IEEE-754 bits (only that form is parsed), integers in decimal, booleans as `"true"` /
//! `"false"` (the kernel writer has no JSON booleans), row labels as `"point:side"`. The digest
//! (REQ-003) is the SHA-256 of the canonical bytes of every field except `digest`; the
//! serialised certificate is that object plus `"digest": "sha256:..."`.
//! [`Certificate::parse`] accepts only the canonical bytes (REQ-006: serialise, parse,
//! serialise gives identical bytes) and checks the digest.
//!
//! # Determinism
//!
//! D0. No FMA, no `f64::min`/`max` (total-order helpers of `fid_num::f`), no hash containers,
//! no I/O; the digest is computed by `fid-kci`'s portable SHA-256. So a certificate and its
//! bytes are bit-identical on x86-64, ARM64 and wasm32.

mod wire;

use fid_num::f::{total_max, total_min};
use fid_types::Digest;

use super::active::{extract, Thresholds};
use super::kkt::{check, dual_threshold, KktReport, KktScales, KktStatus, KktTolerances, Verdict};
use super::recomp::interval::{next_down, next_up};
use super::recomp::{recompute, ActiveSetProblem, RecompConfig, Recomputed};
use super::{CertError, RowLabel};
use crate::conic::{ConicProblem, ConicSolution};

/// The schema of the serialised certificate.
pub const SCHEMA: &str = "fid.cert@1";

/// The algorithm ID recorded in every certificate: this certify pipeline (extraction,
/// recomputation, proven KKT check), version 1.
pub const ALGORITHM: &str = "KRN.SOLV.CERT@1";

/// Default relative extraction thresholds (`slack_rel`, `mult_rel`) of [`CertConfig::default`]
/// ([unverified]).
pub const DEFAULT_THRESHOLD_REL: f64 = 1e-6;

/// What the certified optimum is, supplied by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertKind {
    /// Chebyshev (minimax) fit.
    Chebyshev,
    /// Envelope (one-sided, tangent) fit.
    Envelope,
    /// Tangential fit.
    Tangential,
    /// Minimum zone.
    Zone,
}

impl CertKind {
    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            CertKind::Chebyshev => "chebyshev",
            CertKind::Envelope => "envelope",
            CertKind::Tangential => "tangential",
            CertKind::Zone => "zone",
        }
    }

    fn from_wire(s: &str) -> Option<CertKind> {
        [CertKind::Chebyshev, CertKind::Envelope, CertKind::Tangential, CertKind::Zone]
            .into_iter()
            .find(|k| k.as_str() == s)
    }
}

/// Constants of [`certify`]: the extraction thresholds, the recomputation config and the KKT
/// tolerances. All of them are recorded in the certificate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CertConfig {
    /// Relative active-set extraction thresholds (KRN.SOLV.CERT.AS).
    pub thresholds: Thresholds,
    /// Recomputation constants (KRN.SOLV.CERT.RECOMP).
    pub recomp: RecompConfig,
    /// KKT tolerances (KRN.SOLV.CERT.KKT).
    pub kkt: KktTolerances,
}

impl Default for CertConfig {
    fn default() -> CertConfig {
        CertConfig::new(Thresholds { slack_rel: DEFAULT_THRESHOLD_REL, mult_rel: DEFAULT_THRESHOLD_REL })
    }
}

impl CertConfig {
    /// The given extraction thresholds with the default recomputation config and tolerances.
    pub fn new(thresholds: Thresholds) -> CertConfig {
        CertConfig { thresholds, recomp: RecompConfig::default(), kkt: KktTolerances::default() }
    }
}

/// Flags of a certificate.
///
/// A KKT certificate proves a first-order (KKT) point. It implies a **global** optimum only for
/// a linear or convex problem (see [`Certificate::is_linear`]); for a nonconvex problem it
/// proves a KKT point, which may be a local optimum only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CertFlags {
    /// Set by a multi-start caller ([`Certificate::set_local_optimum_only`]) when it observed
    /// that its starts disagreed (KRN.SOLV.MSTART, KRN.SOLV.SEQ). It signals that observed
    /// disagreement; its absence is not a proof of global optimality.
    pub local_optimum_only: bool,
}

/// One KKT condition as recorded: the proven enclosure `[lo, hi]` of its exact residual and
/// its absolute threshold. In a certificate every condition is satisfied. For the dual
/// condition `threshold` is the smallest per-row threshold and the decision is per row (see
/// [`KktRecord::dual_thresholds`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordedCondition {
    /// Proven lower bound of the exact residual.
    pub lo: f64,
    /// Proven upper bound of the exact residual.
    pub hi: f64,
    /// The absolute threshold.
    pub threshold: f64,
}

/// The KKT part of a certificate: every residual, threshold and scale of the passing report.
#[derive(Debug, Clone, PartialEq)]
pub struct KktRecord {
    /// Primal feasibility, `max_i g_i(p*)` over every row; threshold `tol_p * data`.
    pub primal: RecordedCondition,
    /// Dual feasibility, `max(0, -min_i lambda*_i)`; threshold the smallest per-row threshold.
    pub dual: RecordedCondition,
    /// Stationarity at the recorded multipliers; threshold `tol_s * gradient`.
    pub stationarity: RecordedCondition,
    /// Complementarity, `max |g_i(p*)|` over the active rows; threshold `tol_c * data`.
    pub complementarity: RecordedCondition,
    /// The per-row dual thresholds `down(down(tol_d * gradient_lo) / gradient_norms_hi_i)`
    /// (0 when the rounded numerator is not positive), in active-set order. Row `i` passes when
    /// `lambda_i - multiplier_bounds_i` (rounded down) is at least `-dual_thresholds_i`.
    pub dual_thresholds: Vec<f64>,
    /// The proven upper bounds `hi||grad g_i||` of the active rows' constraint gradient norms,
    /// in active-set order: the divisors of [`Self::dual_thresholds`].
    pub gradient_norms_hi: Vec<f64>,
    /// The tolerances.
    pub tolerances: KktTolerances,
    /// The scales.
    pub scales: KktScales,
}

/// A certificate (schema `fid.cert@1`). Made only by [`certify`] and [`Certificate::parse`];
/// the only change after that is [`Certificate::set_local_optimum_only`].
#[derive(Debug, Clone, PartialEq)]
pub struct Certificate {
    kind: CertKind,
    active_set: Vec<RowLabel>,
    multipliers: Vec<f64>,
    multiplier_bounds: Vec<f64>,
    kkt: KktRecord,
    thresholds: Thresholds,
    recomp: RecompConfig,
    cond: f64,
    steps: u32,
    is_linear: bool,
    flags: CertFlags,
    params: Vec<f64>,
    param_bounds: Vec<f64>,
    value: f64,
    value_bound: f64,
    digest: Digest,
}

/// A certified result: the recomputed parameters and value, their certificate, and the
/// in-memory diagnostic `tie_broken` (not part of the certificate or its digest).
#[derive(Debug, Clone, PartialEq)]
pub struct Certified {
    certificate: Certificate,
    tie_broken: bool,
}

impl Certified {
    /// The recomputed parameters (equal to the certificate's).
    pub fn params(&self) -> &[f64] {
        &self.certificate.params
    }
    /// The recomputed objective value (equal to the certificate's).
    pub fn value(&self) -> f64 {
        self.certificate.value
    }
    /// The certificate.
    pub fn certificate(&self) -> &Certificate {
        &self.certificate
    }
    /// More than one candidate set existed, so the tie-break order chose (REQ-KRN.SOLV.CERT-004).
    /// A diagnostic of this run: it depends on the solution's slacks, so it is not part of the
    /// certificate or its digest (REQ-KRN.SOLV.CERT-005).
    pub fn tie_broken(&self) -> bool {
        self.tie_broken
    }
    /// The certificate, by value.
    pub fn into_certificate(self) -> Certificate {
        self.certificate
    }
    /// [`Certificate::set_local_optimum_only`] on the certificate.
    ///
    /// # Errors
    ///
    /// As [`Certificate::set_local_optimum_only`].
    pub fn set_local_optimum_only(&mut self) -> Result<(), CertError> {
        self.certificate.set_local_optimum_only()
    }
}

/// Certifies the optimum of `problem` that the converged `solution` of its conic form `conic`
/// points to. See the module docs for the pinned procedure. Row `i` of `conic` must be the
/// constraint labelled `problem.row_labels()[i]`.
///
/// # Errors
///
/// Config, tolerance and data-scale errors, every error of the extraction (including
/// [`CertError::ActiveSetUndetermined`]), [`CertError::CertificateFailed`] when no candidate
/// passes, and [`CertError::Inconsistent`] if a passing report would give an inconsistent
/// certificate (a defect; fail closed).
pub fn certify<P: ActiveSetProblem + ?Sized>(
    problem: &P,
    conic: &ConicProblem,
    solution: &ConicSolution,
    kind: CertKind,
    cfg: &CertConfig,
) -> Result<Certified, CertError> {
    cfg.recomp.check()?;
    cfg.kkt.check()?;
    let data_scale = problem.data_scale()?;
    if !(data_scale.is_finite() && data_scale > 0.0) {
        return Err(CertError::InvalidInput { field: "data_scale" });
    }
    let labels = problem.row_labels();
    let proposal = extract(conic, solution, &labels, problem.expected_active(), &cfg.thresholds)?;
    let count = proposal.sets.len();
    let (mut first_report, mut first_error) = (None, None);
    for idx in 0..count {
        let outcome = recompute(problem, &proposal, idx, &cfg.recomp)
            .and_then(|rec| check(problem, &rec, &cfg.kkt).map(|rep| (rec, rep)));
        match outcome {
            Ok((rec, rep)) if passes(&rep) => {
                let certificate = Certificate::build(kind, &rec, &rep, cfg, problem.is_linear())?;
                return Ok(Certified { certificate, tie_broken: proposal.tie_broken });
            }
            Ok((_, rep)) => {
                if idx == 0 {
                    first_report = Some(Box::new(rep));
                }
            }
            Err(e) => {
                if idx == 0 {
                    first_error = Some(Box::new(e));
                }
            }
        }
    }
    Err(CertError::CertificateFailed { tried: count, first_report, first_error })
}

/// A report passes only when it is certified and every condition is proven satisfied.
fn passes(rep: &KktReport) -> bool {
    rep.pass()
        && rep.status() == KktStatus::Certified
        && rep.reason().is_none()
        && [rep.primal(), rep.dual(), rep.stationarity(), rep.complementarity()]
            .iter()
            .all(|c| c.verdict() == Verdict::Satisfied)
}

fn recorded(c: super::kkt::Condition) -> RecordedCondition {
    RecordedCondition { lo: c.lo(), hi: c.hi(), threshold: c.threshold() }
}

fn inconsistent(detail: &'static str) -> CertError {
    CertError::Inconsistent { detail }
}

impl Certificate {
    fn build(
        kind: CertKind,
        rec: &Recomputed,
        rep: &KktReport,
        cfg: &CertConfig,
        is_linear: bool,
    ) -> Result<Certificate, CertError> {
        let mut c = Certificate {
            kind,
            active_set: rec.set().to_vec(),
            multipliers: rep.multipliers().to_vec(),
            multiplier_bounds: rep.multiplier_bounds().to_vec(),
            kkt: KktRecord {
                primal: recorded(rep.primal()),
                dual: recorded(rep.dual()),
                stationarity: recorded(rep.stationarity()),
                complementarity: recorded(rep.complementarity()),
                dual_thresholds: rep.dual_thresholds().to_vec(),
                gradient_norms_hi: rep.gradient_norms_hi().to_vec(),
                tolerances: rep.tolerances(),
                scales: rep.scales(),
            },
            thresholds: cfg.thresholds,
            recomp: cfg.recomp,
            cond: rec.cond(),
            steps: rec.steps(),
            is_linear,
            flags: CertFlags { local_optimum_only: false },
            params: rec.params().to_vec(),
            param_bounds: rec.param_bounds().to_vec(),
            value: rec.value(),
            value_bound: rec.value_bound(),
            digest: Digest::TOMBSTONE,
        };
        c.check_consistency()?;
        c.digest = c.compute_digest()?;
        Ok(c)
    }

    /// The certified kind.
    pub fn kind(&self) -> CertKind {
        self.kind
    }
    /// The active set, sorted ascending.
    pub fn active_set(&self) -> &[RowLabel] {
        &self.active_set
    }
    /// The multipliers (`G^T lambda = -grad f`), one per active row in set order.
    pub fn multipliers(&self) -> &[f64] {
        &self.multipliers
    }
    /// Proven bounds `|multipliers[i] - lambda*_i| <= multiplier_bounds[i]`.
    pub fn multiplier_bounds(&self) -> &[f64] {
        &self.multiplier_bounds
    }
    /// The KKT residuals, thresholds, tolerances and scales.
    pub fn kkt(&self) -> &KktRecord {
        &self.kkt
    }
    /// The per-row dual thresholds (also in [`Self::kkt`]).
    pub fn dual_thresholds(&self) -> &[f64] {
        &self.kkt.dual_thresholds
    }
    /// The relative extraction thresholds.
    pub fn thresholds(&self) -> Thresholds {
        self.thresholds
    }
    /// The recomputation config.
    pub fn recomp(&self) -> RecompConfig {
        self.recomp
    }
    /// Condition estimate of the active-set system.
    pub fn cond(&self) -> f64 {
        self.cond
    }
    /// QR solves of the recomputation.
    pub fn steps(&self) -> u32 {
        self.steps
    }
    /// Whether the problem's active-set equations are linear (`ActiveSetProblem::is_linear`).
    /// For a linear program the KKT certificate proves a global optimum.
    pub fn is_linear(&self) -> bool {
        self.is_linear
    }
    /// The per-row `hi||grad g_i||` (also in [`Self::kkt`]).
    pub fn gradient_norms_hi(&self) -> &[f64] {
        &self.kkt.gradient_norms_hi
    }
    /// The flags.
    pub fn flags(&self) -> CertFlags {
        self.flags
    }
    /// The recomputed parameters.
    pub fn params(&self) -> &[f64] {
        &self.params
    }
    /// Proven bounds `|params[i] - p*_i| <= param_bounds[i]`.
    pub fn param_bounds(&self) -> &[f64] {
        &self.param_bounds
    }
    /// The recomputed objective value.
    pub fn value(&self) -> f64 {
        self.value
    }
    /// Proven bound `|value - f(p*)| <= value_bound`.
    pub fn value_bound(&self) -> f64 {
        self.value_bound
    }
    /// The algorithm ID ([`ALGORITHM`]).
    pub fn algorithm(&self) -> &'static str {
        ALGORITHM
    }
    /// The schema ([`SCHEMA`]).
    pub fn schema(&self) -> &'static str {
        SCHEMA
    }
    /// The digest: SHA-256 of the canonical JSON of every other field.
    pub fn digest(&self) -> &Digest {
        &self.digest
    }

    /// Sets `local_optimum_only` and recomputes the digest; no other field changes
    /// (REQ-KRN.SOLV.CERT.CT-004). Idempotent.
    ///
    /// # Errors
    ///
    /// [`CertError::Inconsistent`] if the canonical writer fails (a defect).
    pub fn set_local_optimum_only(&mut self) -> Result<(), CertError> {
        let mut next = self.clone();
        next.flags.local_optimum_only = true;
        next.digest = next.compute_digest()?;
        *self = next;
        Ok(())
    }

    /// The serialised certificate: canonical JSON of every field including `digest`.
    ///
    /// # Errors
    ///
    /// [`CertError::Inconsistent`] if the canonical writer fails (a defect).
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, CertError> {
        wire::canonical(&wire::to_json(self, true))
    }

    /// Parses a serialised certificate. Only the canonical bytes are accepted; the certificate
    /// must be self-consistent ([`Self::check_consistency`]) and its digest must match.
    ///
    /// # Errors
    ///
    /// [`CertError::Malformed`] (syntax, schema, members, values, `digest_mismatch`,
    /// `non_canonical`) and [`CertError::Inconsistent`].
    pub fn parse(input: &[u8]) -> Result<Certificate, CertError> {
        let c = wire::from_bytes(input)?;
        c.check_consistency()?;
        if c.compute_digest()? != c.digest {
            return Err(CertError::Malformed { detail: "digest_mismatch" });
        }
        if c.to_canonical_json()?.as_slice() != input {
            return Err(CertError::Malformed { detail: "non_canonical" });
        }
        Ok(c)
    }

    fn compute_digest(&self) -> Result<Digest, CertError> {
        let body = wire::canonical(&wire::to_json(self, false))?;
        Ok(fid_kci::kci::sha256_digest(&body))
    }

    /// Checks the certificate's recorded values against each other (see the module docs).
    /// It does not read the problem data.
    ///
    /// # Errors
    ///
    /// [`CertError::Inconsistent`] naming the first failed check.
    pub fn check_consistency(&self) -> Result<(), CertError> {
        let n = self.params.len();
        if n == 0 {
            return Err(inconsistent("params"));
        }
        let lens = [
            self.active_set.len(),
            self.multipliers.len(),
            self.multiplier_bounds.len(),
            self.param_bounds.len(),
            self.kkt.dual_thresholds.len(),
            self.kkt.gradient_norms_hi.len(),
        ];
        if lens.iter().any(|&l| l != n) {
            return Err(inconsistent("length"));
        }
        if self.active_set.windows(2).any(|w| matches!(w, [a, b] if a >= b)) {
            return Err(inconsistent("active_set"));
        }
        let k = &self.kkt;
        let conds = [k.primal, k.dual, k.stationarity, k.complementarity];
        let scalars = [
            self.value,
            self.value_bound,
            self.cond,
            k.scales.data,
            k.scales.gradient,
            k.scales.gradient_lo,
            self.thresholds.slack_rel,
            self.thresholds.mult_rel,
        ];
        let cond_ends: Vec<f64> = conds.iter().flat_map(|c| [c.lo, c.hi, c.threshold]).collect();
        let finite = self
            .params
            .iter()
            .chain(&self.param_bounds)
            .chain(&self.multipliers)
            .chain(&self.multiplier_bounds)
            .chain(&k.dual_thresholds)
            .chain(&k.gradient_norms_hi)
            .chain(&scalars)
            .chain(&cond_ends)
            .all(|v| v.is_finite());
        if !finite {
            return Err(inconsistent("not_finite"));
        }
        let nonneg = |xs: &[f64]| xs.iter().all(|&v| v >= 0.0);
        let bounds_ok = nonneg(&self.param_bounds) && nonneg(&self.multiplier_bounds) && nonneg(&k.dual_thresholds);
        if !bounds_ok || self.value_bound < 0.0 {
            return Err(inconsistent("negative_bound"));
        }
        if !(self.thresholds.slack_rel >= 0.0 && self.thresholds.mult_rel >= 0.0) {
            return Err(inconsistent("thresholds"));
        }
        self.recomp.check().map_err(|_| inconsistent("recomp"))?;
        k.tolerances.check().map_err(|_| inconsistent("tolerances"))?;
        let want_steps = if self.is_linear { 1 } else { self.recomp.gn_steps };
        if self.steps != want_steps {
            return Err(inconsistent("steps"));
        }
        if !(self.cond > 0.0 && self.cond <= self.recomp.cond_max) {
            return Err(inconsistent("cond"));
        }
        if !(k.scales.data > 0.0 && k.scales.gradient > 0.0 && k.scales.gradient_lo >= 0.0) {
            return Err(inconsistent("scales"));
        }
        let t = k.tolerances;
        let same = |a: f64, b: f64| a.to_bits() == b.to_bits();
        let decided = |c: RecordedCondition, thr: f64, floor: f64| {
            c.lo >= floor && c.lo <= c.hi && c.hi <= c.threshold && same(c.threshold, thr)
        };
        if !decided(k.primal, t.tol_p * k.scales.data, f64::NEG_INFINITY) {
            return Err(inconsistent("primal"));
        }
        if !decided(k.stationarity, t.tol_s * k.scales.gradient, 0.0) {
            return Err(inconsistent("stationarity"));
        }
        if !decided(k.complementarity, t.tol_c * k.scales.data, 0.0) {
            return Err(inconsistent("complementarity"));
        }
        if k.gradient_norms_hi.iter().any(|&g| g <= 0.0) {
            return Err(inconsistent("gradient_norms_hi"));
        }
        let thresholds_ok = k
            .dual_thresholds
            .iter()
            .zip(&k.gradient_norms_hi)
            .all(|(&thr, &ghi)| same(thr, dual_threshold(t.tol_d, k.scales.gradient_lo, ghi)));
        if !thresholds_ok {
            return Err(inconsistent("dual_thresholds"));
        }
        self.check_dual().map_err(|_| inconsistent("dual"))
    }

    /// The dual condition recomputed exactly as KRN.SOLV.CERT.KKT forms it: per row the
    /// multiplier box `[next_down(l - k), next_up(l + k)]`, satisfied when its lower end is at
    /// least `-thr_i`; the recorded enclosure is `[max(0, -min hi_i), max(0, -min lo_i)]` and
    /// the recorded threshold the smallest `thr_i`.
    fn check_dual(&self) -> Result<(), CertError> {
        let bad = inconsistent("dual");
        let (mut min_lo, mut min_hi, mut thr_min) = (f64::INFINITY, f64::INFINITY, f64::INFINITY);
        for ((&l, &kb), &thr) in self.multipliers.iter().zip(&self.multiplier_bounds).zip(&self.kkt.dual_thresholds) {
            let (lo, hi) = (next_down(l - kb), next_up(l + kb));
            if !(lo.is_finite() && hi.is_finite()) || lo < -thr {
                return Err(bad);
            }
            min_lo = total_min(min_lo, lo)?;
            min_hi = total_min(min_hi, hi)?;
            thr_min = total_min(thr_min, thr)?;
        }
        let d = self.kkt.dual;
        let want = [total_max(0.0, -min_hi)?, total_max(0.0, -min_lo)?, thr_min];
        if [d.lo, d.hi, d.threshold].iter().zip(&want).all(|(a, b)| a.to_bits() == b.to_bits()) {
            Ok(())
        } else {
            Err(bad)
        }
    }
}
