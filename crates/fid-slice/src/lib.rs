//! Vertical Slice 0 (`fid-slice`): a runnable, deterministic end-to-end dimensional
//! metrology pipeline wired directly over the real Metrology Maven crates:
//!
//! 1. **KNP-1 Kernel Fitting (`fid-fit`)**: certified tangential datum planes (`A|B|C`),
//!    least-squares plane (`PlaneLs`) and least-squares cylinder (`fit_cylinder`).
//! 2. **DRF & GD&T Evaluation (`fid-gps`)**: 3-2-1 Datum Reference Frame construction
//!    (`build_measured_planar_drf`) and closed-form tolerance-zone evaluation (`ZoneShape`).
//! 3. **Side-Preserving Kernel Formatting & Decision Rules (`fid-num`, `fid-kdec`)**:
//!    `fmt.length@1` with exact decimal limits and typed 4-valued conformance outcomes.
//! 4. **Canonical Hash-Chained Journal (`fid-canon`, `fid-types`)**: RFC 8785 canonical JSON
//!    events chained by SHA-256 digests.
//! 5. **Verifiable Result Package (`fid-pkg`)**: deterministic ZIP archive (`manifest.json`,
//!    sorted members under reserved folders, in-toto `statement.json`) verified fail-closed
//!    through `PackageReader`.
//!
//! Pure Rust, `#![forbid(unsafe_code)]`, zero `unwrap`/`expect`/`panic`, `BTreeMap` only.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Cursor;

use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_fit::contract::{Fit, FitParams, FittedFeature, Method, PointsView};
use fid_fit::ls_cyl::{fit_cylinder, CylinderFit};
use fid_fit::ls_lin::PlaneLs;
use fid_gps::dmt::{AlgId, DmtKey};
use fid_gps::drf::plane_sim::{build_measured_planar_drf, MeasuredPlane};
use fid_gps::zone::shape::{Pose, ZoneInput, ZoneShape, P3};
use fid_kdec::{NotDecidableReason, Outcome, RuleId};
use fid_num::fmt::{format as fmt_kernel, FormatSpec, FormatterId, Limits, Precision, Reported, SideOutcome, Unit};
use fid_num::kdec::Decimal;
use fid_pkg::reader::PackageReader;
use fid_pkg::{build_package, EvidenceTier, MemberInput, PackageInput, PkgPath};
use fid_types::Digest;

/// Algorithm ID used for certified tangential datum plane simulation.
pub const DATUM_SIM_ALG: &str = "fit.plane.tan_minimax@1.0.0";
/// DMT row key recorded on simulated planar datums in Vertical Slice 0.
pub const DATUM_DMT_KEY: &str = "dmt:asme/datum_plane/rmb@1";
/// Built-in decision rule ID recorded on slice conformance outcomes.
pub const DECISION_RULE_ID: &str = "dr:simple-acceptance";
/// Maximum accepted input JSON size (4 MiB).
pub const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;

/// A typed Vertical Slice 0 error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceError {
    pub code: &'static str,
    pub message: String,
}

impl SliceError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl fmt::Display for SliceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SliceError {}

/// A complete inspection job specification for Vertical Slice 0 (all coordinates in SI metres).
#[derive(Debug, Clone, PartialEq)]
pub struct JobSpec {
    pub part_number: String,
    pub revision: String,
    pub serial_number: String,
    pub dialect: String,
    /// Measured points on primary Datum A (top face, nominal outward +Z), flat `[x, y, z, ...]`.
    pub datum_a: Vec<f64>,
    /// Measured points on secondary Datum B (side face, nominal outward -Y).
    pub datum_b: Vec<f64>,
    /// Measured points on tertiary Datum C (end face, nominal outward -X).
    pub datum_c: Vec<f64>,
    /// Measured points on the inspected bore cylinder.
    pub bore_points: Vec<f64>,
    /// Nominal bore axis position in DRF coordinates `(x, y)` in metres.
    pub bore_nominal_xy_m: [f64; 2],
    /// Tolerance limits in exact decimal SI metres.
    pub flatness_limit_m: String,
    pub diameter_lower_m: String,
    pub diameter_upper_m: String,
    pub position_limit_m: String,
    pub perp_limit_m: String,
}

