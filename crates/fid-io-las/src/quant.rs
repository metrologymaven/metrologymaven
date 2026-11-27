//! Scaled-integer coordinates, writer scale and offset, quantisation metadata (spec node
//! IO.CLOUD.LAS.QUANT).
//!
//! LAS, LAZ and COPC store each coordinate as a 32-bit integer `X` with a per-axis scale and
//! offset; the coordinate is `offset + scale * X`.
//!
//! **Read** ([`QuantPlan::from_header`], [`QuantPlan::decode`], [`import_batch`]): the
//! coordinate is the exact value of `offset + scale * X` rounded once to the nearest binary64
//! (ties to even), REQ-IO.CLOUD.LAS.QUANT-001. KNP-1 forbids FMA, so the product is split
//! without error (Dekker's product with Veltkamp's split, factor 2^27 + 1) and the three-term
//! sum is rounded once with Knuth's two-sum and a round-to-odd step, after Boldo and
//! Melquiond's correctly rounded sum of three numbers (IEEE Trans. Computers 57(4), 2008).
//! The fixture `fixtures/io/cloud/las/quant/cases.txt` holds 271 cases where the naive
//! `offset + scale * X` (two roundings) is wrong, and the tests check 10^6 seeded triples
//! against an exact integer reference. A zero result is `+0.0`. Every imported point set
//! carries per-axis scale, offset and `u_quant = scale / sqrt(12)` ([`AxisQuant`]), with the
//! square root from `fid-num` (REQ-IO.CLOUD.LAS.QUANT-002).
//!
//! **Envelope.** The header is hostile. Each scale must be finite and between
//! [`SCALE_MIN`] = 2^-512 and [`SCALE_MAX`] = 2^512, each offset finite with
//! |offset| <= [`COORD_MAX`] = 2^480 (`scale_invalid{axis}`, `offset_invalid{axis}`, checked
//! axis by axis in x, y, z order, scale before offset). Inside the envelope no step of the
//! error-free transformations can overflow or lose bits to underflow, so the rounding is
//! exact; outside it the file is refused rather than decoded approximately. A non-positive
//! scale is refused too: `u_quant` would be negative.
//!
//! **Write** ([`quantise`], [`write_las`] with an [`ExportTarget`]): the scale is the caller's per axis or
//! [`DEFAULT_WRITE_SCALE`] = 1e-7 m. A scale above [`MAX_WRITE_SCALE`] = 1e-7 m is
//! `scale_too_coarse{axis}` (REQ-IO.CLOUD.LAS.QUANT-003); one that is not finite or outside
//! the envelope is `scale_invalid{axis}`. The points are checked first, in point order and
//! x, y, z within a point: NaN or infinity is `non_finite{index, axis}`, |c| > 2^480 is
//! `coordinate_outside_envelope{index, axis}`; an empty set is `empty_point_set` (it has no
//! midpoint for the offset). Per axis, the offset is the midpoint `min * 0.5 + max * 0.5`
//! (total order) rounded to a multiple of the scale: `k = round_half_even(mid / scale)`,
//! `offset = k * scale` (binary64; a zero offset is `+0.0`). Each integer is the one nearest
//! to the exact `(c - offset) / scale`, decided with an exact sign test (Shewchuk's
//! grow-expansion), never from the rounded quotient alone. An exact half-way case goes to the
//! even integer and is counted per axis in [`QuantReport::ties`]. The first integer outside
//! the i32 range, in point order and x, y, z order, is `out_of_range{index, axis}`
//! (REQ-IO.CLOUD.LAS.QUANT-004). Every integer is computed, and the header's point count and
//! the batch shape checked, before [`write_las`] creates the [`LasWriter`], so a refused
//! export writes no byte.
//!
//! **Report** (REQ-IO.CLOUD.LAS.QUANT-005): per axis the validated plan (scale, offset,
//! `u_quant`), the number of ties, the decoded bounds and two errors:
//! - `max_err`, the quantisation error: the largest exact residual `|c - offset - scale * X|`
//!   rounded once to binary64 (the exact residual from the same expansion as the sign test,
//!   then correctly rounded with exact midpoint tests). The nearest integer makes the exact
//!   residual at most `scale / 2`, so `max_err <= scale / 2`; with the default scale (the
//!   binary64 nearest 1e-7, slightly below it) that is below 5e-8 m.
//! - `max_read_err`, the largest `|c - decode(X)|` (rounded once): the error a reader of the
//!   written file sees, which adds the reader's own rounding of `offset + scale * X` (up to half
//!   an ulp of the decoded coordinate) and so can pass `scale / 2`, and 5e-8 m with the default
//!   scale, near a half-way case. It is reported as it is, never clamped.

