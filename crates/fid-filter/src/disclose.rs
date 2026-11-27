//! The excluded-set record and its impact disclosure. Spec node `KRN.OUT.DISC`.
//!
//! An [`ExclusionRecord`] is the audit record of one point removal: the policy's algorithm ID
//! and canonical parameters, the input point count, the sorted excluded index set, the bytes
//! the set digest is computed over (and that digest), the method's [`MethodStatus`] and, when
//! at least one point was excluded, an [`ImpactDisclosure`]: the fitted feature and residual
//! statistics before and after the exclusion and their differences (REQ-KRN.OUT.DISC-001,
//! -002, -003; REQ-KRN.OUT-002, -003). It travels with the fit result into reports and signed
//! evidence, so it can only be built through validation, and every refusal is a typed
//! [`DiscloseError`].
//!
//! # Construction (pinned)
//!
//! [`ExclusionRecord::new`] checks, in this order, returning the first failure:
//!
//! 1. `params` decodes as a canonical [`PolicySpec`] ([`PolicySpec::decode`], whose errors
//!    are [`DiscloseError::Policy`]);
//! 2. `algorithm` is `filter.outlier.<that spec's method>@<any semver>`, else
//!    `Policy(InvalidParameter { field: "algorithm", reason: "method_mismatch" })`. Only
//!    outlier policies have a record today; point selection (`KRN.OUT.SEL`) is not built;
//! 3. the excluded set is a valid [`ExclusionMask`] over `point_count` points (range, then
//!    duplicates; `Policy(IndexOutOfRange)`, `Policy(DuplicateIndex)`); it is stored sorted;
//! 4. the guards of `KRN.OUT.POL` hold for it ([`check_guards`]): a record never describes an
//!    exclusion that [`crate::policy::apply`] would have refused;
//! 5. `status` is consistent: [`MethodStatus::ZeroScale`] means the method decided nothing,
//!    so it must come with an empty set ([`DiscloseError::StatusMismatch`]);
//! 6. an impact is given exactly when at least one point is excluded
//!    ([`DiscloseError::ImpactMissing`], [`DiscloseError::ImpactUnexpected`]): an exclusion is
//!    never recorded without its impact (REQ-002), and an empty one carries none (REQ-003);
//! 7. the impact is built by [`ImpactDisclosure::new`] and its point counts must match the
//!    record: `before` has `point_count` residuals and `after` has
//!    `point_count - excluded_count` ([`DiscloseError::CountMismatch`]), and both features are
//!    of the spec's `target` primitive ([`DiscloseError::PrimitiveMismatch`]).
//!
//! [`ImpactDisclosure::new`] checks, for `before` then `after`: the feature passes
//! `fid_fit`'s [`FittedFeature::canonical`] validation ([`DiscloseError::Fit`]); every
//! direction that `canonical` sign-normalises satisfies the sign rule (first component that is
//! not `±0.0` positive, no `-0.0` component) and every orientation axis (cone, paraboloid) has
//! no `-0.0` component, every direction and axis has unit length
//! (`|red::sum_sq(u) - 1| <= UNIT_LENGTH_TOL`, a few ulp), and a slot's `length` and `width`
//! are not `-0.0` ([`DiscloseError::NotCanonical`]): the record stores the values it is given,
//! so they must already be what `canonical` would produce, and the same geometry has one
//! encoding and the differences compare features in one convention; the statistics are finite, `rms >= 0`, `std_dev >= 0`, `min <= max`,
//! `n >= 1` ([`DiscloseError::StatsInvalid`]). Then `after` must be the same primitive as
//! `before` ([`DiscloseError::PrimitiveMismatch`]) with the same parameter layout (polyline
//! vertex count; spline degree, knot and control counts: [`DiscloseError::ShapeMismatch`]).
//!
//! # Differences (pinned)
//!
//! `delta[i] = after[i] - before[i]`, one IEEE-754 binary64 subtraction per parameter
//! (REQ-KRN.OUT.DISC-002, AC-4), over the feature's numeric parameters in the order of its
//! canonical encoding (`fid_fit::contract::encode`): for a plane `p[0..3]` then `n[0..3]`; a
//! spline's integer `degree` is not a parameter (it must be equal), its knots then its control
//! points are. [`StatsDelta`] holds the same difference for `rms`, `max`, `min`, `mean` and
//! `std_dev` (the counts are recorded, not differenced). A difference that overflows is
//! [`DiscloseError::Num`]. `opposed_directions` names, in encoding order, every direction or
//! axis whose before and after vectors have a negative dot product (`fid_num::red::dot`):
//! there the differences of its components describe a sign flip of the canonical form rather
//! than a small tilt, and the record says so instead of leaving a large delta unexplained.
//!
//! # Encoding (pinned, REQ-KRN.OUT.DISC-004)
//!
//! Compact JSON, ASCII only, keys in the fixed order below (not sorted), shown wrapped:
//!
//! ```text
//! {"format":"filter.exclusion_record@1","algorithm":"<id>","params":<policy spec>,
//!  "point_count":<i>,"excluded_count":<i>,"excluded":[<i>...],
//!  "excluded_digest":"sha256:<64 hex>","status":"<code>","impact":null | <impact>}
//! <impact> = {"before":<fit>,"after":<fit>,"delta":[<f>...],
//!  "stats_delta":{"rms":<f>,"max":<f>,"min":<f>,"mean":<f>,"std_dev":<f>},
//!  "opposed_directions":["<field>"...]}
//! <fit> = {"feature":<feature>,"stats":{"n":<i>,"rms":<f>,"max":<f>,"min":<f>,"mean":<f>,
//!  "std_dev":<f>}}
//! ```
//!
//! `<f>` is an `f64` as a JSON string of exactly 16 lowercase hexadecimal digits, its bit
//! pattern; `<i>` an unsigned integer as a JSON string of decimal digits without leading zeros;
//! `<policy spec>` the canonical [`PolicySpec`] encoding, embedded as is (it is a JSON object);
//! `<feature>` the `"feature"` object of `fid_fit::contract::encode`
//! ([`fid_fit::contract::encode_feature`]); `<code>` the [`MethodStatus`] code. The digest is
//! SHA-256 of [`ExclusionRecord::digest_input`]. Integer formatting, hex digits and field
//! order involve no floating-point arithmetic, so the bytes are the same on every target.

