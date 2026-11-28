//! ALN.BFIT.ICP.LOOP acceptance criteria AC-1 to AC-5 plus REQ-002, REQ-008 and the refusals.
//!
//! Expected values come from outside this crate:
//!
//! - AC-1, AC-2, AC-4 and the lock case: `fixtures/aln/bfit/icp/loop_cases.json`, written by the
//!   independent Python reference `make_loop_fixtures.py` next to it: a box mesh, surface
//!   points and, per case, the known pose (an exact rational rotation from an integer
//!   quaternion and a dyadic translation, each entry rounded once) and the measured points
//!   (the exact inverse image of the surface points, rounded once). Never from running this
//!   crate. The `ALN.BFIT.BENCH` corpus is not built yet; these are the leaf's own cases.
//! - AC-3: `fixtures/aln/bfit/icp/plate_outliers.json` (same script): the planted outlier
//!   indices and the SHA-256 of the excluded set in the `KRN.OUT.POL` layout.
//! - AC-5 compares the loop with itself on 1, 3 and 8 correspondence threads, which is what it
//!   states; the 1,000,000-point timing of REQ-007 is not run (`[unverified]`).
//! - The refusals are hand-labelled from the leaf's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::needless_range_loop)]

use core::sync::atomic::AtomicBool;

use fid_align_solve::{
    bestfit, validate, BestfitError, BestfitOutput, BestfitParams, Dof, DofLocks, FitStatus, FrameContext,
    LockFrameRef, LoopWarning, Metric, ProvisionalRule, Reference, Scope, StopCriteria, Threshold, Verdict, WeightSpec,
    BESTFIT_DETERMINISM, BESTFIT_SIDE_EFFECT, MAX_DISTANCE_FLOOR_M,
};
use fid_align_solve::{ConstraintSet, Stopped};
use fid_filter::policy::{MethodStatus, OutlierPolicy, PolicySpec, Side};
use fid_fit::contract::Primitive;
use fid_geo_dist::{MeshTarget, SignTarget};
use fid_geo_frame::{FrameId, FrameTree, Rigid};
use fid_geo_index::PrimitiveBvh;
use fid_geo_types::{FrameRef, Mesh, PointSet};

type V3 = [f64; 3];
type M3 = [[f64; 3]; 3];

/// AC-1, AC-2: translation within 1e-9 m and rotation within 1e-9 rad.
const POSE_TOL: f64 = 1e-9;

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
    fn at(&self, key: &str) -> &J {
        match self {
            J::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v).unwrap_or_else(|| panic!("missing {key}")),
            _ => panic!("not an object"),
        }
    }

    fn arr(&self) -> &[J] {
        match self {
            J::Arr(v) => v,
            _ => panic!("not an array"),
        }
    }

    fn f(&self) -> f64 {
        match self {
            J::Num(s) => s.parse().unwrap(),
            _ => panic!("not a number: {self:?}"),
        }
    }

    fn u(&self) -> u64 {
        match self {
            J::Num(s) => s.parse().unwrap(),
            _ => panic!("not an integer"),
        }
    }

    fn s(&self) -> &str {
        match self {
            J::Str(s) => s,
            _ => panic!("not a string"),
        }
    }

    fn f3(&self) -> V3 {
        let a = self.arr();
        assert_eq!(a.len(), 3);
        [a[0].f(), a[1].f(), a[2].f()]
    }

    fn m3(&self) -> M3 {
        let a = self.arr();
        assert_eq!(a.len(), 3);
        [a[0].f3(), a[1].f3(), a[2].f3()]
    }
}

fn load(rel: &str) -> J {
    let text = std::fs::read(format!("{}/fixtures/{rel}", fid_testkit::WORKSPACE_ROOT)).unwrap();
    let mut p = P { b: &text, i: 0 };
    let v = p.value();
    p.ws();
    assert_eq!(p.i, text.len(), "trailing bytes in {rel}");
    v
}

// ------------------------------------------------------------------ fixtures

fn frame() -> FrameRef {
    FrameRef::from_bytes([7; 16])
}

struct Scene {
    mesh: Mesh,
    bvh: PrimitiveBvh,
}