impl JobSpec {
    /// Parses a bounded canonical/standard JSON job specification.
    pub fn from_json(bytes: &[u8]) -> Result<Self, SliceError> {
        if bytes.len() > MAX_INPUT_BYTES {
            return Err(SliceError::new("slice.limit_exceeded", "input JSON exceeds 4 MiB"));
        }
        let val = fid_canon::parse(bytes).map_err(|e| SliceError::new("slice.invalid_json", e.to_string()))?;
        let CValue::Obj(map) = val else {
            return Err(SliceError::new("slice.invalid_schema", "root must be a JSON object"));
        };
        let get_str = |k: &str| -> Result<String, SliceError> {
            match map.get(k) {
                Some(CValue::Str(s)) if !s.is_empty() && s.len() <= 128 => Ok(s.clone()),
                _ => Err(SliceError::new("slice.invalid_schema", format!("missing or invalid string `{k}`"))),
            }
        };
        let get_f64_vec = |k: &str| -> Result<Vec<f64>, SliceError> {
            let Some(CValue::Arr(arr)) = map.get(k) else {
                return Err(SliceError::new("slice.invalid_schema", format!("missing array `{k}`")));
            };
            if arr.len() < 9 || arr.len() % 3 != 0 {
                return Err(SliceError::new("slice.invalid_schema", format!("`{k}` must hold at least 3 xyz triples")));
            }
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                let CValue::Num(v) = item else {
                    return Err(SliceError::new("slice.invalid_schema", format!("non-numeric entry in `{k}`")));
                };
                if !v.is_finite() {
                    return Err(SliceError::new("slice.invalid_schema", format!("non-finite entry in `{k}`")));
                }
                out.push(*v);
            }
            Ok(out)
        };
        let bore_xy = match map.get("bore_nominal_xy_m") {
            Some(CValue::Arr(a)) if a.len() == 2 => {
                let (CValue::Num(x), CValue::Num(y)) = (&a[0], &a[1]) else {
                    return Err(SliceError::new("slice.invalid_schema", "bore_nominal_xy_m must hold 2 numbers"));
                };
                [*x, *y]
            }
            _ => return Err(SliceError::new("slice.invalid_schema", "missing bore_nominal_xy_m [x, y]")),
        };
        Ok(Self {
            part_number: get_str("part_number")?,
            revision: get_str("revision")?,
            serial_number: get_str("serial_number")?,
            dialect: get_str("dialect")?,
            datum_a: get_f64_vec("datum_a")?,
            datum_b: get_f64_vec("datum_b")?,
            datum_c: get_f64_vec("datum_c")?,
            bore_points: get_f64_vec("bore_points")?,
            bore_nominal_xy_m: bore_xy,
            flatness_limit_m: get_str("flatness_limit_m")?,
            diameter_lower_m: get_str("diameter_lower_m")?,
            diameter_upper_m: get_str("diameter_upper_m")?,
            position_limit_m: get_str("position_limit_m")?,
            perp_limit_m: get_str("perp_limit_m")?,
        })
    }

    /// Serialises this job specification to deterministic canonical JSON.
    pub fn to_cvalue(&self) -> Result<CValue, SliceError> {
        let mut obj = BTreeMap::new();
        obj.insert("part_number".into(), CValue::Str(self.part_number.clone()));
        obj.insert("revision".into(), CValue::Str(self.revision.clone()));
        obj.insert("serial_number".into(), CValue::Str(self.serial_number.clone()));
        obj.insert("dialect".into(), CValue::Str(self.dialect.clone()));
        obj.insert("datum_a".into(), f64_slice_to_cvalue(&self.datum_a)?);
        obj.insert("datum_b".into(), f64_slice_to_cvalue(&self.datum_b)?);
        obj.insert("datum_c".into(), f64_slice_to_cvalue(&self.datum_c)?);
        obj.insert("bore_points".into(), f64_slice_to_cvalue(&self.bore_points)?);
        obj.insert("bore_nominal_xy_m".into(), f64_slice_to_cvalue(&self.bore_nominal_xy_m)?);
        obj.insert("flatness_limit_m".into(), CValue::Str(self.flatness_limit_m.clone()));
        obj.insert("diameter_lower_m".into(), CValue::Str(self.diameter_lower_m.clone()));
        obj.insert("diameter_upper_m".into(), CValue::Str(self.diameter_upper_m.clone()));
        obj.insert("position_limit_m".into(), CValue::Str(self.position_limit_m.clone()));
        obj.insert("perp_limit_m".into(), CValue::Str(self.perp_limit_m.clone()));
        Ok(CValue::Obj(obj))
    }
}

