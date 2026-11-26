//! ALN.BFIT.ICP.METRIC acceptance criteria AC-1 to AC-4 plus REQ-005 and the refusals. Every
//! expected row in AC-1 and every finite difference in AC-2 comes from the fixtures
//! `fixtures/aln/bfit/icp/metric_rows.json` and `metric_fd.json`, written by the independent
//! Python reference `fixtures/aln/bfit/icp/make_metric_fixtures.py` (exact rational arithmetic,
//! no libm), never from running this crate. AC-3 and AC-4 are hand-labelled from the leaf's
//! rules (weight 4 doubles a row exactly; locked columns are dropped). CI runs this file on
//! Linux, Windows, macOS and wasm32-wasip1, so the AC-1 bit pins hold on each.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align_solve::{
    build_rows, validate, ConstraintSet, CorrReason, Correspondences, Dof, DofLocks, FrameContext, InputField,
    LockFrameRef, Metric, MetricError, WeightSpec, NORMAL_TOL, POINT_TO_PLANE_ALGORITHM_ID,
    POINT_TO_POINT_ALGORITHM_ID,
};
use fid_geo_frame::{FrameId, FrameTree, Rigid};

// ------------------------------------------------------------------ a small JSON reader

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
enum J {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) {
        self.ws();
        assert_eq!(self.b[self.i], c, "expected {} at {}", c as char, self.i);
        self.i += 1;
    }

    fn value(&mut self) -> J {
        self.ws();
        match self.b[self.i] {
            b'{' => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.b[self.i] == b'}' {
                    self.i += 1;
                    return J::Obj(out);
                }
                loop {
                    self.ws();
                    let J::Str(k) = self.value() else { panic!("object key") };
                    assert!(out.iter().all(|(o, _)| *o != k), "duplicate key {k}");
                    self.eat(b':');
                    let v = self.value();
                    out.push((k, v));
                    self.ws();
                    let c = self.b[self.i];
                    self.i += 1;
                    match c {
                        b',' => continue,
                        b'}' => return J::Obj(out),
                        _ => panic!("bad object at {}", self.i),
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.b[self.i] == b']' {
                    self.i += 1;
                    return J::Arr(out);
                }
                loop {
                    out.push(self.value());
                    self.ws();
                    let c = self.b[self.i];
                    self.i += 1;
                    match c {
                        b',' => continue,
                        b']' => return J::Arr(out),
                        _ => panic!("bad array at {}", self.i),
                    }
                }
            }
            b'"' => {
                self.i += 1;
                let start = self.i;
                while self.b[self.i] != b'"' {
                    assert_ne!(self.b[self.i], b'\\', "escapes are not used by the fixtures");
                    self.i += 1;
                }
                let s = std::str::from_utf8(&self.b[start..self.i]).unwrap().to_string();
                self.i += 1;
                J::Str(s)
            }
            b't' => {
                self.i += 4;
                J::Bool(true)
            }
            b'f' => {
                self.i += 5;
                J::Bool(false)
            }
            b'n' => {
                self.i += 4;
                J::Null
            }
            _ => {
                let start = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    self.i += 1;
                }
                J::Num(std::str::from_utf8(&self.b[start..self.i]).unwrap().to_string())
            }
        }
    }
}

impl J {
    fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => panic!("not an object"),
        }
    }

    fn at(&self, key: &str) -> &J {
        self.get(key).unwrap_or_else(|| panic!("missing key {key}"))
    }

    fn arr(&self) -> &[J] {
        match self {
            J::Arr(v) => v,
            _ => panic!("not an array"),
        }
    }

    /// A number, or the strings the reference uses for non-finite values.
    fn f(&self) -> f64 {
        match self {
            J::Num(s) => s.parse().unwrap(),
            J::Str(s) if s == "NaN" => f64::NAN,
            J::Str(s) if s == "Infinity" => f64::INFINITY,
            J::Str(s) if s == "-Infinity" => f64::NEG_INFINITY,
            _ => panic!("not a number: {self:?}"),
        }
    }

    fn u(&self) -> u64 {
        match self {
            J::Num(s) => s.parse().unwrap(),
            _ => panic!("not an integer"),
        }
    }
}

impl J {
    fn f3(&self) -> [f64; 3] {
        let a = self.arr();
        assert_eq!(a.len(), 3);
        [a[0].f(), a[1].f(), a[2].f()]
    }

    fn fs(&self) -> Vec<f64> {
        self.arr().iter().map(J::f).collect()
    }