impl Scene {
    fn of(j: &J) -> Scene {
        let verts: Vec<V3> = j.at("vertices").arr().iter().map(J::f3).collect();
        let tris: Vec<[u64; 3]> = j
            .at("triangles")
            .arr()
            .iter()
            .map(|t| {
                let a = t.arr();
                [a[0].u(), a[1].u(), a[2].u()]
            })
            .collect();
        let mesh = Mesh::from_points(frame(), &verts, &tris).unwrap();
        let payload: Vec<u64> = (0..tris.len() as u64).collect();
        let bvh = PrimitiveBvh::build(&mesh, &payload, 1).unwrap();
        Scene { mesh, bvh }
    }
}

struct Case {
    name: String,
    rotation: M3,
    translation: V3,
    measured: Vec<V3>,
}

fn case_of(c: &J) -> Case {
    Case {
        name: c.at("name").s().to_string(),
        rotation: c.at("rotation").m3(),
        translation: c.at("translation").f3(),
        measured: c.at("measured").arr().iter().map(J::f3).collect(),
    }
}

fn loop_cases() -> (J, Vec<Case>) {
    let j = load("aln/bfit/icp/loop_cases.json");
    let cases = j.at("cases").arr().iter().map(case_of).collect();
    (j, cases)
}

fn constraints(locks: DofLocks, start: &Rigid, weights: &WeightSpec, n: usize) -> ConstraintSet {
    let tree = FrameTree::new();
    let ctx = FrameContext { tree: &tree, parent: FrameId::WORLD };
    validate(locks, LockFrameRef::Parent, &ctx, start, weights, n).unwrap()
}

fn tight_stop(max_iterations: u64) -> StopCriteria {
    StopCriteria { max_iterations, translation_tol: 1e-13, rotation_tol: 1e-13, ..StopCriteria::default() }
}

fn params<'a>(cs: &'a ConstraintSet, metric: Metric, stop: StopCriteria) -> BestfitParams<'a> {
    BestfitParams {
        frame: frame(),
        scope: Scope::GLOBAL,
        metric,
        constraints: cs,
        outlier: None,
        stop,
        max_corr_distance: None,
        threads: 1,
        cond_threshold: Threshold::DEFAULT,
    }
}

fn run(scene: &Scene, pts: &[V3], p: &BestfitParams<'_>) -> Result<BestfitOutput, BestfitError> {
    let st = SignTarget::new(MeshTarget::new(&scene.mesh, &scene.bvh).unwrap()).unwrap();
    let ps = PointSet::from_points(frame(), pts).unwrap();
    bestfit(&ps, Reference::Mesh(&st), p, &AtomicBool::new(false))
}

fn mat_mul_t(a: &M3, b: &M3) -> M3 {
    let mut c = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            c[i][j] = (a[i][0] * b[j][0] + a[i][1] * b[j][1]) + a[i][2] * b[j][2];
        }
    }
    c
}

/// Translation distance and rotation angle between a fitted pose and the true one.
fn pose_error(fit: &Rigid, rot: &M3, t: &V3) -> (f64, f64) {
    let ft = fit.t();
    let dt = ((ft[0] - t[0]).powi(2) + (ft[1] - t[1]).powi(2) + (ft[2] - t[2]).powi(2)).sqrt();
    let d = mat_mul_t(&fit.r(), rot);
    let s = ((d[2][1] - d[1][2]).powi(2) + (d[0][2] - d[2][0]).powi(2) + (d[1][0] - d[0][1]).powi(2)).sqrt() * 0.5;
    let c = (((d[0][0] + d[1][1]) + d[2][2]) - 1.0) * 0.5;
    (dt, s.atan2(c))
}

fn assert_recovered(out: &BestfitOutput, rot: &M3, t: &V3, what: &str) {
    assert_eq!(out.status(), FitStatus::Ok, "{what}: {:?}", out.verdict());
    assert!(matches!(out.verdict(), Verdict::Converged(_)), "{what}");
    let pose = out.final_pose().expect("converged pose").pose();
    let (dt, dr) = pose_error(&pose, rot, t);
    assert!(dt <= POSE_TOL, "{what}: translation error {dt:e} m");
    assert!(dr <= POSE_TOL, "{what}: rotation error {dr:e} rad");
    assert!(out.not_converged_pose().is_none(), "{what}");
}