use crate::error::{LasError, LasField};
use crate::header::LasHeader;
use crate::points::{AttributeColumns, RawPointBatch};
use crate::vlr::Vlr;
use crate::writer::LasWriter;
use fid_num::NumError;
use std::cmp::Ordering;
use std::fmt;
use std::io::Write;

/// Largest scale the writer accepts, in metres (REQ-IO.CLOUD.LAS.QUANT-003).
pub const MAX_WRITE_SCALE: f64 = 1e-7;
/// Scale the writer uses when the caller gives none, in metres.
pub const DEFAULT_WRITE_SCALE: f64 = 1e-7;
/// Smallest scale inside the exactness envelope: 2^-512.
pub const SCALE_MIN: f64 = f64::from_bits((1023 - 512) << 52);
/// Largest scale inside the exactness envelope: 2^512.
pub const SCALE_MAX: f64 = f64::from_bits((1023 + 512) << 52);
/// Largest |offset| (read) and |coordinate| (write) inside the envelope: 2^480.
pub const COORD_MAX: f64 = f64::from_bits((1023 + 480) << 52);

/// Veltkamp's split factor for binary64: 2^27 + 1.
const SPLIT: f64 = 134_217_729.0;
/// |(c - offset) / scale| beyond this cannot round to an i32 (|i32| <= 2^31, plus one step of
/// slack for the rounded quotient).
const QUOTIENT_BOUND: f64 = 2_147_483_649.0;

/// A coordinate axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Axis {
    /// x.
    X,
    /// y.
    Y,
    /// z.
    Z,
}

impl Axis {
    /// The three axes in order.
    pub const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    /// `x`, `y` or `z`.
    pub fn as_str(self) -> &'static str {
        match self {
            Axis::X => "x",
            Axis::Y => "y",
            Axis::Z => "z",
        }
    }
}

impl fmt::Display for Axis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a scale or offset was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ValueReason {
    /// NaN or an infinity.
    NotFinite,
    /// A scale that is zero (either sign) or negative.
    NotPositive,
    /// Outside the exactness envelope ([`SCALE_MIN`], [`SCALE_MAX`], [`COORD_MAX`]).
    OutsideEnvelope,
}

impl ValueReason {
    /// The stable snake_case name.
    pub fn as_str(self) -> &'static str {
        match self {
            ValueReason::NotFinite => "not_finite",
            ValueReason::NotPositive => "not_positive",
            ValueReason::OutsideEnvelope => "outside_envelope",
        }
    }
}

/// Errors of the quantisation layer. Indices are point indices in the caller's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuantError {
    /// A header or caller scale is unusable.
    ScaleInvalid {
        /// The first failing axis.
        axis: Axis,
        /// Why.
        reason: ValueReason,
    },
    /// A header offset is unusable.
    OffsetInvalid {
        /// The first failing axis.
        axis: Axis,
        /// Why.
        reason: ValueReason,
    },
    /// A caller scale is above [`MAX_WRITE_SCALE`] (REQ-IO.CLOUD.LAS.QUANT-003).
    ScaleTooCoarse {
        /// The first failing axis.
        axis: Axis,
    },
    /// The export has no points, so no midpoint to place the offset at.
    EmptyPointSet,
    /// A coordinate to export is NaN or infinite.
    NonFinite {
        /// The point.
        index: u64,
        /// The axis.
        axis: Axis,
    },
    /// A coordinate to export is larger in magnitude than [`COORD_MAX`].
    CoordinateOutsideEnvelope {
        /// The point.
        index: u64,
        /// The axis.
        axis: Axis,
    },
    /// A coordinate is not representable as an i32 at the chosen scale and offset
    /// (REQ-IO.CLOUD.LAS.QUANT-004): the first such point and axis.
    OutOfRange {
        /// The point.
        index: u64,
        /// The axis.
        axis: Axis,
    },
    /// The exact residual of a point could not be rounded (the correction loop did not settle;
    /// never expected, reported rather than guessed).
    ResidualNotRounded {
        /// The point.
        index: u64,
        /// The axis.
        axis: Axis,
    },
    /// The integer columns of a point batch have different lengths.
    ColumnMismatch {
        /// Length of the x column.
        x: u64,
        /// Length of the y column.
        y: u64,
        /// Length of the z column.
        z: u64,
    },
    /// A `fid-num` function failed (only `sqrt(12)`; never expected).
    Numeric(NumError),
}