use core::fmt;

use fid_fit::contract::{encode_feature, AlgorithmId, FitError, FittedFeature, Primitive, ResidualStats};
use fid_num::{red, NumError};
use fid_types::Digest;

use crate::policy::{
    check_guards, outlier_algorithm_id, Exclusion, ExclusionMask, MethodStatus, PolicyError, PolicySpec,
};

/// The record format, the first field of the encoding.
pub const RECORD_FORMAT: &str = "filter.exclusion_record@1";

/// One fit of the impact disclosure: the fitted feature and its residual statistics.
pub type FitSnapshot = (FittedFeature, ResidualStats);

/// Which fit of an impact disclosure a refusal is about.
pub const BEFORE: &str = "before";
/// Which fit of an impact disclosure a refusal is about.
pub const AFTER: &str = "after";

/// Why a record or a disclosure was refused. Every variant has a stable
/// [`code`](DiscloseError::code). `which` is [`BEFORE`] or [`AFTER`].
#[derive(Debug, Clone, PartialEq)]
pub enum DiscloseError {
    /// The parameters, the algorithm ID, the excluded set or the guards (see the module
    /// documentation, steps 1 to 4).
    Policy(PolicyError),
    /// A feature failed `fid_fit` validation, or its encoding could not be allocated. Boxed:
    /// `FitError` is large, and this keeps every `Result` of this module small.
    Fit(Box<FitError>),
    /// The method reported `status` with a non-empty excluded set.
    StatusMismatch { status: MethodStatus, excluded: usize },
    /// Points were excluded but no impact disclosure was given (REQ-KRN.OUT.DISC-002).
    ImpactMissing { excluded: usize },
    /// No point was excluded but an impact disclosure was given (REQ-KRN.OUT.DISC-003).
    ImpactUnexpected,
    /// A feature is not the expected primitive.
    PrimitiveMismatch { which: &'static str, expected: Primitive, got: Primitive },
    /// `after` has a different parameter layout than `before` (`field`: `vertices`,
    /// `degree`, `knots` or `control`).
    ShapeMismatch { field: &'static str },
    /// A direction of a feature is not in the canonical sign convention (`field` names it).
    NotCanonical { which: &'static str, field: &'static str },
    /// A residual statistic is not valid (`reason`: `non_finite`, `negative`, `min_above_max`
    /// or `zero`).
    StatsInvalid { which: &'static str, field: &'static str, reason: &'static str },
    /// The residual count of a fit does not match the record's points.
    CountMismatch { which: &'static str, expected: usize, got: usize },
    /// A difference overflowed, or a reduction failed.
    Num(NumError),
    /// A buffer could not be reserved.
    Alloc { op: &'static str },
}

impl DiscloseError {
    /// Stable code.
    pub const fn code(&self) -> &'static str {
        match self {
            DiscloseError::Policy(_) => "policy",
            DiscloseError::Fit(_) => "fit",
            DiscloseError::StatusMismatch { .. } => "status_mismatch",
            DiscloseError::ImpactMissing { .. } => "impact_missing",
            DiscloseError::ImpactUnexpected => "impact_unexpected",
            DiscloseError::PrimitiveMismatch { .. } => "primitive_mismatch",
            DiscloseError::ShapeMismatch { .. } => "shape_mismatch",
            DiscloseError::NotCanonical { .. } => "not_canonical",
            DiscloseError::StatsInvalid { .. } => "stats_invalid",
            DiscloseError::CountMismatch { .. } => "count_mismatch",
            DiscloseError::Num(_) => "num",
            DiscloseError::Alloc { .. } => "alloc_failed",
        }
    }
}

impl fmt::Display for DiscloseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiscloseError::Policy(e) => write!(f, "policy: {e}"),
            DiscloseError::Fit(e) => write!(f, "fit: {}", e.code()),
            DiscloseError::StatusMismatch { status, excluded } => {
                write!(f, "status_mismatch: status {} with {excluded} excluded points", status.code())
            }
            DiscloseError::ImpactMissing { excluded } => {
                write!(f, "impact_missing: {excluded} points excluded and no impact disclosure")
            }
            DiscloseError::ImpactUnexpected => write!(f, "impact_unexpected: nothing excluded"),
            DiscloseError::PrimitiveMismatch { which, expected, got } => {
                write!(f, "primitive_mismatch: {which} is {}, expected {}", got.slug(), expected.slug())
            }
            DiscloseError::ShapeMismatch { field } => write!(f, "shape_mismatch: {field}"),
            DiscloseError::NotCanonical { which, field } => write!(f, "not_canonical: {which}.{field}"),
            DiscloseError::StatsInvalid { which, field, reason } => {
                write!(f, "stats_invalid: {which}.{field}: {reason}")
            }
            DiscloseError::CountMismatch { which, expected, got } => {
                write!(f, "count_mismatch: {which} has {got} residuals, expected {expected}")
            }
            DiscloseError::Num(e) => write!(f, "num: {e}"),
            DiscloseError::Alloc { op } => write!(f, "alloc_failed: {op}"),
        }
    }
}

impl core::error::Error for DiscloseError {}

impl From<PolicyError> for DiscloseError {
    fn from(e: PolicyError) -> Self {
        DiscloseError::Policy(e)
    }
}

impl From<FitError> for DiscloseError {
    fn from(e: FitError) -> Self {
        DiscloseError::Fit(Box::new(e))
    }
}

impl From<NumError> for DiscloseError {
    fn from(e: NumError) -> Self {
        DiscloseError::Num(e)
    }
}

const fn alloc(op: &'static str) -> DiscloseError {
    DiscloseError::Alloc { op }
}

/// After minus before of each residual statistic (one binary64 subtraction each).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatsDelta {
    pub rms: f64,
    pub max: f64,
    pub min: f64,
    pub mean: f64,
    pub std_dev: f64,
}