fn check_bookkeeping(out: &BestfitOutput) {
    let it = out.iterations();
    assert_eq!(it.len() as u64, out.last_iteration() + 1);
    assert_eq!(out.history().len() as u64, out.last_iteration());
    for (k, s) in it.iter().enumerate() {
        assert_eq!(s.iteration, k as u64);
        assert_eq!(s.verdict.is_none(), k == 0);
        assert_eq!(s.d_translation.is_none(), k == 0);
        let c = s.correspondence;
        assert_eq!(c.hits + c.normal_undefined + c.beyond_max_distance + c.no_hit, c.points);
        assert!(s.n_rows >= 6);
        if k > 0 {
            let row = out.history().row(k - 1).unwrap();
            assert_eq!(row.objective.to_bits(), s.objective.to_bits());
            assert_eq!(row.n_used, s.n_used);
            assert_eq!(Some(row.d_translation), s.d_translation);
            assert_eq!(Some(row.d_rotation), s.d_rotation);
        }
    }
    let last = it.last().unwrap();
    assert_eq!(last.verdict, Some(out.verdict()));
    assert!(it[..it.len() - 1].iter().skip(1).all(|s| s.verdict == Some(Verdict::Continue)));
    assert_eq!(out.report().len(), out.iterations()[0].correspondence.points as usize);
    assert!(out.diagnostics().history().is_some());
    assert!(it[0].step_conditioning.is_none());
    for w in out.warnings() {
        let LoopWarning::ConvergedAboveBest { best_iteration, best_objective } = *w;
        assert_eq!(w.code(), "converged_above_best");
        assert_eq!(out.status(), FitStatus::Ok);
        assert_ne!(best_iteration, out.pose_iteration());
        assert!(best_objective < it[out.pose_iteration() as usize].objective);
    }
    assert_eq!(out.algorithm_id(), out.metric().algorithm_id());
}

// ------------------------------------------------------------------ AC-1, AC-2

fn recover_all(metric: Metric) {
    let (j, cases) = loop_cases();
    let scene = Scene::of(&j);
    let mut ran = 0;
    for c in cases.iter().filter(|c| c.name != "locked_tz") {
        let cs = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec::default(), c.measured.len());
        let p = params(&cs, metric, tight_stop(500));
        let out = run(&scene, &c.measured, &p).unwrap_or_else(|e| panic!("{}: {e}", c.name));
        assert_recovered(&out, &c.rotation, &c.translation, &format!("{} {}", metric, c.name));
        check_bookkeeping(&out);
        // REQ-002: global scope and no policy: every point with a correspondence and a
        // non-zero weight contributes to every iteration.
        for s in out.iterations() {
            assert_eq!(s.n_used, s.correspondence.hits, "{}", c.name);
            assert_eq!(s.eligible, s.n_used);
            assert_eq!(s.excluded, 0);
            assert_eq!(s.policy_status, None);
            assert_eq!(s.n_rows, s.n_used * metric.rows_per_point() as u64);
        }
        assert!(out.exclusion().is_none());
        let stats = out.report().stats();
        assert_eq!(stats.n_used(), c.measured.len() as u64);
        assert!(stats.max_abs().unwrap().value() <= POSE_TOL);
        ran += 1;
    }
    assert_eq!(ran, 5);
}

#[test]
fn ac1_point_to_plane_recovers_noise_free_poses() {
    recover_all(Metric::PointToPlane);
}

#[test]
fn ac2_point_to_point_recovers_noise_free_poses() {
    recover_all(Metric::PointToPoint);
}

#[test]
fn locked_translation_stays_locked_and_the_free_pose_is_recovered() {
    let (j, cases) = loop_cases();
    let scene = Scene::of(&j);
    let c = cases.iter().find(|c| c.name == "locked_tz").unwrap();
    assert_eq!(c.translation[2], 0.0);
    for metric in Metric::ALL {
        let cs = constraints(DofLocks::NONE.with(Dof::Tz), &Rigid::IDENTITY, &WeightSpec::default(), c.measured.len());
        let p = params(&cs, metric, tight_stop(500));
        let out = run(&scene, &c.measured, &p).unwrap();
        assert_recovered(&out, &c.rotation, &c.translation, "locked_tz");
        let pose = out.final_pose().unwrap();
        assert!(pose.is_for(&cs));
        assert!(pose.relative()[2].abs() <= 1e-12);
        assert_eq!(out.diagnostics().params(), &[Dof::Tx, Dof::Ty, Dof::Rx, Dof::Ry, Dof::Rz]);
        for s in out.iterations() {
            assert_eq!(s.params[2].to_bits(), 0.0f64.to_bits(), "a locked parameter is never an unknown");
        }
    }
}