impl QuantError {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            QuantError::ScaleInvalid { .. } => "scale_invalid",
            QuantError::OffsetInvalid { .. } => "offset_invalid",
            QuantError::ScaleTooCoarse { .. } => "scale_too_coarse",
            QuantError::EmptyPointSet => "empty_point_set",
            QuantError::NonFinite { .. } => "non_finite",
            QuantError::CoordinateOutsideEnvelope { .. } => "coordinate_outside_envelope",
            QuantError::OutOfRange { .. } => "out_of_range",
            QuantError::ResidualNotRounded { .. } => "residual_not_rounded",
            QuantError::ColumnMismatch { .. } => "column_mismatch",
            QuantError::Numeric(_) => "numeric",
        }
    }
}

impl fmt::Display for QuantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            QuantError::ScaleInvalid { axis, reason } | QuantError::OffsetInvalid { axis, reason } => {
                write!(f, "{code}: axis {axis}, {}", reason.as_str())
            }
            QuantError::ScaleTooCoarse { axis } => {
                write!(f, "{code}: axis {axis} scale above {MAX_WRITE_SCALE:e} m")
            }
            QuantError::EmptyPointSet => write!(f, "{code}: no points to export"),
            QuantError::NonFinite { index, axis }
            | QuantError::CoordinateOutsideEnvelope { index, axis }
            | QuantError::OutOfRange { index, axis }
            | QuantError::ResidualNotRounded { index, axis } => write!(f, "{code}: point {index}, axis {axis}"),
            QuantError::ColumnMismatch { x, y, z } => write!(f, "{code}: x {x}, y {y}, z {z} values"),
            QuantError::Numeric(e) => write!(f, "{code}: {e:?}"),
        }
    }
}

impl std::error::Error for QuantError {}

/// Errors of [`write_las`]: quantisation (nothing written) or the LAS writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// Refused before any byte was written.
    Quant(QuantError),
    /// The LAS writer failed.
    Las(LasError),
}

impl ExportError {
    /// The stable snake_case code of the inner error.
    pub fn code(&self) -> &'static str {
        match self {
            ExportError::Quant(e) => e.code(),
            ExportError::Las(e) => e.code(),
        }
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Quant(e) => e.fmt(f),
            ExportError::Las(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<QuantError> for ExportError {
    fn from(e: QuantError) -> Self {
        ExportError::Quant(e)
    }
}

impl From<LasError> for ExportError {
    fn from(e: LasError) -> Self {
        ExportError::Las(e)
    }
}

/// One axis's quantisation: scale, offset and quantisation uncertainty `scale / sqrt(12)`
/// (the standard uncertainty of a uniform rounding error of width `scale`), all in metres.
/// Only [`QuantPlan::new`] (and so [`QuantPlan::from_header`] and [`quantise`]) makes one, so
/// every value has passed the envelope checks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisQuant {
    scale: f64,
    offset: f64,
    u_quant: f64,
}

impl AxisQuant {
    /// Scale.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Offset.
    pub fn offset(&self) -> f64 {
        self.offset
    }

    /// `scale / sqrt(12)`, with the square root from `fid-num`.
    pub fn u_quant(&self) -> f64 {
        self.u_quant
    }
}

/// The quantisation of all three axes: the point-set metadata of REQ-IO.CLOUD.LAS.QUANT-002.
/// [`QuantPlan::new`] is the only constructor, so a plan always lies inside the envelope.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuantPlan {
    axes: [AxisQuant; 3],
}