/// The fitted values with and without the exclusion, and their differences
/// (REQ-KRN.OUT.DISC-002). Fields are private: every value went through
/// [`ImpactDisclosure::new`].
#[derive(Debug, Clone, PartialEq)]
pub struct ImpactDisclosure {
    before: FitSnapshot,
    after: FitSnapshot,
    delta: Vec<f64>,
    stats_delta: StatsDelta,
    opposed: Vec<&'static str>,
}

impl ImpactDisclosure {
    /// Validates both fits and computes the differences (see the
    /// [module documentation](self)).
    pub fn new(before: FitSnapshot, after: FitSnapshot) -> Result<ImpactDisclosure, DiscloseError> {
        check_snapshot(BEFORE, &before)?;
        check_snapshot(AFTER, &after)?;
        let expected = before.0.primitive();
        let got = after.0.primitive();
        if got != expected {
            return Err(DiscloseError::PrimitiveMismatch { which: AFTER, expected, got });
        }
        check_shape(&before.0, &after.0)?;
        let b = parameters(&before.0)?;
        let a = parameters(&after.0)?;
        if a.len() != b.len() {
            // check_shape makes this unreachable; refuse rather than truncate.
            return Err(DiscloseError::ShapeMismatch { field: "parameters" });
        }
        let mut delta = Vec::new();
        delta.try_reserve_exact(b.len()).map_err(|_| alloc("disclose.delta"))?;
        for (x, y) in a.iter().zip(&b) {
            delta.push(diff(*x, *y)?);
        }
        let (sa, sb) = (&after.1, &before.1);
        let stats_delta = StatsDelta {
            rms: diff(sa.rms, sb.rms)?,
            max: diff(sa.max, sb.max)?,
            min: diff(sa.min, sb.min)?,
            mean: diff(sa.mean, sb.mean)?,
            std_dev: diff(sa.std_dev, sb.std_dev)?,
        };
        let mut opposed = Vec::new();
        let (db, da) = (directions(&before.0), directions(&after.0));
        for (bd, ad) in db.iter().flatten().zip(da.iter().flatten()) {
            if red::dot(bd.1, ad.1)? < 0.0 {
                opposed.try_reserve(1).map_err(|_| alloc("disclose.opposed"))?;
                opposed.push(bd.0);
            }
        }
        Ok(ImpactDisclosure { before, after, delta, stats_delta, opposed })
    }