// ------------------------------------------------------------------ AC-3

fn gate_spec(max_distance_m: f64) -> PolicySpec {
    PolicySpec {
        policy: OutlierPolicy::NominalGate { max_distance_m },
        max_excluded_fraction: 0.25,
        target: Primitive::Point,
    }
}

#[test]
fn ac3_plate_outliers_are_excluded_and_the_digest_is_recorded() {
    let j = load("aln/bfit/icp/plate_outliers.json");
    let scene = Scene::of(&j);
    let measured: Vec<V3> = j.at("measured").arr().iter().map(J::f3).collect();
    let planted: Vec<u64> = j.at("outliers").arr().iter().map(J::u).collect();
    assert_eq!(measured.len(), 500);
    assert_eq!(planted.len(), 10);
    let gate = j.at("gate_max_distance_m").f();
    assert_eq!(gate, 1e-3);
    let rot = j.at("rotation").m3();
    let t = j.at("translation").f3();
    for metric in Metric::ALL {
        let cs = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec::default(), measured.len());
        let mut p = params(&cs, metric, tight_stop(500));
        p.outlier = Some(gate_spec(gate));
        let out = run(&scene, &measured, &p).unwrap();
        assert_recovered(&out, &rot, &t, "plate");
        check_bookkeeping(&out);
        let ex = out.exclusion().expect("exclusion record");
        assert_eq!(ex.excluded(), planted.as_slice());
        assert_eq!(ex.n_excluded(), 10);
        assert_eq!(ex.eligible(), 500);
        assert_eq!(ex.excluded_digest().to_string(), j.at("excluded_digest").s());
        assert_eq!(ex.algorithm(), "filter.outlier.nominal_gate@1.0.0");
        assert_eq!(ex.params(), gate_spec(gate).encode().unwrap().as_slice());
        assert_eq!(ex.status(), MethodStatus::Decided);
        // Applied every iteration, to that iteration's distances.
        for s in out.iterations() {
            assert_eq!(s.excluded, 10, "iteration {}", s.iteration);
            assert_eq!(s.policy_status, Some(MethodStatus::Decided));
            assert_eq!(s.n_used, 490);
        }
        // The report names the outliers.
        let mut rejected = Vec::new();
        for e in out.report().entries(0, 500) {
            if let Some(r) = e.reason {
                assert_eq!(r, "outlier");
                rejected.push(e.reference.item);
            }
        }
        assert_eq!(rejected, planted);
        assert_eq!(out.report().stats().n_rejected(), 10);
        assert!(out.provisional().contains(&ProvisionalRule::DistanceFloor));
    }
}