fn check_scale(axis: Axis, s: f64) -> Result<f64, QuantError> {
    let reason = if !s.is_finite() {
        ValueReason::NotFinite
    } else if s <= 0.0 {
        ValueReason::NotPositive
    } else if !(SCALE_MIN..=SCALE_MAX).contains(&s) {
        ValueReason::OutsideEnvelope
    } else {
        return Ok(s);
    };
    Err(QuantError::ScaleInvalid { axis, reason })
}

fn check_offset(axis: Axis, o: f64) -> Result<f64, QuantError> {
    let reason = if !o.is_finite() {
        ValueReason::NotFinite
    } else if o.abs() > COORD_MAX {
        ValueReason::OutsideEnvelope
    } else {
        return Ok(o);
    };
    Err(QuantError::OffsetInvalid { axis, reason })
}

fn u_quant(scale: f64) -> Result<f64, QuantError> {
    let root = fid_num::f::sqrt(12.0).map_err(QuantError::Numeric)?;
    Ok(scale / root)
}

impl QuantPlan {
    /// Checks a scale and offset per axis (x, y, z; scale before offset) against the
    /// envelope and attaches `u_quant`.
    pub fn new(scale: [f64; 3], offset: [f64; 3]) -> Result<QuantPlan, QuantError> {
        let mut axes = [AxisQuant { scale: 1.0, offset: 0.0, u_quant: 0.0 }; 3];
        for ((slot, axis), (s, o)) in axes.iter_mut().zip(Axis::ALL).zip(scale.into_iter().zip(offset)) {
            let s = check_scale(axis, s)?;
            let o = check_offset(axis, o)?;
            *slot = AxisQuant { scale: s, offset: o, u_quant: u_quant(s)? };
        }
        Ok(QuantPlan { axes })
    }

    /// The plan of a file's header (hostile input).
    pub fn from_header(header: &LasHeader) -> Result<QuantPlan, QuantError> {
        QuantPlan::new(header.scale, header.offset)
    }

    /// The three axes, x, y, z.
    pub fn axes(&self) -> [AxisQuant; 3] {
        self.axes
    }

    /// One axis.
    pub fn axis(&self, axis: Axis) -> AxisQuant {
        match axis {
            Axis::X => self.axes[0],
            Axis::Y => self.axes[1],
            Axis::Z => self.axes[2],
        }
    }

    /// `offset + scale * v` for one axis, rounded once (REQ-IO.CLOUD.LAS.QUANT-001).
    pub fn decode(&self, axis: Axis, v: i32) -> f64 {
        let q = self.axis(axis);
        decode(q.scale, q.offset, v)
    }

    /// Decodes a batch's integer coordinates, point by point.
    pub fn decode_batch(&self, batch: &RawPointBatch) -> Result<Vec<[f64; 3]>, QuantError> {
        let (x, y, z) = (&batch.xi, &batch.yi, &batch.zi);
        if x.len() != y.len() || x.len() != z.len() {
            return Err(QuantError::ColumnMismatch { x: x.len() as u64, y: y.len() as u64, z: z.len() as u64 });
        }
        let [qx, qy, qz] = self.axes;
        Ok(x.iter()
            .zip(y)
            .zip(z)
            .map(|((&a, &b), &c)| {
                [decode(qx.scale, qx.offset, a), decode(qy.scale, qy.offset, b), decode(qz.scale, qz.offset, c)]
            })
            .collect())
    }
}

/// Decoded coordinates of a batch with their quantisation metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedPoints {
    /// x, y, z per point, in metres.
    pub xyz: Vec<[f64; 3]>,
    /// Scale, offset and `u_quant` per axis.
    pub quant: QuantPlan,
}

/// Decodes a batch read from a file with `header`: checks the header's scale and offset,
/// then decodes every point (REQ-IO.CLOUD.LAS.QUANT-001, -002).
pub fn import_batch(header: &LasHeader, batch: &RawPointBatch) -> Result<ImportedPoints, QuantError> {
    let quant = QuantPlan::from_header(header)?;
    let xyz = quant.decode_batch(batch)?;
    Ok(ImportedPoints { xyz, quant })
}

/// The scale an export uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WriteScale {
    /// [`DEFAULT_WRITE_SCALE`] on every axis.
    Default,
    /// The caller's scale per axis (x, y, z), each at most [`MAX_WRITE_SCALE`].
    PerAxis([f64; 3]),
}