    /// The fit to every input point.
    pub fn before(&self) -> &FitSnapshot {
        &self.before
    }

    /// The fit to the kept points.
    pub fn after(&self) -> &FitSnapshot {
        &self.after
    }

    /// After minus before per feature parameter, in encoding order.
    pub fn delta(&self) -> &[f64] {
        &self.delta
    }

    /// After minus before per residual statistic.
    pub fn stats_delta(&self) -> &StatsDelta {
        &self.stats_delta
    }

    /// The directions and axes whose before and after vectors point into opposite
    /// half-spaces (negative dot product), in encoding order.
    pub fn opposed_directions(&self) -> &[&'static str] {
        &self.opposed
    }

    /// The canonical bytes of the disclosure: the `<impact>` object of the record encoding
    /// (pinned in the [module documentation](self)), on its own.
    pub fn encode(&self) -> Result<Vec<u8>, DiscloseError> {
        let mut o = Out { buf: Vec::new() };
        self.write(&mut o)?;
        Ok(o.buf)
    }

    fn write(&self, o: &mut Out) -> Result<(), DiscloseError> {
        o.put(b"{\"before\":")?;
        o.snapshot(&self.before)?;
        o.put(b",\"after\":")?;
        o.snapshot(&self.after)?;
        o.put(b",\"delta\":[")?;
        for (k, &x) in self.delta.iter().enumerate() {
            if k > 0 {
                o.put(b",")?;
            }
            o.float(x)?;
        }
        let s = &self.stats_delta;
        o.put(b"],\"stats_delta\":{\"rms\":")?;
        o.float(s.rms)?;
        o.put(b",\"max\":")?;
        o.float(s.max)?;
        o.put(b",\"min\":")?;
        o.float(s.min)?;
        o.put(b",\"mean\":")?;
        o.float(s.mean)?;
        o.put(b",\"std_dev\":")?;
        o.float(s.std_dev)?;
        o.put(b"},\"opposed_directions\":[")?;
        for (k, name) in self.opposed.iter().enumerate() {
            if k > 0 {
                o.put(b",")?;
            }
            o.put(b"\"")?;
            o.put(name.as_bytes())?;
            o.put(b"\"")?;
        }
        o.put(b"]}")?;
        Ok(())
    }
}