#[test]
fn without_a_policy_the_plate_outliers_are_beyond_a_maximum_distance() {
    let j = load("aln/bfit/icp/plate_outliers.json");
    let scene = Scene::of(&j);
    let measured: Vec<V3> = j.at("measured").arr().iter().map(J::f3).collect();
    let planted: Vec<u64> = j.at("outliers").arr().iter().map(J::u).collect();
    let cs = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec::default(), measured.len());
    let mut p = params(&cs, Metric::PointToPlane, tight_stop(500));
    p.max_corr_distance = Some(1e-3);
    let out = run(&scene, &measured, &p).unwrap();
    assert_recovered(&out, &j.at("rotation").m3(), &j.at("translation").f3(), "plate max distance");
    let mut beyond = Vec::new();
    for e in out.report().entries(0, 500) {
        if let Some(r) = e.reason {
            assert_eq!(r, "beyond_max_distance");
            assert_eq!(e.residual.value(), 0.0);
            assert!(e.vector.is_none());
            beyond.push(e.reference.item);
        }
    }
    assert_eq!(beyond, planted);
    for s in out.iterations() {
        assert_eq!(s.correspondence.beyond_max_distance, 10);
    }
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_max_iterations_gives_not_converged_with_the_lowest_objective_pose() {
    let (j, cases) = loop_cases();
    let scene = Scene::of(&j);
    let c = cases.iter().find(|c| c.name == "start5").unwrap();
    for metric in Metric::ALL {
        let cs = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec::default(), c.measured.len());
        let p = params(&cs, metric, StopCriteria { max_iterations: 2, ..StopCriteria::default() });
        let out = run(&scene, &c.measured, &p).unwrap();
        check_bookkeeping(&out);
        assert_eq!(out.status(), FitStatus::NotConverged);
        assert_eq!(out.verdict(), Verdict::Stopped(Stopped::MaxIterations));
        assert_eq!(out.last_iteration(), 2);
        assert!(out.final_pose().is_none(), "a non-converged pose is never final");
        let pose = out.not_converged_pose().unwrap();
        // The lowest objective seen (iteration 0 included; the earlier wins a tie).
        let it = out.iterations();
        let mut best = 0;
        for k in 1..it.len() {
            if it[k].objective < it[best].objective {
                best = k;
            }
        }
        assert_eq!(out.pose_iteration(), best as u64);
        let rel = pose.relative();
        for d in 0..6 {
            assert!((rel[d] - it[best].params[d]).abs() <= 1e-12, "{metric}: component {d}");
        }
        assert_eq!(out.history().stopped_by().to_string(), "limit(max_iterations)");
        assert_eq!(out.history().final_pose(), pose);
        // The same start converges when allowed to.
        let p = params(&cs, metric, tight_stop(500));
        let out = run(&scene, &c.measured, &p).unwrap();
        assert_recovered(&out, &c.rotation, &c.translation, "start5");
    }
}

// ------------------------------------------------------------------ AC-5

fn encode(out: &BestfitOutput) -> Vec<u8> {
    let mut b = Vec::new();
    let pose = match out.final_pose() {
        Some(p) => p.pose(),
        None => out.not_converged_pose().unwrap().pose(),
    };
    for row in pose.r() {
        for x in row {
            b.extend_from_slice(&x.to_bits().to_le_bytes());
        }
    }
    for x in pose.t() {
        b.extend_from_slice(&x.to_bits().to_le_bytes());
    }
    b.extend_from_slice(out.report().digest().as_str().as_bytes());
    b.extend_from_slice(out.history().digest().as_str().as_bytes());
    for s in out.iterations() {
        b.extend_from_slice(format!("{s:?}").as_bytes());
    }
    b.extend_from_slice(format!("{:?}", out.diagnostics()).as_bytes());
    b.extend_from_slice(format!("{:?}", out.exclusion()).as_bytes());
    b
}

#[test]
fn ac5_output_is_bit_identical_for_any_thread_count() {
    assert_eq!(BESTFIT_DETERMINISM.as_str(), "D0");
    assert_eq!(BESTFIT_SIDE_EFFECT.as_str(), "R1");
    let (j, cases) = loop_cases();
    let scene = Scene::of(&j);
    let c = cases.iter().find(|c| c.name == "rot_large").unwrap();
    let plate = load("aln/bfit/icp/plate_outliers.json");
    let plate_scene = Scene::of(&plate);
    let plate_pts: Vec<V3> = plate.at("measured").arr().iter().map(J::f3).collect();
    for metric in Metric::ALL {
        let cs = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec::default(), c.measured.len());
        let pcs = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec::default(), plate_pts.len());
        let mut reference = None;
        let mut plate_reference = None;
        for threads in [1, 3, 8] {
            let mut p = params(&cs, metric, tight_stop(500));
            p.threads = threads;
            let e = encode(&run(&scene, &c.measured, &p).unwrap());
            assert_eq!(reference.get_or_insert_with(|| e.clone()), &e, "{metric} threads {threads}");
            let mut p = params(&pcs, metric, tight_stop(500));
            p.threads = threads;
            p.outlier = Some(gate_spec(1e-3));
            let e = encode(&run(&plate_scene, &plate_pts, &p).unwrap());
            assert_eq!(plate_reference.get_or_insert_with(|| e.clone()), &e, "plate {metric} threads {threads}");
        }
    }
}

// ------------------------------------------------------------------ scope, weights, report