/// What an export did, per axis x, y, z (REQ-IO.CLOUD.LAS.QUANT-005).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuantReport {
    /// Number of points.
    pub points: u64,
    /// Chosen scale, offset and `u_quant`, as validated by [`QuantPlan::new`].
    plan: QuantPlan,
    /// Largest exact residual `|c - offset - scale * X|`, rounded once: the quantisation
    /// error, at most `scale / 2`.
    pub max_err: [f64; 3],
    /// Largest `|c - decode(X)|`, rounded once: the error a reader of the file sees
    /// (quantisation plus the reader's rounding); can exceed `scale / 2` near a half-way case.
    pub max_read_err: [f64; 3],
    /// Number of exact half-way cases, each resolved to the even integer.
    pub ties: [u64; 3],
    /// Smallest decoded coordinate.
    pub min: [f64; 3],
    /// Largest decoded coordinate.
    pub max: [f64; 3],
}

impl QuantReport {
    /// The validated plan the export used.
    pub fn plan(&self) -> QuantPlan {
        self.plan
    }

    /// The plan's three axes, x, y, z.
    pub fn axes(&self) -> [AxisQuant; 3] {
        self.plan.axes
    }

    /// Sets the header's scale, offset and bounds to what was quantised.
    pub fn apply_to(&self, header: &mut LasHeader) {
        header.scale = self.plan.axes.map(|q| q.scale);
        header.offset = self.plan.axes.map(|q| q.offset);
        header.min = self.min;
        header.max = self.max;
    }
}

/// Integer coordinates of an export with its report.
#[derive(Debug, Clone, PartialEq)]
pub struct Quantised {
    /// Integer x per point.
    pub xi: Vec<i32>,
    /// Integer y per point.
    pub yi: Vec<i32>,
    /// Integer z per point.
    pub zi: Vec<i32>,
    /// The report.
    pub report: QuantReport,
}

fn write_scales(req: WriteScale) -> Result<[f64; 3], QuantError> {
    let scale = match req {
        WriteScale::Default => return Ok([DEFAULT_WRITE_SCALE; 3]),
        WriteScale::PerAxis(s) => s,
    };
    for (axis, s) in Axis::ALL.into_iter().zip(scale) {
        check_scale(axis, s)?;
        if s > MAX_WRITE_SCALE {
            return Err(QuantError::ScaleTooCoarse { axis });
        }
    }
    Ok(scale)
}

fn lower(a: f64, b: f64) -> f64 {
    if b.total_cmp(&a) == Ordering::Less {
        b
    } else {
        a
    }
}

fn higher(a: f64, b: f64) -> f64 {
    if b.total_cmp(&a) == Ordering::Greater {
        b
    } else {
        a
    }
}

fn plus_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

/// Chooses scale and offset, checks every coordinate and computes the integers and the
/// report. Nothing is written; see the module documentation for the rules and error order.
pub fn quantise(points: &[[f64; 3]], scale: WriteScale) -> Result<Quantised, QuantError> {
    let scale = write_scales(scale)?;
    let Some(first) = points.first() else {
        return Err(QuantError::EmptyPointSet);
    };
    let (mut lo, mut hi) = (*first, *first);
    for (i, p) in points.iter().enumerate() {
        for (((axis, &c), l), h) in Axis::ALL.into_iter().zip(p).zip(lo.iter_mut()).zip(hi.iter_mut()) {
            let index = i as u64;
            if !c.is_finite() {
                return Err(QuantError::NonFinite { index, axis });
            }
            if c.abs() > COORD_MAX {
                return Err(QuantError::CoordinateOutsideEnvelope { index, axis });
            }
            *l = lower(*l, c);
            *h = higher(*h, c);
        }
    }
    let mut offset = [0.0; 3];
    for (((o, s), l), h) in offset.iter_mut().zip(scale).zip(lo).zip(hi) {
        let mid = l * 0.5 + h * 0.5;
        *o = plus_zero((mid / s).round_ties_even() * s);
    }
    // |mid| <= 2^480 and scale >= 2^-512 keep the offset finite and inside the envelope;
    // the plan re-checks it rather than trusting that argument.
    let plan = QuantPlan::new(scale, offset)?;

    let n = points.len();
    let mut cols = [Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n)];
    let mut max_err = [0.0f64; 3];
    let mut max_read_err = [0.0f64; 3];
    let mut ties = [0u64; 3];
    let mut dmin = [0.0f64; 3];
    let mut dmax = [0.0f64; 3];
    for (i, p) in points.iter().enumerate() {
        for (a, (axis, &c)) in Axis::ALL.into_iter().zip(p).enumerate() {
            let q = plan.axis(axis);
            let index = i as u64;
            let (v, tie) = nearest(c, q.offset, q.scale).ok_or(QuantError::OutOfRange { index, axis })?;
            let (ph, pl) = two_prod(q.scale, f64::from(v));
            let err = residual(c, q.offset, ph, pl).ok_or(QuantError::ResidualNotRounded { index, axis })?.abs();
            let d = decode(q.scale, q.offset, v);
            let read_err = (c - d).abs();
            if let (Some(col), Some(m), Some(mr), Some(t), Some(lo), Some(hi)) = (
                cols.get_mut(a),
                max_err.get_mut(a),
                max_read_err.get_mut(a),
                ties.get_mut(a),
                dmin.get_mut(a),
                dmax.get_mut(a),
            ) {
                col.push(v);
                *m = higher(*m, err);
                *mr = higher(*mr, read_err);
                *t += u64::from(tie);
                *lo = if i == 0 { d } else { lower(*lo, d) };
                *hi = if i == 0 { d } else { higher(*hi, d) };
            }
        }
    }
    let [xi, yi, zi] = cols;
    let report = QuantReport { points: n as u64, plan, max_err, max_read_err, ties, min: dmin, max: dmax };
    Ok(Quantised { xi, yi, zi, report })
}