/// The audit record of one point removal (REQ-KRN.OUT.DISC-001). Fields are private: every
/// value went through [`ExclusionRecord::new`].
#[derive(Debug, Clone, PartialEq)]
pub struct ExclusionRecord {
    algorithm: AlgorithmId,
    params: Vec<u8>,
    mask: ExclusionMask,
    digest: Digest,
    status: MethodStatus,
    impact: Option<ImpactDisclosure>,
}

impl ExclusionRecord {
    /// Builds and validates a record (the pinned order of the [module documentation](self)).
    /// `excluded` may be in any order; the record holds it sorted. `impact` is the fit to every
    /// point and the fit to the kept points, required exactly when `excluded` is not empty.
    pub fn new(
        algorithm: AlgorithmId,
        params: Vec<u8>,
        point_count: usize,
        excluded: Vec<u32>,
        status: MethodStatus,
        impact: Option<(FitSnapshot, FitSnapshot)>,
    ) -> Result<ExclusionRecord, DiscloseError> {
        let spec = PolicySpec::decode(&params)?;
        if algorithm != outlier_algorithm_id(spec.policy.kind(), algorithm.version) {
            return Err(PolicyError::InvalidParameter { field: "algorithm", reason: "method_mismatch" }.into());
        }
        let mask = ExclusionMask::new(point_count, excluded)?;
        check_guards(&spec, &mask)?;
        if status != MethodStatus::Decided && !mask.is_empty() {
            return Err(DiscloseError::StatusMismatch { status, excluded: mask.len() });
        }
        let impact = match (mask.is_empty(), impact) {
            (true, None) => None,
            (true, Some(_)) => return Err(DiscloseError::ImpactUnexpected),
            (false, None) => return Err(DiscloseError::ImpactMissing { excluded: mask.len() }),
            (false, Some((before, after))) => {
                let target = spec.target;
                for (which, snap) in [(BEFORE, &before), (AFTER, &after)] {
                    let got = snap.0.primitive();
                    if got != target {
                        return Err(DiscloseError::PrimitiveMismatch { which, expected: target, got });
                    }
                }
                let d = ImpactDisclosure::new(before, after)?;
                for (which, expected, got) in
                    [(BEFORE, mask.point_count(), d.before.1.n), (AFTER, mask.remaining(), d.after.1.n)]
                {
                    if got != expected {
                        return Err(DiscloseError::CountMismatch { which, expected, got });
                    }
                }
                Some(d)
            }
        };
        let digest = mask.digest();
        Ok(ExclusionRecord { algorithm, params, mask, digest, status, impact })
    }

    /// The record of an [`Exclusion`] made by [`crate::policy::apply`], with the fits before
    /// and after it (required exactly when it excluded a point). Same checks as
    /// [`ExclusionRecord::new`]; the method's status is carried over.
    pub fn from_exclusion(
        exclusion: &Exclusion,
        impact: Option<(FitSnapshot, FitSnapshot)>,
    ) -> Result<ExclusionRecord, DiscloseError> {
        let mut excluded = Vec::new();
        excluded.try_reserve_exact(exclusion.excluded_count()).map_err(|_| alloc("disclose.record"))?;
        excluded.extend_from_slice(exclusion.excluded());
        let mut params = Vec::new();
        params.try_reserve_exact(exclusion.params().len()).map_err(|_| alloc("disclose.record"))?;
        params.extend_from_slice(exclusion.params());
        ExclusionRecord::new(
            exclusion.algorithm().clone(),
            params,
            exclusion.point_count(),
            excluded,
            exclusion.status(),
            impact,
        )
    }