    fn rows6(&self) -> Vec<[f64; 6]> {
        self.arr()
            .iter()
            .map(|r| {
                let v = r.fs();
                assert_eq!(v.len(), 6);
                [v[0], v[1], v[2], v[3], v[4], v[5]]
            })
            .collect()
    }
}

fn load(name: &str) -> J {
    let path = format!("{}/fixtures/aln/bfit/icp/{name}", fid_testkit::WORKSPACE_ROOT);
    let text = std::fs::read(path).unwrap();
    let mut p = P { b: &text, i: 0 };
    let v = p.value();
    p.ws();
    assert_eq!(p.i, text.len(), "trailing bytes in {name}");
    v
}

// ------------------------------------------------------------------ helpers

fn cs_with(n: usize, locks: DofLocks, per_point: Option<Vec<f64>>) -> ConstraintSet {
    let tree = FrameTree::new();
    let ctx = FrameContext { tree: &tree, parent: FrameId::WORLD };
    let w = WeightSpec { per_point, per_region: Vec::new() };
    validate(locks, LockFrameRef::Parent, &ctx, &Rigid::IDENTITY, &w, n).unwrap()
}

fn cs(n: usize) -> ConstraintSet {
    cs_with(n, DofLocks::NONE, None)
}

fn corr(foot: Vec<[f64; 3]>, normal: Vec<[f64; 3]>) -> Correspondences {
    let n = foot.len();
    Correspondences {
        foot,
        normal,
        entity: (0..n as u64).collect(),
        dist: vec![0.0; n],
        valid: vec![true; n],
        reason: vec![CorrReason::Hit; n],
    }
}

fn invalidate(c: &mut Correspondences, i: usize, reason: CorrReason) {
    c.valid[i] = false;
    c.reason[i] = reason;
    c.foot[i] = [f64::NAN; 3];
    c.normal[i] = [f64::NAN; 3];
    c.dist[i] = f64::NAN;
}

/// Three valid correspondences with unit normals (within NORMAL_TOL).
fn three() -> (Vec<[f64; 3]>, Correspondences) {
    let moved = vec![[0.1, 0.2, 0.3], [-0.4, 0.05, 0.7], [1.25, -0.5, 0.125]];
    let foot = vec![[0.1, 0.2, 0.29], [-0.41, 0.06, 0.7], [1.2, -0.5, 0.1]];
    let normal = vec![[0.0, 0.0, 1.0], [0.6, -0.8, 0.0], [0.0, 0.8, 0.6]];
    (moved, corr(foot, normal))
}

fn full_rows(rows: &fid_align_solve::Rows) -> Vec<Vec<f64>> {
    let j = rows.j();
    (0..j.rows()).map(|i| j.row(i).unwrap().to_vec()).collect()
}