/// Builds a deterministic, realistic synthetic aerospace bracket inspection job.
///
/// Geometry (SI metres):
/// - Datum A (`z ≈ 0`, outward `+Z`): 16 points over `0.100 × 0.080 m` with 4 µm peak-to-valley form.
/// - Datum B (`y ≈ 0`, outward `-Y`): 9 points over `0.100 × 0.030 m` with 3 µm form.
/// - Datum C (`x ≈ 0`, outward `-X`): 9 points over `0.080 × 0.030 m` with 2 µm form.
/// - Inspected Bore: nominal `Ø 20.000 ± 0.020 mm` at `(x = 50.000 mm, y = 40.000 mm)` in `[A|B|C]`,
///   measured with 36 points across 3 axial rings, actual diameter `20.004 mm` and `+6 µm` X offset.
pub fn demo_job() -> JobSpec {
    let mut datum_a = Vec::with_capacity(16 * 3);
    for ix in 0..4 {
        for iy in 0..4 {
            let x = 0.010 + (ix as f64) * 0.025;
            let y = 0.010 + (iy as f64) * 0.020;
            // Subtle deterministic 4 um saddle form below z = 0.
            let z = -0.000_002 * (((ix + iy) % 3) as f64);
            datum_a.extend_from_slice(&[x, y, z]);
        }
    }

    let mut datum_b = Vec::with_capacity(9 * 3);
    for ix in 0..3 {
        for iz in 0..3 {
            let x = 0.015 + (ix as f64) * 0.035;
            let y = 0.000_001 * (((ix + iz) % 3) as f64);
            let z = -0.005 - (iz as f64) * 0.010;
            datum_b.extend_from_slice(&[x, y, z]);
        }
    }

    let mut datum_c = Vec::with_capacity(9 * 3);
    for iy in 0..3 {
        for iz in 0..3 {
            let x = 0.000_001 * (((iy + iz) % 2) as f64);
            let y = 0.015 + (iy as f64) * 0.025;
            let z = -0.005 - (iz as f64) * 0.010;
            datum_c.extend_from_slice(&[x, y, z]);
        }
    }

    let bore_points = synth_bore_points([0.050_006, 0.040_003], 0.010_002, 0.000_05, 0.000_001);

    // In the 3-2-1 DRF built from outward normals A=+Z, B=-Y, C=-X:
    // z = +Z, y = -Y, x = y × z = -X, so part point (+50, +40) mm is (-50, -40) mm in DRF.
    JobSpec {
        part_number: "MM-BRKT-001".into(),
        revision: "A".into(),
        serial_number: "SN-2026-0001".into(),
        dialect: "ASME".into(),
        datum_a,
        datum_b,
        datum_c,
        bore_points,
        bore_nominal_xy_m: [-0.050, -0.040],
        flatness_limit_m: "0.000025".into(),
        diameter_lower_m: "0.019980".into(),
        diameter_upper_m: "0.020020".into(),
        position_limit_m: "0.000050".into(),
        perp_limit_m: "0.000030".into(),
    }
}

/// Deterministic bore point generator used by [`demo_job`] and tests (36 points across 3 rings).
pub fn synth_bore_points(center_xy_m: [f64; 2], radius_m: f64, tilt_x_rad: f64, radial_ripple_m: f64) -> Vec<f64> {
    let mut pts = Vec::with_capacity(36 * 3);
    let ring_z = [-0.005, -0.015, -0.025];
    for (rz_idx, &z) in ring_z.iter().enumerate() {
        for k in 0..12 {
            let theta = (k as f64) * (core::f64::consts::TAU / 12.0);
            let ripple = if (k + rz_idx) % 2 == 0 { radial_ripple_m } else { -radial_ripple_m };
            let r = radius_m + ripple;
            let cx = center_xy_m[0] + tilt_x_rad * (z + 0.015);
            let cy = center_xy_m[1];
            let x = cx + r * theta.cos();
            let y = cy + r * theta.sin();
            pts.extend_from_slice(&[x, y, z]);
        }
    }
    pts
}

/// One evaluated characteristic record in the slice output.
#[derive(Debug, Clone, PartialEq)]
pub struct CharacteristicResult {
    pub balloon: u32,
    pub id: String,
    pub label: String,
    pub requirement: String,
    pub algorithm_id: String,
    pub value_m: f64,
    pub formatted_mm: String,
    pub formatter_id: String,
    pub side: String,
    pub outcome: Outcome,
    pub rule_id: String,
    pub uec: String,
    pub evaluation_basis: String,
    pub driving_index: Option<u32>,
}

/// One entry in the hash-chained semantic journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    pub seq: u64,
    pub command_id: String,
    pub prev_hash: Option<Digest>,
    pub result_digest: Digest,
    pub entry_hash: Digest,
}

/// Complete verified output of one Vertical Slice 0 run.
#[derive(Debug, Clone)]
pub struct SliceOutput {
    pub part_number: String,
    pub revision: String,
    pub serial_number: String,
    pub drf_origin_m: [f64; 3],
    pub drf_axes: [[f64; 3]; 3],
    pub characteristics: Vec<CharacteristicResult>,
    pub journal: Vec<JournalEntry>,
    pub journal_head: Digest,
    pub form3_csv: String,
    pub records_json: Vec<u8>,
    pub package_bytes: Vec<u8>,
    pub package_digest: Digest,
    pub verified_members: usize,
}