/// Everything of a LAS file except the coordinates: what [`write_las`] writes around the
/// quantised points.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportTarget<'a> {
    /// The header; its declared point count must equal the number of points. Scale, offset
    /// and bounds are replaced by the quantisation's.
    pub header: &'a LasHeader,
    /// VLRs.
    pub vlrs: &'a [Vlr],
    /// Bytes between the last VLR and the point data.
    pub vlr_padding: &'a [u8],
    /// EVLRs.
    pub evlrs: &'a [Vlr],
    /// The points' attribute columns, in point order.
    pub attrs: AttributeColumns,
}

/// Quantises `points` and writes a LAS file around them. In order, before the writer is
/// created: quantisation (every integer), the point format, the header's declared point count
/// against `points.len()` (`malformed{point_count}`) and the batch shape. So any of these
/// errors writes nothing (REQ-IO.CLOUD.LAS.QUANT-004). Returns the sink and the report.
pub fn write_las<W: Write>(
    out: W,
    target: ExportTarget<'_>,
    points: &[[f64; 3]],
    scale: WriteScale,
) -> Result<(W, QuantReport), ExportError> {
    let q = quantise(points, scale)?;
    let mut h = target.header.clone();
    q.report.apply_to(&mut h);
    let format = h.format().ok_or_else(|| LasError::malformed(LasField::PointDataFormat))?;
    if h.declared_points()? != q.report.points {
        return Err(LasError::malformed(LasField::PointCount).into());
    }
    let batch = RawPointBatch { format, xi: q.xi, yi: q.yi, zi: q.zi, attrs: target.attrs };
    batch.check_shape()?;
    let mut w = LasWriter::new(out, &h, target.vlrs, target.vlr_padding, target.evlrs)?;
    w.write_batch(&batch)?;
    Ok((w.finish()?, q.report))
}

/// `(s, e)` with `s = RN(a + b)` and `s + e = a + b` exactly (Knuth's two-sum; exact unless
/// `a + b` overflows).
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

/// Veltkamp's split: `hi + lo = a` with each half fitting 26 bits (|a| < 2^996).
fn split(a: f64) -> (f64, f64) {
    let c = SPLIT * a;
    let hi = c - (c - a);
    (hi, a - hi)
}

/// `(p, e)` with `p = RN(a * b)` and `p + e = a * b` exactly (Dekker's product; exact inside
/// the envelope, where neither the split overflows nor the error underflows). No FMA.
fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    (p, al * bl - (((p - ah * bh) - al * bh) - ah * bl))
}