    /// The policy's algorithm ID.
    pub fn algorithm(&self) -> &AlgorithmId {
        &self.algorithm
    }

    /// The canonical encoding of the policy spec.
    pub fn params(&self) -> &[u8] {
        &self.params
    }

    /// Number of input points.
    pub fn point_count(&self) -> usize {
        self.mask.point_count()
    }

    /// The excluded indices, ascending and unique.
    pub fn excluded(&self) -> &[u32] {
        self.mask.indices()
    }

    /// Number of excluded points (REQ-KRN.OUT.DISC-003 states zero too).
    pub fn excluded_count(&self) -> usize {
        self.mask.len()
    }

    /// What the method reported with its set.
    pub fn status(&self) -> MethodStatus {
        self.status
    }

    /// The impact disclosure; `None` exactly when nothing was excluded.
    pub fn impact(&self) -> Option<&ImpactDisclosure> {
        self.impact.as_ref()
    }

    /// The bytes the set digest is computed over: `u32le(point_count)` then `u32le(index)`
    /// for each excluded index, ascending (REQ-KRN.OUT.DISC-001).
    pub fn digest_input(&self) -> Result<Vec<u8>, DiscloseError> {
        Ok(self.mask.canonical_bytes()?)
    }

    /// SHA-256 of [`ExclusionRecord::digest_input`].
    pub fn digest(&self) -> &Digest {
        &self.digest
    }

    /// The canonical bytes (pinned in the [module documentation](self)).
    pub fn encode(&self) -> Result<Vec<u8>, DiscloseError> {
        let mut o = Out { buf: Vec::new() };
        o.put(b"{\"format\":\"")?;
        o.put(RECORD_FORMAT.as_bytes())?;
        o.put(b"\",\"algorithm\":\"")?;
        o.put(self.algorithm.to_string().as_bytes())?;
        o.put(b"\",\"params\":")?;
        o.put(&self.params)?;
        o.put(b",\"point_count\":")?;
        o.int(self.mask.point_count() as u64)?;
        o.put(b",\"excluded_count\":")?;
        o.int(self.mask.len() as u64)?;
        o.put(b",\"excluded\":[")?;
        for (k, &i) in self.mask.indices().iter().enumerate() {
            if k > 0 {
                o.put(b",")?;
            }
            o.int(u64::from(i))?;
        }
        o.put(b"],\"excluded_digest\":\"")?;
        o.put(self.digest.as_str().as_bytes())?;
        o.put(b"\",\"status\":\"")?;
        o.put(self.status.code().as_bytes())?;
        o.put(b"\",\"impact\":")?;
        match &self.impact {
            None => o.put(b"null")?,
            Some(d) => d.write(&mut o)?,
        }
        o.put(b"}")?;
        Ok(o.buf)
    }
}

const OP: &str = "disclose.encode";

struct Out {
    buf: Vec<u8>,
}

impl Out {
    fn put(&mut self, b: &[u8]) -> Result<(), DiscloseError> {
        self.buf.try_reserve(b.len()).map_err(|_| alloc(OP))?;
        self.buf.extend_from_slice(b);
        Ok(())
    }