/// Executes the full Vertical Slice 0 pipeline end-to-end and verifies the generated Result Package.
pub fn run_slice(job: &JobSpec) -> Result<SliceOutput, SliceError> {
    let mut journal = Vec::new();

    // Step 1: Hash and record imported points (canonical JSON).
    let job_cval = job.to_cvalue()?;
    let job_bytes = canonicalize_cvalue(&job_cval)?;
    let points_digest = to_fid_digest(&job_bytes)?;
    append_journal(&mut journal, "import.measured_points@1", points_digest)?;

    // Step 2: Build 3-2-1 Datum Reference Frame via certified tangential planes (fid-gps + fid-fit).
    let sim_alg = AlgId::parse(DATUM_SIM_ALG).map_err(|e| SliceError::new("slice.drf", e.to_string()))?;
    let dmt_key = DmtKey::parse(DATUM_DMT_KEY).map_err(|e| SliceError::new("slice.drf", e.to_string()))?;
    let planes = [
        MeasuredPlane { label: "A", xyz: &job.datum_a, nominal_outward: [0.0, 0.0, 1.0] },
        MeasuredPlane { label: "B", xyz: &job.datum_b, nominal_outward: [0.0, -1.0, 0.0] },
        MeasuredPlane { label: "C", xyz: &job.datum_c, nominal_outward: [-1.0, 0.0, 0.0] },
    ];
    let measured_drf = build_measured_planar_drf(&planes, &sim_alg, &dmt_key)
        .map_err(|e| SliceError::new("slice.drf", e.to_string()))?;
    let drf = measured_drf.drf();
    let drf_origin = drf.origin();
    let drf_axes = drf.axes();
    let drf_digest = hash_cvalue(&drf_to_cvalue(drf_origin, drf_axes)?)?;
    append_journal(&mut journal, "gdt.build_drf@1", drf_digest)?;

    // Step 3: Fit Datum A least-squares plane (fid-fit::ls_lin) and Bore cylinder (fid-fit::ls_cyl).
    let ls_params = FitParams::new(Method::LeastSquares);
    let datum_a_view = PointsView::new(&job.datum_a, None).map_err(|e| SliceError::new("slice.fit", e.to_string()))?;
    let plane_a_fit =
        PlaneLs.fit(&datum_a_view, &ls_params, None).map_err(|e| SliceError::new("slice.fit", e.to_string()))?;
    let (plane_a_point, plane_a_normal) = match &plane_a_fit.feature {
        FittedFeature::Plane { p, n } => (*p, *n),
        _ => return Err(SliceError::new("slice.fit", "expected fitted plane")),
    };

    let bore_view = PointsView::new(&job.bore_points, None).map_err(|e| SliceError::new("slice.fit", e.to_string()))?;
    let cyl_fit: CylinderFit =
        fit_cylinder(&bore_view, &ls_params, Some(0)).map_err(|e| SliceError::new("slice.fit", e.to_string()))?;
    let (bore_p, bore_d, bore_r) = match &cyl_fit.result.feature {
        FittedFeature::Cylinder { p, d, r } => (*p, *d, *r),
        _ => return Err(SliceError::new("slice.fit", "expected fitted cylinder")),
    };
    let bore_fit_digest = hash_cvalue(&cyl_fit_to_cvalue(bore_p, bore_d, bore_r, cyl_fit.result.stats.rms)?)?;
    append_journal(&mut journal, "fit.ls_cylinder@1", bore_fit_digest)?;

    // Step 4: Evaluate 4 GD&T / dimensional characteristics with closed-form ZoneShape & fmt.length@1.
    let rule_id = RuleId::parse(DECISION_RULE_ID).map_err(|e| SliceError::new("slice.rule", e.to_string()))?;

    // Balloon 1: Flatness of Datum A (TwoPlanes zone centred on LS plane).
    let datum_a_pts = triples_to_p3(&job.datum_a);
    let flat_pose = Pose::new(plane_a_point, plane_a_normal);
    let flat_usage = ZoneShape::TwoPlanes
        .usage(&flat_pose, ZoneInput::Points(&datum_a_pts))
        .map_err(|e| SliceError::new("slice.zone", e.to_string()))?;
    let ch1 = build_char_result(
        1,
        "CHAR-001",
        "Datum A Flatness",
        format!("Flatness <= {} mm", m_str_to_mm_label(&job.flatness_limit_m)?),
        "gdt.zone.two_planes@1.0.0",
        flat_usage.size_m,
        None,
        Some(&job.flatness_limit_m),
        &rule_id,
        Some(flat_usage.driving_index),
    )?;

    // Balloon 2: Bore Diameter (2 * radius from KNP-1 least-squares cylinder).
    let bore_dia_m = 2.0 * bore_r;
    let ch2 = build_char_result(
        2,
        "CHAR-002",
        "Bore Diameter",
        format!("Dia {}..{} mm", m_str_to_mm_label(&job.diameter_lower_m)?, m_str_to_mm_label(&job.diameter_upper_m)?),
        &cyl_fit.result.algorithm.to_string(),
        bore_dia_m,
        Some(&job.diameter_lower_m),
        Some(&job.diameter_upper_m),
        &rule_id,
        None,
    )?;

    // Build two axis endpoints spanning the measured bore depth along the fitted axis.
    let bore_pts = triples_to_p3(&job.bore_points);
    let (axis_end_lo, axis_end_hi) = bore_axis_endpoints(&bore_pts, bore_p, bore_d)?;

    // Balloon 3: True Position of Bore axis to [A|B|C] (Cylinder zone along DRF z at nominal XY).
    let nom_x = job.bore_nominal_xy_m[0];
    let nom_y = job.bore_nominal_xy_m[1];
    let nom_origin = [
        drf_origin[0] + nom_x * drf_axes[0][0] + nom_y * drf_axes[1][0],
        drf_origin[1] + nom_x * drf_axes[0][1] + nom_y * drf_axes[1][1],
        drf_origin[2] + nom_x * drf_axes[0][2] + nom_y * drf_axes[1][2],
    ];
    let pos_pose = Pose::new(nom_origin, drf_axes[2]);
    let axis_pair = [axis_end_lo, axis_end_hi];
    let pos_usage = ZoneShape::Cylinder
        .usage(&pos_pose, ZoneInput::Points(&axis_pair))
        .map_err(|e| SliceError::new("slice.zone", e.to_string()))?;
    let ch3 = build_char_result(
        3,
        "CHAR-003",
        "Bore True Position [A|B|C]",
        format!("Pos Dia {} | A | B | C", m_str_to_mm_label(&job.position_limit_m)?),
        "gdt.zone.cylinder@1.0.0",
        pos_usage.size_m,
        None,
        Some(&job.position_limit_m),
        &rule_id,
        Some(pos_usage.driving_index),
    )?;

    // Balloon 4: Perpendicularity of Bore axis to [A] (Cylinder zone along DRF z at bore midpoint).
    let perp_pose = Pose::new(bore_p, drf_axes[2]);
    let perp_usage = ZoneShape::Cylinder
        .usage(&perp_pose, ZoneInput::Points(&axis_pair))
        .map_err(|e| SliceError::new("slice.zone", e.to_string()))?;
    let ch4 = build_char_result(
        4,
        "CHAR-004",
        "Bore Perpendicularity [A]",
        format!("Perp Dia {} | A", m_str_to_mm_label(&job.perp_limit_m)?),
        "gdt.zone.cylinder@1.0.0",
        perp_usage.size_m,
        None,
        Some(&job.perp_limit_m),
        &rule_id,
        Some(perp_usage.driving_index),
    )?;

    let characteristics = vec![ch1, ch2, ch3, ch4];
    let records_cval = records_to_cvalue(job, &characteristics)?;
    let records_json = canonicalize_cvalue(&records_cval)?;
    let records_digest = to_fid_digest(&records_json)?;
    append_journal(&mut journal, "gdt.evaluate_characteristics@1", records_digest)?;

    // Step 5: Generate AS9102 Form 3 starter CSV and canonical journal/profile/kernel artifacts.
    let form3_csv = render_form3_csv(job, &characteristics);
    let csv_digest = to_fid_digest(form3_csv.as_bytes())?;
    append_journal(&mut journal, "report.export_form3_csv@1", csv_digest)?;

    let journal_head =
        journal.last().map(|e| e.entry_hash).ok_or_else(|| SliceError::new("slice.internal", "journal is empty"))?;

    let journal_bytes = canonicalize_cvalue(&journal_to_cvalue(&journal))?;
    let profile_bytes = canonicalize_cvalue(&profile_to_cvalue(job))?;
    let kernel_bytes = canonicalize_cvalue(&kernel_to_cvalue())?;
    let profile_digest = to_fid_digest(&profile_bytes)?;
    let kernel_manifest_digest = to_fid_digest(&kernel_bytes)?;

    // Step 6: Build deterministic ZIP Result Package via fid-pkg and verify it fail-closed.
    let pkg_input = PackageInput {
        created_by: format!("fid-slice/{}", env!("CARGO_PKG_VERSION")),
        evidence_tier: EvidenceTier::Points,
        profile_digest,
        kernel_manifest_digest,
        result_count: characteristics.len() as u64,
        members: vec![
            member("journal/events.json", "application/json", journal_bytes)?,
            member("kernel/algorithms.json", "application/json", kernel_bytes)?,
            member("points/measured.json", "application/json", job_bytes)?,
            member("profile/spec.json", "application/json", profile_bytes)?,
            member("results/form3.csv", "text/csv", form3_csv.as_bytes().to_vec())?,
            member("results/records.json", "application/json", records_json.clone())?,
        ],
    };
    let built = build_package(&pkg_input).map_err(|e| SliceError::new("slice.pkg", e.to_string()))?;
    let package_bytes = built.container;
    let package_digest = to_fid_digest(&package_bytes)?;

    let verified_members = verify_package_bytes(&package_bytes)?;

    Ok(SliceOutput {
        part_number: job.part_number.clone(),
        revision: job.revision.clone(),
        serial_number: job.serial_number.clone(),
        drf_origin_m: drf_origin,
        drf_axes,
        characteristics,
        journal,
        journal_head,
        form3_csv,
        records_json,
        package_bytes,
        package_digest,
        verified_members,
    })
}