#[test]
fn local_scope_and_zero_weights_are_reported() {
    let (j, cases) = loop_cases();
    let scene = Scene::of(&j);
    let c = cases.iter().find(|c| c.name == "rot_trans").unwrap();
    let n = c.measured.len();
    // Every third point out of scope; point 1 weighted 0.
    let scope: Vec<u64> = (0..n as u64).filter(|i| i % 3 != 0).collect();
    let mut w = vec![1.0; n];
    w[1] = 0.0;
    let cs = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec { per_point: Some(w), per_region: vec![] }, n);
    let mut p = params(&cs, Metric::PointToPlane, tight_stop(500));
    p.scope = Scope { measured: Some(&scope), patches: None };
    let out = run(&scene, &c.measured, &p).unwrap();
    assert_recovered(&out, &c.rotation, &c.translation, "local scope");
    assert!(!out.report().weighted(), "a zero weight makes the report unweighted");
    for e in out.report().entries(0, n) {
        let i = e.reference.item;
        let why = e.reason.map(str::to_string);
        if i % 3 == 0 {
            assert_eq!(why.as_deref(), Some("out_of_scope"), "point {i}");
        } else if i == 1 {
            assert_eq!(why.as_deref(), Some("zero_weight"));
            assert!(e.vector.is_some(), "a zero-weight point keeps its real residual");
        } else {
            assert_eq!(why, None, "point {i}");
        }
    }
    for s in out.iterations() {
        assert_eq!(s.correspondence.points, scope.len() as u64);
        assert_eq!(s.n_used, scope.len() as u64 - 1);
    }
}

#[test]
fn positive_weights_give_a_weighted_report() {
    let (j, cases) = loop_cases();
    let scene = Scene::of(&j);
    let c = cases.iter().find(|c| c.name == "rot_z").unwrap();
    let n = c.measured.len();
    let w: Vec<f64> = (0..n).map(|i| if i % 2 == 0 { 1.0 } else { 2.0 }).collect();
    let cs = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec { per_point: Some(w), per_region: vec![] }, n);
    let out = run(&scene, &c.measured, &params(&cs, Metric::PointToPlane, tight_stop(500))).unwrap();
    assert_recovered(&out, &c.rotation, &c.translation, "weighted");
    assert!(out.report().weighted());
    assert!(out.report().stats().weighted_rms().is_some());
}

// ------------------------------------------------------------------ REQ-008 and refusals

fn small_scene() -> (Scene, Vec<Case>) {
    let (j, cases) = loop_cases();
    (Scene::of(&j), cases)
}

fn code_of(r: Result<BestfitOutput, BestfitError>) -> (String, Option<u64>) {
    let e = r.unwrap_err();
    assert!(!e.to_string().is_empty());
    (e.code().to_string(), e.iteration())
}

#[test]
fn fewer_than_six_rows_is_insufficient_correspondences() {
    let (scene, cases) = small_scene();
    let c = &cases[0];
    let n = c.measured.len();
    let cs = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec::default(), n);
    // Point-to-plane, five points: five rows.
    let scope: Vec<u64> = vec![0, 1, 2, 3, 4];
    let mut p = params(&cs, Metric::PointToPlane, tight_stop(10));
    p.scope = Scope { measured: Some(&scope), patches: None };
    let e = run(&scene, &c.measured, &p).unwrap_err();
    assert_eq!(e, BestfitError::InsufficientCorrespondences { iteration: 0, rows: 5 });
    // Point-to-point, one point: three rows.
    let scope: Vec<u64> = vec![7];
    let mut p = params(&cs, Metric::PointToPoint, tight_stop(10));
    p.scope = Scope { measured: Some(&scope), patches: None };
    let e = run(&scene, &c.measured, &p).unwrap_err();
    assert_eq!(e, BestfitError::InsufficientCorrespondences { iteration: 0, rows: 3 });
    // After masking: a gate that keeps nothing leaves too few rows or trips the guard.
    let mut p = params(&cs, Metric::PointToPlane, tight_stop(10));
    p.outlier = Some(PolicySpec {
        policy: OutlierPolicy::NominalGate { max_distance_m: 1e-6 },
        max_excluded_fraction: 0.999,
        target: Primitive::Point,
    });
    let (code, it) = code_of(run(&scene, &c.measured, &p));
    assert!(code == "insufficient_correspondences" || code == "too_few_remaining", "{code}");
    assert_eq!(it, Some(0));
}