    fn float(&mut self, x: f64) -> Result<(), DiscloseError> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let bits = x.to_bits();
        let mut b = [0u8; 18];
        b[0] = b'"';
        b[17] = b'"';
        for (k, slot) in b[1..17].iter_mut().enumerate() {
            *slot = HEX[((bits >> (60 - 4 * k)) & 0xF) as usize];
        }
        self.put(&b)
    }

    fn int(&mut self, v: u64) -> Result<(), DiscloseError> {
        let mut digits = [0u8; 20];
        let mut pos = digits.len();
        let mut x = v;
        loop {
            pos -= 1;
            digits[pos] = b'0' + (x % 10) as u8;
            x /= 10;
            if x == 0 {
                break;
            }
        }
        self.put(b"\"")?;
        self.put(&digits[pos..])?;
        self.put(b"\"")
    }

    fn snapshot(&mut self, s: &FitSnapshot) -> Result<(), DiscloseError> {
        self.put(b"{\"feature\":")?;
        self.put(&encode_feature(&s.0)?)?;
        let st = &s.1;
        self.put(b",\"stats\":{\"n\":")?;
        self.int(st.n as u64)?;
        self.put(b",\"rms\":")?;
        self.float(st.rms)?;
        self.put(b",\"max\":")?;
        self.float(st.max)?;
        self.put(b",\"min\":")?;
        self.float(st.min)?;
        self.put(b",\"mean\":")?;
        self.float(st.mean)?;
        self.put(b",\"std_dev\":")?;
        self.float(st.std_dev)?;
        self.put(b"}}")
    }
}

/// `after - before`, refused if it overflows.
fn diff(after: f64, before: f64) -> Result<f64, DiscloseError> {
    let d = after - before;
    if d.is_finite() {
        Ok(d)
    } else {
        Err(NumError::Infinite { op: "disclose.delta", index: None }.into())
    }
}

fn is_negative_zero(x: f64) -> bool {
    x == 0.0 && x.is_sign_negative()
}

/// The sign rule of `fid_fit::contract::normalize_direction`, checked: the first component
/// that is not `±0.0` is positive, and no component is `-0.0`.
fn sign_normalised(u: &[f64]) -> bool {
    !u.iter().any(|&x| is_negative_zero(x)) && matches!(u.iter().find(|&&x| x != 0.0), Some(&x) if x > 0.0)
}

/// Largest accepted `|‖u‖² - 1|` of a direction: a few units in the last place of 1, the
/// rounding of a division by its norm and of the sum of squares. A fixed floor, so it can
/// never be zero.
pub const UNIT_LENGTH_TOL: f64 = 8.0 * f64::EPSILON;

/// Whether `u` has unit length: `|red::sum_sq(u) - 1| <= UNIT_LENGTH_TOL`.
fn unit_length(u: &[f64]) -> Result<bool, DiscloseError> {
    let sq = red::sum_sq(u)?;
    Ok((sq - 1.0).abs() <= UNIT_LENGTH_TOL)
}

fn check_snapshot(which: &'static str, s: &FitSnapshot) -> Result<(), DiscloseError> {
    let (feat, st) = s;
    // fid-fit's validation (finite values, ranges, spline counts). Its normalised copy is not
    // used: the record stores the values it was given, so they must already be in canonical
    // form, which the checks below enforce instead of silently rewriting them.
    feat.canonical()?;
    for &(field, u, signed) in directions(feat).iter().flatten() {
        let ok = if signed { sign_normalised(u) } else { !u.iter().any(|&x| is_negative_zero(x)) };
        if !ok || !unit_length(u)? {
            return Err(DiscloseError::NotCanonical { which, field });
        }
    }
    if let FittedFeature::Slot { length, width, .. } = feat {
        // fid-fit's canonical form turns -0.0 into +0.0 here; both zeros must not encode twice.
        for (field, x) in [("length", *length), ("width", *width)] {
            if is_negative_zero(x) {
                return Err(DiscloseError::NotCanonical { which, field });
            }
        }
    }
    let bad = |field, reason| Err(DiscloseError::StatsInvalid { which, field, reason });
    if st.n == 0 {
        return bad("n", "zero");
    }
    for (field, x) in [("rms", st.rms), ("max", st.max), ("min", st.min), ("mean", st.mean), ("std_dev", st.std_dev)] {
        if !x.is_finite() {
            return bad(field, "non_finite");
        }
    }
    if st.rms < 0.0 {
        return bad("rms", "negative");
    }
    if st.std_dev < 0.0 {
        return bad("std_dev", "negative");
    }
    if st.min > st.max {
        return bad("min", "min_above_max");
    }
    Ok(())
}