/// Opens and fail-closed verifies a `.zip` Result Package via `fid_pkg::reader::PackageReader`.
pub fn verify_package_bytes(bytes: &[u8]) -> Result<usize, SliceError> {
    let mut reader = PackageReader::open(Cursor::new(bytes))
        .map_err(|e| SliceError::new("slice.verify", format!("open failed: {e}")))?;
    reader.verify_all().map_err(|e| SliceError::new("slice.verify", format!("verification failed: {e}")))?;
    Ok(reader.manifest().members.len())
}

#[allow(clippy::too_many_arguments)]
fn build_char_result(
    balloon: u32,
    id: &str,
    label: &str,
    requirement: String,
    algorithm_id: &str,
    value_m: f64,
    lower_m: Option<&str>,
    upper_m: Option<&str>,
    rule_id: &RuleId,
    driving_index: Option<u32>,
) -> Result<CharacteristicResult, SliceError> {
    let lower = match lower_m {
        Some(s) => Some(Decimal::parse(s).map_err(|e| SliceError::new("slice.limit", e.to_string()))?),
        None => None,
    };
    let upper = match upper_m {
        Some(s) => Some(Decimal::parse(s).map_err(|e| SliceError::new("slice.limit", e.to_string()))?),
        None => None,
    };
    let spec = FormatSpec {
        formatter: FormatterId::LENGTH_V1,
        unit: Unit::Mm,
        precision: Precision::Decimals(4),
        limits: Some(Limits { lower, upper }),
    };
    let reported: Reported = fmt_kernel(value_m, &spec).map_err(|e| SliceError::new("slice.format", e.to_string()))?;

    let (outcome, side_str) = match reported.side {
        SideOutcome::Within | SideOutcome::NoLimits => (Outcome::Conforms, "within"),
        SideOutcome::AboveUpper => (Outcome::Nonconforms, "above"),
        SideOutcome::BelowLower => (Outcome::Nonconforms, "below"),
        SideOutcome::Undecidable => (Outcome::NotDecidable(NotDecidableReason::RuleMissing), "undecidable"),
    };

    Ok(CharacteristicResult {
        balloon,
        id: id.to_owned(),
        label: label.to_owned(),
        requirement,
        algorithm_id: algorithm_id.to_owned(),
        value_m,
        formatted_mm: format!("{} mm", reported.value),
        formatter_id: reported.formatter.to_string(),
        side: side_str.to_owned(),
        outcome,
        rule_id: rule_id.to_string(),
        uec: "UEC-0".to_owned(),
        evaluation_basis: "POINTS".to_owned(),
        driving_index,
    })
}