#[test]
fn refusals_before_the_run() {
    let (scene, cases) = small_scene();
    let c = &cases[0];
    let n = c.measured.len();
    let cs = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec::default(), n);
    let base = params(&cs, Metric::PointToPlane, tight_stop(10));

    let other = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec::default(), n - 1);
    assert_eq!(
        code_of(run(&scene, &c.measured, &params(&other, Metric::PointToPlane, tight_stop(10)))).0,
        "point_count_mismatch"
    );

    for bad in [vec![], vec![2, 1], vec![1, 1], vec![n as u64]] {
        let mut p = base;
        p.scope = Scope { measured: Some(&bad), patches: None };
        assert_eq!(code_of(run(&scene, &c.measured, &p)), ("invalid_scope".to_string(), None), "{bad:?}");
    }
    for threads in [0, 257] {
        let mut p = base;
        p.threads = threads;
        assert_eq!(code_of(run(&scene, &c.measured, &p)).0, "invalid_threads");
    }
    for d in [f64::NAN, f64::INFINITY, 0.0, -1.0, MAX_DISTANCE_FLOOR_M / 2.0] {
        let mut p = base;
        p.max_corr_distance = Some(d);
        assert_eq!(code_of(run(&scene, &c.measured, &p)), ("invalid_max_distance".to_string(), None), "{d}");
    }
    let mut p = base;
    p.outlier = Some(PolicySpec {
        policy: OutlierPolicy::Ransac { threshold_m: 1e-3, iterations: 10, min_inliers: 10, seed: 1 },
        max_excluded_fraction: 0.25,
        target: Primitive::Plane,
    });
    assert_eq!(code_of(run(&scene, &c.measured, &p)).0, "outlier_policy_unsupported");
    let mut p = base;
    p.outlier = Some(gate_spec(MAX_DISTANCE_FLOOR_M / 2.0));
    assert_eq!(code_of(run(&scene, &c.measured, &p)), ("invalid_outlier_gate".to_string(), None));
    for (lower_m, upper_m) in [(0.0, 1e-2), (-1e-2, 5e-10), (-1e-2, 1e-2)] {
        let mut p = base;
        p.outlier = Some(PolicySpec {
            policy: OutlierPolicy::ToleranceBand { lower_m, upper_m },
            max_excluded_fraction: 0.25,
            target: Primitive::Point,
        });
        let r = run(&scene, &c.measured, &p);
        if lower_m == -1e-2 && upper_m == 1e-2 {
            assert!(r.is_ok(), "a band holding the floor is accepted: {r:?}");
        } else {
            assert_eq!(code_of(r).0, "invalid_outlier_gate", "{lower_m} {upper_m}");
        }
    }
    let mut p = base;
    p.outlier = Some(PolicySpec { max_excluded_fraction: 1.0, ..gate_spec(1e-3) });
    assert_eq!(code_of(run(&scene, &c.measured, &p)).0, "invalid_parameter");
    let mut p = base;
    p.stop.translation_tol = f64::NAN;
    assert_eq!(code_of(run(&scene, &c.measured, &p)).0, "invalid_stop_criteria");
}