fn same_bits(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_rows_match_the_analytic_values() {
    let fx = load("metric_rows.json");
    let cases = fx.at("cases").arr();
    assert_eq!(cases.len(), 5);
    let moved: Vec<[f64; 3]> = cases.iter().map(|c| c.at("x").f3()).collect();
    let foot: Vec<[f64; 3]> = cases.iter().map(|c| c.at("q").f3()).collect();
    let normal: Vec<[f64; 3]> = cases.iter().map(|c| c.at("n").f3()).collect();
    let weights: Vec<f64> = cases.iter().map(|c| c.at("w").f()).collect();
    let c = corr(foot, normal);
    let set = cs(5);
    for m in Metric::ALL {
        let rows = build_rows(m, &moved, &c, &weights, &set).unwrap();
        assert_eq!(rows.metric(), m);
        let per = m.rows_per_point();
        assert_eq!(rows.len(), 5 * per);
        assert_eq!(rows.j().cols(), 6);
        let want_points: Vec<u64> = (0..5u64).flat_map(|i| std::iter::repeat_n(i, per)).collect();
        assert_eq!(rows.point_of_row(), &want_points[..]);
        for (i, case) in cases.iter().enumerate() {
            let e = case.at(m.name());
            for (kind, tol_check) in [("exact", true), ("pinned", false)] {
                let ex = e.at(kind);
                let er = ex.at("r").fs();
                let ej = ex.at("j").rows6();
                assert_eq!(er.len(), per);
                for k in 0..per {
                    let row = i * per + k;
                    let got_r = rows.r()[row];
                    let got_j = rows.j().row(row).unwrap();
                    if tol_check {
                        assert!((got_r - er[k]).abs() <= 1e-15, "{m} case {i} row {k}: r {got_r} vs {}", er[k]);
                        for (a, b) in got_j.iter().zip(&ej[k]) {
                            assert!((a - b).abs() <= 1e-15, "{m} case {i} row {k}: j {a} vs {b}");
                        }
                    } else {
                        assert_eq!(got_r.to_bits(), er[k].to_bits(), "{m} case {i} row {k}: r bits");
                        assert!(same_bits(got_j, &ej[k]), "{m} case {i} row {k}: j bits {got_j:?} vs {:?}", ej[k]);
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_jacobian_matches_central_finite_differences() {
    let fx = load("metric_fd.json");
    assert_eq!(fx.at("seed").u(), 5);
    let poses = fx.at("poses").arr();
    assert_eq!(poses.len(), 4);
    let mut checked = 0usize;
    for (p, pose) in poses.iter().enumerate() {
        let pts = pose.at("points").arr();
        let moved: Vec<[f64; 3]> = pts.iter().map(|c| c.at("x").f3()).collect();
        let foot: Vec<[f64; 3]> = pts.iter().map(|c| c.at("q").f3()).collect();
        let normal: Vec<[f64; 3]> = pts.iter().map(|c| c.at("n").f3()).collect();
        let w: Vec<f64> = pts.iter().map(|c| c.at("w").f()).collect();
        let set = cs_with(pts.len(), DofLocks::NONE, Some(w));
        let c = corr(foot, normal);
        for m in Metric::ALL {
            let rows = build_rows(m, &moved, &c, set.combined_weights(), &set).unwrap();
            let per = m.rows_per_point();
            assert_eq!(rows.len(), pts.len() * per);
            for (i, pt) in pts.iter().enumerate() {
                let fd = pt.at(m.name()).rows6();
                for (k, fr) in fd.iter().enumerate() {
                    let got = rows.j().row(i * per + k).unwrap();
                    let scale = fr.iter().fold(0.0f64, |a, v| if v.abs() > a { v.abs() } else { a });
                    assert!(scale > 0.0);
                    for (col, (a, b)) in got.iter().zip(fr).enumerate() {
                        assert!(
                            (a - b).abs() <= 1e-6 * scale,
                            "pose {p} point {i} {m} row {k} col {col}: analytic {a} vs fd {b}"
                        );
                    }
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 4 * 6 * 4);
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_weight_zero_drops_rows_and_weight_four_doubles_them() {
    let (moved, c) = three();
    let ones = cs(3);
    let weighted = cs_with(3, DofLocks::NONE, Some(vec![0.0, 1.0, 4.0]));
    assert_eq!(weighted.combined_weights(), &[0.0, 1.0, 4.0]);
    for m in Metric::ALL {
        let base = build_rows(m, &moved, &c, ones.combined_weights(), &ones).unwrap();
        let got = build_rows(m, &moved, &c, weighted.combined_weights(), &weighted).unwrap();
        let per = m.rows_per_point();
        assert_eq!(got.len(), 2 * per, "{m}: the weight-0 point contributes no rows");
        let want_points: Vec<u64> = [1u64, 2].iter().flat_map(|i| std::iter::repeat_n(*i, per)).collect();
        assert_eq!(got.point_of_row(), &want_points[..]);
        let bj = full_rows(&base);
        let gj = full_rows(&got);
        for k in 0..per {
            // point 1, weight 1: unchanged
            assert_eq!(got.r()[k].to_bits(), base.r()[per + k].to_bits());
            assert!(same_bits(&gj[k], &bj[per + k]));
            // point 2, weight 4: doubled (sqrt(4) = 2, exact)
            assert_eq!(got.r()[per + k].to_bits(), (2.0 * base.r()[2 * per + k]).to_bits());
            let doubled: Vec<f64> = bj[2 * per + k].iter().map(|v| 2.0 * v).collect();
            assert!(same_bits(&gj[per + k], &doubled), "{m} row {k}");
        }
    }
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_locks_on_tz_and_c_leave_tx_ty_a_b() {
    let (moved, c) = three();
    let free = cs(3);
    let locks = DofLocks::NONE.with(Dof::Tz).with(Dof::Rz);
    let locked = cs_with(3, locks, None);
    for m in Metric::ALL {
        let all = build_rows(m, &moved, &c, free.combined_weights(), &free).unwrap();
        let red = build_rows(m, &moved, &c, locked.combined_weights(), &locked).unwrap();
        assert_eq!(red.j().cols(), 4);
        assert_eq!(red.j().rows(), all.j().rows());
        assert!(same_bits(red.r(), all.r()));
        for (i, row) in full_rows(&all).iter().enumerate() {
            // tx, ty, a, b are columns 0, 1, 3, 4 of the full order tx, ty, tz, a, b, c.
            let want = [row[0], row[1], row[3], row[4]];
            assert!(same_bits(red.j().row(i).unwrap(), &want), "{m} row {i}");
        }
    }
}

// ------------------------------------------------------------------ REQ-005

#[test]
fn req005_metric_names_map_one_to_one_to_algorithm_ids() {
    assert_eq!(Metric::PointToPoint.algorithm_id(), "align.bestfit.point_to_point@1.0.0");
    assert_eq!(Metric::PointToPlane.algorithm_id(), "align.bestfit.point_to_plane@1.0.0");
    assert_eq!(POINT_TO_POINT_ALGORITHM_ID, Metric::PointToPoint.algorithm_id());
    assert_eq!(POINT_TO_PLANE_ALGORITHM_ID, Metric::PointToPlane.algorithm_id());
    for m in Metric::ALL {
        assert_eq!(Metric::from_name(m.name()), Some(m));
        assert_eq!(Metric::from_algorithm_id(m.algorithm_id()), Some(m));
        assert_eq!(m.to_string(), m.name());
    }
    assert_ne!(Metric::PointToPoint.algorithm_id(), Metric::PointToPlane.algorithm_id());
    assert_eq!(Metric::from_name("Point_To_Point"), None);
    assert_eq!(Metric::from_name("point_to_line"), None);
    assert_eq!(Metric::from_algorithm_id("align.bestfit.point_to_point@1.0.1"), None);
    assert_eq!(Metric::from_algorithm_id("align.bestfit.point_to_plane"), None);
}

// ------------------------------------------------------------------ skipping and refusals

#[test]
fn invalid_correspondences_contribute_no_rows_and_are_not_read() {
    let (moved, mut c) = three();
    invalidate(&mut c, 0, CorrReason::NoHit);
    invalidate(&mut c, 2, CorrReason::BeyondMaxDistance);
    let set = cs(3);
    for m in Metric::ALL {
        let rows = build_rows(m, &moved, &c, set.combined_weights(), &set).unwrap();
        assert_eq!(rows.len(), m.rows_per_point());
        assert!(rows.point_of_row().iter().all(|&p| p == 1));
    }
    // A zero-weight point's foot is not read either.
    let (moved, mut c) = three();
    c.foot[0] = [f64::NAN; 3];
    let rows = build_rows(Metric::PointToPoint, &moved, &c, &[-0.0, 1.0, 1.0], &set).unwrap();
    assert_eq!(rows.point_of_row(), &[1, 1, 1, 2, 2, 2]);
}

#[test]
fn empty_rows_when_nothing_contributes() {
    let (moved, mut c) = three();
    let set = cs(3);
    let err = build_rows(Metric::PointToPlane, &moved, &c, &[0.0, 0.0, 0.0], &set).unwrap_err();
    assert_eq!(err, MetricError::EmptyRows);
    assert_eq!(err.code(), "empty_rows");
    for i in 0..3 {
        invalidate(&mut c, i, CorrReason::NoHit);
    }
    let err = build_rows(Metric::PointToPoint, &moved, &c, set.combined_weights(), &set).unwrap_err();
    assert_eq!(err.code(), "empty_rows");
    let empty = Correspondences::default();
    let one = cs(1);
    assert_eq!(build_rows(Metric::PointToPoint, &[], &empty, &[], &one).unwrap_err().code(), "length_mismatch");
}

#[test]
fn nan_and_infinities_are_refused_naming_the_field() {
    let set = cs(3);
    let w = set.combined_weights().to_vec();
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let (mut moved, c) = three();
        moved[1][2] = bad;
        let err = build_rows(Metric::PointToPoint, &moved, &c, &w, &set).unwrap_err();
        assert_eq!(err, MetricError::NanInput { field: InputField::Moved, index: 1 });
        assert_eq!(err.code(), "nan_input");

        let (moved, c) = three();
        let err = build_rows(Metric::PointToPoint, &moved, &c, &[1.0, 1.0, bad], &set).unwrap_err();
        assert_eq!(err, MetricError::NanInput { field: InputField::Weight, index: 2 });

        let (moved, mut c) = three();
        c.foot[2][0] = bad;
        let err = build_rows(Metric::PointToPlane, &moved, &c, &w, &set).unwrap_err();
        assert_eq!(err, MetricError::NanInput { field: InputField::Foot, index: 2 });

        for m in Metric::ALL {
            let (moved, mut c) = three();
            c.normal[0][1] = bad;
            let err = build_rows(m, &moved, &c, &w, &set).unwrap_err();
            assert_eq!(err, MetricError::NanInput { field: InputField::Normal, index: 0 });
        }
    }
    // The first offender in index order wins: a NaN weight of point 0 before a NaN point 2.
    let (mut moved, c) = three();
    moved[2][0] = f64::NAN;
    let err = build_rows(Metric::PointToPoint, &moved, &c, &[f64::NAN, 1.0, 1.0], &set).unwrap_err();
    assert_eq!(err, MetricError::NanInput { field: InputField::Weight, index: 0 });
}

#[test]
fn other_refusals() {
    let set = cs(3);
    let w = set.combined_weights().to_vec();

    let (moved, c) = three();
    let err = build_rows(Metric::PointToPoint, &moved, &c, &[1.0, -1.0, 1.0], &set).unwrap_err();
    assert_eq!(err, MetricError::InvalidWeight { index: 1 });
    assert_eq!(err.code(), "invalid_weight");

    let (moved, mut c) = three();
    c.reason[1] = CorrReason::NoHit;
    let err = build_rows(Metric::PointToPoint, &moved, &c, &w, &set).unwrap_err();
    assert_eq!(err, MetricError::InconsistentCorrespondence { index: 1 });
    let (moved, mut c) = three();
    c.valid[2] = false;
    let err = build_rows(Metric::PointToPoint, &moved, &c, &w, &set).unwrap_err();
    assert_eq!(err.code(), "inconsistent_correspondence");

    // A non-unit normal is refused for point-to-plane only.
    let (moved, mut c) = three();
    c.normal[1] = [0.6, -0.8, 1e-5];
    let err = build_rows(Metric::PointToPlane, &moved, &c, &w, &set).unwrap_err();
    assert_eq!(err, MetricError::NonUnitNormal { index: 1 });
    assert_eq!(err.code(), "non_unit_normal");
    assert!(build_rows(Metric::PointToPoint, &moved, &c, &w, &set).is_ok());
    // Just inside the tolerance is accepted.
    let (moved, mut c) = three();
    c.normal[0] = [0.0, 0.0, 1.0 + NORMAL_TOL / 4.0];
    assert!(build_rows(Metric::PointToPlane, &moved, &c, &w, &set).is_ok());

    // Overflow in a row is non_finite.
    let (mut moved, mut c) = three();
    moved[0] = [1e308, 0.0, 0.0];
    c.foot[0] = [-1e308, 0.0, 0.0];
    for m in Metric::ALL {
        let err = build_rows(m, &moved, &c, &w, &set).unwrap_err();
        assert_eq!(err, MetricError::NonFinite { index: 0 }, "{m}");
    }

    // Lengths.
    let (moved, c) = three();
    let err = build_rows(Metric::PointToPoint, &moved[..2], &c, &w[..2], &cs(2)).unwrap_err();
    assert_eq!(err, MetricError::LengthMismatch { what: "foot", expected: 2, got: 3 });
    let err = build_rows(Metric::PointToPoint, &moved, &c, &w[..2], &set).unwrap_err();
    assert_eq!(err, MetricError::LengthMismatch { what: "weights", expected: 3, got: 2 });
    let err = build_rows(Metric::PointToPoint, &moved, &c, &w, &cs(4)).unwrap_err();
    assert_eq!(err, MetricError::LengthMismatch { what: "constraint_points", expected: 3, got: 4 });
    let (moved, mut c) = three();
    c.reason.pop();
    let err = build_rows(Metric::PointToPoint, &moved, &c, &w, &set).unwrap_err();
    assert_eq!(err, MetricError::LengthMismatch { what: "reason", expected: 3, got: 2 });
    assert!(err.to_string().starts_with("length_mismatch"));
}

#[test]
fn rows_are_in_measured_index_order_and_deterministic() {
    let (moved, c) = three();
    let set = cs(3);
    for m in Metric::ALL {
        let a = build_rows(m, &moved, &c, set.combined_weights(), &set).unwrap();
        let b = build_rows(m, &moved, &c, set.combined_weights(), &set).unwrap();
        assert_eq!(a, b);
        let per = m.rows_per_point() as u64;
        let want: Vec<u64> = (0..3 * per).map(|r| r / per).collect();
        assert_eq!(a.point_of_row(), &want[..]);
        assert!(!a.is_empty());
    }
}