fn bore_axis_endpoints(pts: &[P3], p0: P3, d: P3) -> Result<(P3, P3), SliceError> {
    let mut t_min = f64::INFINITY;
    let mut t_max = f64::NEG_INFINITY;
    for p in pts {
        let rx = p[0] - p0[0];
        let ry = p[1] - p0[1];
        let rz = p[2] - p0[2];
        let t = (rx * d[0] + ry * d[1]) + rz * d[2];
        if !t.is_finite() {
            return Err(SliceError::new("slice.zone", "non-finite axial projection"));
        }
        if t < t_min {
            t_min = t;
        }
        if t > t_max {
            t_max = t;
        }
    }
    if !t_min.is_finite() || !t_max.is_finite() {
        return Err(SliceError::new("slice.zone", "empty bore point set"));
    }
    let lo = [p0[0] + t_min * d[0], p0[1] + t_min * d[1], p0[2] + t_min * d[2]];
    let hi = [p0[0] + t_max * d[0], p0[1] + t_max * d[1], p0[2] + t_max * d[2]];
    Ok((lo, hi))
}

fn triples_to_p3(flat: &[f64]) -> Vec<P3> {
    let mut out = Vec::with_capacity(flat.len() / 3);
    for c in flat.chunks_exact(3) {
        out.push([c[0], c[1], c[2]]);
    }
    out
}