/// `a + b` rounded to odd: the exact sum if representable, else whichever neighbour has an
/// odd last significand bit.
fn add_round_odd(a: f64, b: f64) -> f64 {
    let (s, e) = two_sum(a, b);
    if e == 0.0 || s.to_bits() & 1 == 1 {
        return s;
    }
    // e != 0 implies s != 0. The exact sum lies strictly between s and its neighbour on the
    // side of e: away from zero when e has the sign of s, towards zero otherwise.
    let bits = s.to_bits();
    if (e > 0.0) == (s > 0.0) {
        f64::from_bits(bits.wrapping_add(1))
    } else {
        f64::from_bits(bits.wrapping_sub(1))
    }
}

/// `a + b + c` rounded once to nearest-even (Boldo and Melquiond's algorithm with
/// round-to-odd).
fn sum3(a: f64, b: f64, c: f64) -> f64 {
    let (uh, ul) = two_sum(b, c);
    let (th, tl) = two_sum(a, uh);
    th + add_round_odd(tl, ul)
}

/// `offset + scale * v`, rounded once; a zero result is `+0.0`.
fn decode(scale: f64, offset: f64, v: i32) -> f64 {
    let (ph, pl) = two_prod(scale, f64::from(v));
    plus_zero(sum3(offset, ph, pl))
}

/// The sum of `terms` as an exact non-overlapping expansion in increasing magnitude (zeros
/// possible), by Shewchuk's grow-expansion (no overflow assumed).
fn expansion<const N: usize>(terms: [f64; N]) -> [f64; N] {
    let mut e = [0.0f64; N];
    for (n, b) in terms.into_iter().enumerate() {
        let mut q = b;
        for slot in e.iter_mut().take(n) {
            let (s, err) = two_sum(q, *slot);
            *slot = err;
            q = s;
        }
        if let Some(slot) = e.get_mut(n) {
            *slot = q;
        }
    }
    e
}

/// The exact sign of the sum of `terms`: the sign of the expansion's largest non-zero
/// component.
fn sign_of_sum<const N: usize>(terms: [f64; N]) -> Ordering {
    match expansion(terms).iter().rev().find(|v| **v != 0.0) {
        Some(v) if *v > 0.0 => Ordering::Greater,
        Some(_) => Ordering::Less,
        None => Ordering::Equal,
    }
}

/// Of two adjacent doubles, the one with an even last significand bit.
fn even_of(a: f64, b: f64) -> f64 {
    if a.to_bits() & 1 == 0 {
        a
    } else {
        b
    }
}

/// `c - offset - (ph + pl)` exactly, rounded once to nearest-even; a zero result is `+0.0`.
/// The start is the expansion summed in increasing magnitude (within a few ulps); each step
/// compares twice the exact residual with the sum of the candidate and a neighbour, which is
/// twice their midpoint, by an exact sign test, and moves one ulp or stops. `None` if it has
/// not settled after 16 steps (never expected).
fn residual(c: f64, offset: f64, ph: f64, pl: f64) -> Option<f64> {
    let mut y: f64 = expansion([c, -offset, -ph, -pl]).iter().sum();
    let r2 = [2.0 * c, -2.0 * offset, -2.0 * ph, -2.0 * pl];
    for _ in 0..16 {
        let (up, dn) = (y.next_up(), y.next_down());
        let [a, b, e, f] = r2;
        match sign_of_sum([a, b, e, f, -y, -up]) {
            Ordering::Greater => {
                y = up;
                continue;
            }
            Ordering::Equal => return Some(plus_zero(even_of(y, up))),
            Ordering::Less => {}
        }
        match sign_of_sum([a, b, e, f, -y, -dn]) {
            Ordering::Less => y = dn,
            Ordering::Equal => return Some(plus_zero(even_of(y, dn))),
            Ordering::Greater => return Some(plus_zero(y)),
        }
    }
    None
}