#[test]
fn refusals_during_the_run() {
    let (scene, cases) = small_scene();
    let c = &cases[0];
    let n = c.measured.len();
    let cs = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec::default(), n);
    let base = params(&cs, Metric::PointToPlane, tight_stop(10));

    // The reference is in another frame.
    let mut p = base;
    p.frame = FrameRef::from_bytes([8; 16]);
    let e = run(&scene, &c.measured, &p).unwrap_err();
    assert!(matches!(e, BestfitError::Corr { iteration: 0, .. }), "{e:?}");

    // Cancelled.
    let st = SignTarget::new(MeshTarget::new(&scene.mesh, &scene.bvh).unwrap()).unwrap();
    let ps = PointSet::from_points(frame(), &c.measured).unwrap();
    let e = bestfit(&ps, Reference::Mesh(&st), &base, &AtomicBool::new(true)).unwrap_err();
    assert_eq!(e.code(), "cancelled");

    // The relative resolution floor: points moved 1e6 m away make 1e-8 m unresolvable.
    let far = Rigid::new([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [1e6, 0.0, 0.0]).unwrap();
    let fcs = constraints(DofLocks::NONE, &far, &WeightSpec::default(), n);
    let mut p = params(&fcs, Metric::PointToPlane, tight_stop(10));
    p.max_corr_distance = Some(1e-8);
    let e = run(&scene, &c.measured, &p).unwrap_err();
    match e {
        BestfitError::InvalidMaxDistance { value, floor, iteration } => {
            assert_eq!(value, 1e-8);
            assert!(floor > 1e-8);
            assert_eq!(iteration, Some(0));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_unobservable_step_is_refused() {
    // A flat sheet: point-to-plane cannot see tx, ty or rz.
    let verts: Vec<V3> = vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]];
    let tris: Vec<[u64; 3]> = vec![[0, 1, 2], [0, 2, 3]];
    let mesh = Mesh::from_points(frame(), &verts, &tris).unwrap();
    let bvh = PrimitiveBvh::build(&mesh, &[0, 1], 1).unwrap();
    let scene = Scene { mesh, bvh };
    let pts: Vec<V3> = (0..25).map(|i| [(i % 5) as f64 * 0.25 - 0.5, (i / 5) as f64 * 0.25 - 0.5, 0.001]).collect();
    let cs = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec::default(), pts.len());
    let e = run(&scene, &pts, &params(&cs, Metric::PointToPlane, tight_stop(10))).unwrap_err();
    match e {
        BestfitError::UnobservableStep { iteration, count, .. } => {
            assert_eq!(iteration, 1);
            assert_eq!(count, 3);
        }
        other => panic!("{other:?}"),
    }
    // With those three locked the plane fit is observable and converges.
    let locks = DofLocks::NONE.with(Dof::Tx).with(Dof::Ty).with(Dof::Rz);
    let cs = constraints(locks, &Rigid::IDENTITY, &WeightSpec::default(), pts.len());
    let out = run(&scene, &pts, &params(&cs, Metric::PointToPlane, tight_stop(50))).unwrap();
    assert_eq!(out.status(), FitStatus::Ok);
    let t = out.final_pose().unwrap().pose().t();
    assert!((t[2] + 0.001).abs() <= 1e-12, "{t:?}");
}

#[test]
fn provisional_rules_are_named() {
    let (scene, cases) = small_scene();
    let c = &cases[0];
    let cs = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec::default(), c.measured.len());
    let out = run(&scene, &c.measured, &params(&cs, Metric::PointToPlane, tight_stop(500))).unwrap();
    assert_eq!(out.provisional(), &[ProvisionalRule::UnobservableGuard, ProvisionalRule::DefaultCondThreshold]);
    let mut p = params(&cs, Metric::PointToPlane, tight_stop(500));
    p.cond_threshold = Threshold::caller(1e10).unwrap();
    p.max_corr_distance = Some(0.01);
    let out = run(&scene, &c.measured, &p).unwrap();
    assert_eq!(out.provisional(), &[ProvisionalRule::UnobservableGuard, ProvisionalRule::DistanceFloor]);
    let codes: Vec<&str> = out.provisional().iter().map(|r| r.code()).collect();
    assert_eq!(codes, ["unobservable_guard", "distance_floor"]);
}

#[test]
fn nsigma_policy_on_noise_free_data_reports_its_status() {
    // A statistics policy runs on the signed correspondence distances every iteration.
    let (scene, cases) = small_scene();
    let c = &cases[0];
    let cs = constraints(DofLocks::NONE, &Rigid::IDENTITY, &WeightSpec::default(), c.measured.len());
    let mut p = params(&cs, Metric::PointToPlane, tight_stop(500));
    p.outlier = Some(PolicySpec {
        policy: OutlierPolicy::NSigma { k: 1e6, side: Side::Both },
        max_excluded_fraction: 0.25,
        target: Primitive::Point,
    });
    let out = run(&scene, &c.measured, &p).unwrap();
    assert_recovered(&out, &c.rotation, &c.translation, "nsigma");
    let ex = out.exclusion().unwrap();
    assert_eq!(ex.n_excluded(), 0);
    assert_eq!(ex.algorithm(), "filter.outlier.nsigma@1.0.0");
    for s in out.iterations() {
        assert!(s.policy_status.is_some());
    }
}

#[test]
fn constants() {
    assert_eq!(fid_align_solve::MAX_DISTANCE_REL, f64::from_bits((1023 - 44) << 52));
    assert_eq!(fid_align_solve::MIN_ROWS, 6);
}