fn m_str_to_mm_label(metres: &str) -> Result<String, SliceError> {
    // Validate exact decimal syntax first via KRN.NUM.KDEC.
    let _ = Decimal::parse(metres).map_err(|e| SliceError::new("slice.limit", e.to_string()))?;
    let val_m = metres.parse::<f64>().map_err(|e| SliceError::new("slice.limit", e.to_string()))?;
    let spec = FormatSpec {
        formatter: FormatterId::LENGTH_V1,
        unit: Unit::Mm,
        precision: Precision::Decimals(3),
        limits: None,
    };
    let r = fmt_kernel(val_m, &spec).map_err(|e| SliceError::new("slice.format", e.to_string()))?;
    Ok(r.value)
}

fn member(path: &str, media_type: &str, body: Vec<u8>) -> Result<MemberInput, SliceError> {
    let p = PkgPath::parse(path).map_err(|e| SliceError::new("slice.pkg", e.to_string()))?;
    Ok(MemberInput { path: p, media_type: media_type.to_owned(), body })
}

fn append_journal(journal: &mut Vec<JournalEntry>, command_id: &str, result_digest: Digest) -> Result<(), SliceError> {
    let seq = (journal.len() as u64) + 1;
    let prev_hash = journal.last().map(|e| e.entry_hash);
    let mut obj = BTreeMap::new();
    obj.insert("command_id".into(), CValue::Str(command_id.to_owned()));
    obj.insert(
        "prev_hash".into(),
        match &prev_hash {
            Some(h) => CValue::Str(h.to_string()),
            None => CValue::Null,
        },
    );
    obj.insert("result_digest".into(), CValue::Str(result_digest.to_string()));
    obj.insert("seq".into(), CValue::Num(seq as f64));
    let entry_hash = hash_cvalue(&CValue::Obj(obj))?;
    journal.push(JournalEntry { seq, command_id: command_id.to_owned(), prev_hash, result_digest, entry_hash });
    Ok(())
}

fn render_form3_csv(job: &JobSpec, chars: &[CharacteristicResult]) -> String {
    let mut out = String::from(
        "balloon,char_id,part_number,revision,serial_number,label,requirement,result,outcome,rule_id,uec,evaluation_basis,algorithm_id,formatter_id\n",
    );
    for c in chars {
        let fields = [
            c.balloon.to_string(),
            c.id.clone(),
            job.part_number.clone(),
            job.revision.clone(),
            job.serial_number.clone(),
            c.label.clone(),
            c.requirement.clone(),
            c.formatted_mm.clone(),
            outcome_str(c.outcome).to_owned(),
            c.rule_id.clone(),
            c.uec.clone(),
            c.evaluation_basis.clone(),
            c.algorithm_id.clone(),
            c.formatter_id.clone(),
        ];
        for (i, field) in fields.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&csv_escape_safe(field));
        }
        out.push('\n');
    }
    out
}