/// The integer nearest to the exact `(c - offset) / scale`, ties to even (`true` when it was
/// a tie), or `None` outside the i32 range.
fn nearest(c: f64, offset: f64, scale: f64) -> Option<(i32, bool)> {
    let t = (c - offset) / scale;
    if t.is_nan() || t.abs() > QUOTIENT_BOUND {
        return None;
    }
    // |t| <= 2^31 + 1, so the conversion is exact. The rounded quotient is within far less
    // than one step of the exact one, so the answer is v0 or a neighbour.
    let v0 = t.round_ties_even() as i64;
    let (ph, pl) = two_prod(scale, v0 as f64);
    let even = |a: i64, b: i64| if a % 2 == 0 { a } else { b };
    let (v, tie) = if sign_of_sum([c, -offset, -ph, -pl]) != Ordering::Less {
        // r = c - offset - scale * v0 >= 0: compare 2r with the scale.
        match sign_of_sum([2.0 * c, -2.0 * offset, -2.0 * ph, -2.0 * pl, -scale]) {
            Ordering::Greater => (v0 + 1, false),
            Ordering::Equal => (even(v0, v0 + 1), true),
            Ordering::Less => (v0, false),
        }
    } else {
        match sign_of_sum([2.0 * c, -2.0 * offset, -2.0 * ph, -2.0 * pl, scale]) {
            Ordering::Less => (v0 - 1, false),
            Ordering::Equal => (even(v0 - 1, v0), true),
            Ordering::Greater => (v0, false),
        }
    };
    i32::try_from(v).ok().map(|v| (v, tie))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_constants_are_powers_of_two() {
        assert_eq!(SCALE_MIN, 7.458340731200207e-155);
        assert_eq!(SCALE_MAX, 1.3407807929942597e154);
        assert_eq!(COORD_MAX, 3.1217485503159922e144);
        assert_eq!(SCALE_MIN * SCALE_MAX, 1.0);
    }

    #[test]
    fn sign_of_sum_sees_below_rounding() {
        let tiny = f64::from_bits(1);
        assert_eq!(sign_of_sum([1.0, tiny, -1.0]), Ordering::Greater);
        assert_eq!(sign_of_sum([1e300, -1e300, -tiny]), Ordering::Less);
        assert_eq!(sign_of_sum([0.1, 0.2, -0.1, -0.2]), Ordering::Equal);
        assert_eq!(sign_of_sum::<0>([]), Ordering::Equal);
    }

    #[test]
    fn round_odd_picks_the_odd_neighbour() {
        // 1 + 2^-60 is between 1 (even) and 1 + 2^-52 (odd).
        assert_eq!(add_round_odd(1.0, 2f64.powi(-60)), 1.0 + f64::EPSILON);
        assert_eq!(add_round_odd(1.0, -(2f64.powi(-60))), f64::from_bits(1.0f64.to_bits() - 1));
        assert_eq!(add_round_odd(1.0, 1.0), 2.0);
    }

    #[test]
    fn residual_is_rounded_once_with_ties_to_even() {
        let ulp = f64::EPSILON;
        // 1 + ulp/2 and 1 + 3ulp/2 are ties: to 1 and to 1 + 2ulp.
        assert_eq!(residual(1.0, 0.0, 0.0, -ulp / 2.0), Some(1.0));
        assert_eq!(residual(1.0, 0.0, 0.0, -3.0 * ulp / 2.0), Some(1.0 + 2.0 * ulp));
        // Just above a tie rounds up; cancellation leaves the product's low part exactly.
        assert_eq!(residual(1.0, -(ulp / 2.0), 0.0, -(ulp * ulp)), Some(1.0 + ulp));
        assert_eq!(residual(1e8, 1e8 - 0.5, 0.25, 1e-30), Some(0.25));
        assert_eq!(residual(0.5, 0.25, 0.25, 0.0), Some(0.0));
        assert_eq!(residual(0.5, 0.25, 0.25, 0.0).map(f64::to_bits), Some(0));
        let tiny = f64::from_bits(1);
        assert_eq!(residual(tiny, 0.0, 0.0, 0.0), Some(tiny));
    }

    #[test]
    fn nearest_takes_the_even_integer_on_ties() {
        let q = 2f64.powi(-24);
        assert_eq!(nearest(q / 2.0, 0.0, q), Some((0, true)));
        assert_eq!(nearest(3.0 * q / 2.0, 0.0, q), Some((2, true)));
        assert_eq!(nearest(-q / 2.0, 0.0, q), Some((0, true)));
        assert_eq!(nearest(q * 0.75, 0.0, q), Some((1, false)));
        assert_eq!(nearest(2f64.powi(31) * q, 0.0, q), None);
        assert_eq!(nearest(-(2f64.powi(31)) * q, 0.0, q), Some((i32::MIN, false)));
    }
}