/// One direction field: `(name, vector, sign-normalised)`. `false` marks an orientation
/// axis (cone, paraboloid), unit length only.
type Direction<'a> = (&'static str, &'a [f64], bool);

/// The direction fields of a feature, in encoding order (at most two).
fn directions(feat: &FittedFeature) -> [Option<Direction<'_>>; 2] {
    use FittedFeature as F;
    match feat {
        F::Line2 { d, .. } => [Some(("d", &d[..], true)), None],
        F::Line3 { d, .. } | F::Cylinder { d, .. } => [Some(("d", &d[..], true)), None],
        F::Plane { n, .. } | F::Circle3 { n, .. } | F::Torus { n, .. } => [Some(("n", &n[..], true)), None],
        F::Slot { n, d, .. } | F::Rectangle { n, d, .. } | F::Ellipse { n, d, .. } => {
            [Some(("n", &n[..], true)), Some(("d", &d[..], true))]
        }
        F::Cone { d, .. } | F::Paraboloid { d, .. } => [Some(("d", &d[..], false)), None],
        F::Point { .. } | F::Circle2 { .. } | F::Sphere { .. } | F::Polyline { .. } | F::Spline { .. } => [None, None],
    }
}

fn check_shape(before: &FittedFeature, after: &FittedFeature) -> Result<(), DiscloseError> {
    use FittedFeature as F;
    let bad = |field| Err(DiscloseError::ShapeMismatch { field });
    match (before, after) {
        (F::Polyline { vertices: a }, F::Polyline { vertices: b }) if a.len() != b.len() => bad("vertices"),
        (F::Spline { degree: da, knots: ka, control: ca }, F::Spline { degree: db, knots: kb, control: cb }) => {
            if da != db {
                bad("degree")
            } else if ka.len() != kb.len() {
                bad("knots")
            } else if ca.len() != cb.len() {
                bad("control")
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

/// The numeric parameters of a feature in its encoding order.
fn parameters(feat: &FittedFeature) -> Result<Vec<f64>, DiscloseError> {
    use FittedFeature as F;
    let mut out: Vec<f64> = Vec::new();
    let mut put = |xs: &[f64]| -> Result<(), DiscloseError> {
        out.try_reserve(xs.len()).map_err(|_| alloc("disclose.parameters"))?;
        out.extend_from_slice(xs);
        Ok(())
    };
    match feat {
        F::Point { c } => put(c)?,
        F::Line2 { p, d } => {
            put(p)?;
            put(d)?;
        }
        F::Line3 { p, d } => {
            put(p)?;
            put(d)?;
        }
        F::Plane { p, n } => {
            put(p)?;
            put(n)?;
        }
        F::Circle2 { c, r } => {
            put(c)?;
            put(&[*r])?;
        }
        F::Circle3 { c, n, r } => {
            put(c)?;
            put(n)?;
            put(&[*r])?;
        }
        F::Sphere { c, r } => {
            put(c)?;
            put(&[*r])?;
        }
        F::Cylinder { p, d, r } => {
            put(p)?;
            put(d)?;
            put(&[*r])?;
        }
        F::Cone { apex, d, half_angle } => {
            put(apex)?;
            put(d)?;
            put(&[*half_angle])?;
        }
        F::Torus { c, n, major_r, minor_r } => {
            put(c)?;
            put(n)?;
            put(&[*major_r, *minor_r])?;
        }
        F::Slot { c, n, d, length, width } | F::Rectangle { c, n, d, length, width } => {
            put(c)?;
            put(n)?;
            put(d)?;
            put(&[*length, *width])?;
        }
        F::Ellipse { c, n, d, a, b } => {
            put(c)?;
            put(n)?;
            put(d)?;
            put(&[*a, *b])?;
        }
        F::Paraboloid { apex, d, focal } => {
            put(apex)?;
            put(d)?;
            put(&[*focal])?;
        }
        F::Polyline { vertices } => {
            for v in vertices {
                put(v)?;
            }
        }
        F::Spline { knots, control, .. } => {
            put(knots)?;
            for v in control {
                put(v)?;
            }
        }
    }
    Ok(out)
}