/// Neutralises spreadsheet formula-injection prefixes on untrusted free text and RFC-4180 quotes.
fn csv_escape_safe(s: &str) -> String {
    let safe = match s.as_bytes().first() {
        Some(b'=' | b'@' | b'\t' | b'\r') => format!("'{s}"),
        _ => s.to_owned(),
    };
    if safe.contains(',') || safe.contains('"') || safe.contains('\n') {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

pub fn outcome_str(o: Outcome) -> &'static str {
    o.wire()
}

fn f64_to_cvalue(v: f64) -> Result<CValue, SliceError> {
    if !v.is_finite() {
        return Err(SliceError::new("slice.canon", "non-finite float"));
    }
    Ok(CValue::Num(v))
}

fn f64_slice_to_cvalue(xs: &[f64]) -> Result<CValue, SliceError> {
    let mut arr = Vec::with_capacity(xs.len());
    for &x in xs {
        arr.push(f64_to_cvalue(x)?);
    }
    Ok(CValue::Arr(arr))
}

fn drf_to_cvalue(origin: [f64; 3], axes: [[f64; 3]; 3]) -> Result<CValue, SliceError> {
    let mut obj = BTreeMap::new();
    obj.insert("origin_m".into(), f64_slice_to_cvalue(&origin)?);
    obj.insert("x".into(), f64_slice_to_cvalue(&axes[0])?);
    obj.insert("y".into(), f64_slice_to_cvalue(&axes[1])?);
    obj.insert("z".into(), f64_slice_to_cvalue(&axes[2])?);
    Ok(CValue::Obj(obj))
}

fn cyl_fit_to_cvalue(p: [f64; 3], d: [f64; 3], r: f64, rms: f64) -> Result<CValue, SliceError> {
    let mut obj = BTreeMap::new();
    obj.insert("point_m".into(), f64_slice_to_cvalue(&p)?);
    obj.insert("direction".into(), f64_slice_to_cvalue(&d)?);
    obj.insert("radius_m".into(), f64_to_cvalue(r)?);
    obj.insert("rms_m".into(), f64_to_cvalue(rms)?);
    Ok(CValue::Obj(obj))
}

fn records_to_cvalue(job: &JobSpec, chars: &[CharacteristicResult]) -> Result<CValue, SliceError> {
    let mut rows = Vec::with_capacity(chars.len());
    for c in chars {
        let mut r = BTreeMap::new();
        r.insert("algorithm_id".into(), CValue::Str(c.algorithm_id.clone()));
        r.insert("balloon".into(), CValue::Num(f64::from(c.balloon)));
        r.insert("characteristic_id".into(), CValue::Str(c.id.clone()));
        r.insert("evaluation_basis".into(), CValue::Str(c.evaluation_basis.clone()));
        r.insert("formatted_value".into(), CValue::Str(c.formatted_mm.clone()));
        r.insert("formatter_id".into(), CValue::Str(c.formatter_id.clone()));
        r.insert("label".into(), CValue::Str(c.label.clone()));
        r.insert("outcome".into(), CValue::Str(outcome_str(c.outcome).to_owned()));
        r.insert("requirement".into(), CValue::Str(c.requirement.clone()));
        r.insert("rule_id".into(), CValue::Str(c.rule_id.clone()));
        r.insert("side".into(), CValue::Str(c.side.clone()));
        r.insert("uec".into(), CValue::Str(c.uec.clone()));
        r.insert("value_m".into(), f64_to_cvalue(c.value_m)?);
        if let Some(idx) = c.driving_index {
            r.insert("driving_index".into(), CValue::Num(f64::from(idx)));
        }
        rows.push(CValue::Obj(r));
    }
    let mut root = BTreeMap::new();
    root.insert("dialect".into(), CValue::Str(job.dialect.clone()));
    root.insert("part_number".into(), CValue::Str(job.part_number.clone()));
    root.insert("records".into(), CValue::Arr(rows));
    root.insert("revision".into(), CValue::Str(job.revision.clone()));
    root.insert("serial_number".into(), CValue::Str(job.serial_number.clone()));
    Ok(CValue::Obj(root))
}

fn journal_to_cvalue(entries: &[JournalEntry]) -> CValue {
    let arr = entries
        .iter()
        .map(|e| {
            let mut obj = BTreeMap::new();
            obj.insert("command_id".into(), CValue::Str(e.command_id.clone()));
            obj.insert("entry_hash".into(), CValue::Str(e.entry_hash.to_string()));
            obj.insert(
                "prev_hash".into(),
                match &e.prev_hash {
                    Some(h) => CValue::Str(h.to_string()),
                    None => CValue::Null,
                },
            );
            obj.insert("result_digest".into(), CValue::Str(e.result_digest.to_string()));
            obj.insert("seq".into(), CValue::Num(e.seq as f64));
            CValue::Obj(obj)
        })
        .collect();
    CValue::Arr(arr)
}

fn profile_to_cvalue(job: &JobSpec) -> CValue {
    let mut obj = BTreeMap::new();
    obj.insert("decision_rule".into(), CValue::Str(DECISION_RULE_ID.to_owned()));
    obj.insert("dialect".into(), CValue::Str(job.dialect.clone()));
    obj.insert("governance_profile".into(), CValue::Str("lean".to_owned()));
    obj.insert("slice".into(), CValue::Str("vertical-slice-0".to_owned()));
    CValue::Obj(obj)
}

fn kernel_to_cvalue() -> CValue {
    let mut obj = BTreeMap::new();
    obj.insert("datum_dmt_key".into(), CValue::Str(DATUM_DMT_KEY.to_owned()));
    obj.insert("datum_sim_alg".into(), CValue::Str(DATUM_SIM_ALG.to_owned()));
    obj.insert("formatter".into(), CValue::Str(FormatterId::LENGTH_V1.to_string()));
    obj.insert("numeric_profile".into(), CValue::Str("KNP-1".to_owned()));
    obj.insert("slice_version".into(), CValue::Str(env!("CARGO_PKG_VERSION").to_owned()));
    CValue::Obj(obj)
}

fn canonicalize_cvalue(v: &CValue) -> Result<Vec<u8>, SliceError> {
    to_canonical_bytes(v).map_err(|e| SliceError::new("slice.canon", e.to_string()))
}

fn hash_cvalue(v: &CValue) -> Result<Digest, SliceError> {
    let bytes = canonicalize_cvalue(v)?;
    to_fid_digest(&bytes)
}

fn to_fid_digest(bytes: &[u8]) -> Result<Digest, SliceError> {
    let d = digest_bytes(bytes);
    Digest::parse(&d.to_string()).map_err(|e| SliceError::new("slice.digest", e.to_string()))
}
