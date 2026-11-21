//! Acceptance criteria AC-1 to AC-6 of spec node ALN.BFIT.CONSTR plus the pinned edge cases.
//! Every expected sine, cosine, pose, moved-point digest, Jacobian row, combined weight and
//! digest comes from `fixtures/aln/bfit/make_constr_fixtures.py`, an independent stdlib Python
//! reference (exact Decimal trigonometry, no libm) of the rules pinned in
//! `fid_align_solve::constr`; nothing expected is produced by this crate. CI runs this file on
//! Linux, Windows, macOS and wasm32-wasip1, so equality with the reference on each is
//! bit-identity across platforms. The output pin at the end also covers the Gauss-Newton test
//! loop's recovered parameters, which have no exact reference.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align_solve::constr::{MAX_JACOBIAN_ROWS, MAX_REGIONS};
use fid_align_solve::{
    linearise, pose_from_params, reduced_jacobian, validate, verify_locks, ConstrError, ConstraintSet, Dof, DofLocks,
    FrameContext, LockFrameRef, RegionWeight, WeightInput, WeightSpec, B_MAX, LOCK_TOL,
};
use fid_geo_frame::{Frame, FrameId, FrameKind, FrameTree, Rigid, Similarity, Transform};
use fid_kci::kci::Sha256;
use fid_types::Digest;

// The workspace root comes from fid-testkit: kernel-closure sources do not use the
// manifest-directory macro (KRN.MAN.MANI.IMPL).
fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{}/fixtures/aln/bfit/{name}", fid_testkit::WORKSPACE_ROOT)).unwrap()
}

fn hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s.strip_prefix("0x").unwrap(), 16).unwrap())
}

fn hexs(it: &[&str]) -> Vec<f64> {
    it.iter().map(|s| hex(s)).collect()
}

fn same_bits(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

fn rigid(v: &[f64]) -> Rigid {
    Rigid::new([[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]], [v[9], v[10], v[11]]).unwrap()
}

fn flat(r: &Rigid) -> Vec<f64> {
    let m = r.r();
    m.iter().flatten().copied().chain(r.t()).collect()
}

fn digest_of(xs: impl IntoIterator<Item = f64>) -> Digest {
    let mut h = Sha256::new();
    for x in xs {
        h.update(&x.to_le_bytes());
    }
    Digest::from_bytes(h.finish())
}

// ------------------------------------------------------------------ the sweep fixture

struct Case {
    mask: u8,
    ok: Option<CaseOk>,
}

struct CaseOk {
    params: Vec<f64>,
    sincos: Vec<f64>,
    pose: Vec<f64>,
    targets: String,
    jac: Vec<(usize, Vec<f64>)>,
    locks: String,
}

struct Sweep {
    b_max: f64,
    lock_id: FrameId,
    lock: Rigid,
    start: Rigid,
    points: Vec<[f64; 3]>,
    cases: Vec<Case>,
    defaults: Vec<(u8, String)>,
}

fn sweep() -> Sweep {
    let text = read("locks-sweep/sweep.txt");
    let mut s = Sweep {
        b_max: 0.0,
        lock_id: FrameId::WORLD,
        lock: Rigid::IDENTITY,
        start: Rigid::IDENTITY,
        points: Vec::new(),
        cases: Vec::new(),
        defaults: Vec::new(),
    };
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split(' ').collect();
        match f[0] {
            "seed" | "n_points" => {}
            "b_max" => s.b_max = hex(f[1]),
            "lock_id" => s.lock_id = FrameId::parse(f[1]).unwrap(),
            "lock" => s.lock = rigid(&hexs(&f[1..])),
            "start" => s.start = rigid(&hexs(&f[1..])),
            "p" => s.points.push([hex(f[1]), hex(f[2]), hex(f[3])]),
            "locks_default" => s.defaults.push((f[1].parse().unwrap(), f[2].to_string())),
            "case" => {
                let mask: u8 = f[1].parse().unwrap();
                if f[2] == "err" {
                    assert_eq!(f[3], "all_dof_locked");
                    s.cases.push(Case { mask, ok: None });
                    continue;
                }
                let at = |tag: &str| f.iter().position(|x| *x == tag).unwrap() + 1;
                let mut jac = Vec::new();
                for (i, x) in f.iter().enumerate() {
                    if *x == "jac" {
                        jac.push((f[i + 1].parse().unwrap(), hexs(&f[i + 2..i + 20])));
                    }
                }
                s.cases.push(Case {
                    mask,
                    ok: Some(CaseOk {
                        params: hexs(&f[at("params")..at("params") + 6]),
                        sincos: hexs(&f[at("sincos")..at("sincos") + 6]),
                        pose: hexs(&f[at("pose")..at("pose") + 12]),
                        targets: f[at("targets")].to_string(),
                        jac,
                        locks: f[at("locks")].to_string(),
                    }),
                });
            }
            other => panic!("unknown line {other}"),
        }
    }
    assert_eq!(s.points.len(), 2000);
    assert_eq!(s.cases.len(), 64);
    s
}

fn tree(lock_id: FrameId, lock: Rigid) -> FrameTree {
    let mut t = FrameTree::new();
    t.insert(Frame {
        id: lock_id,
        name: "lock".to_string(),
        kind: FrameKind::User,
        parent: Some(FrameId::WORLD),
        to_parent: Transform::Rigid(lock),
    })
    .unwrap();
    t
}

fn set(s: &Sweep, t: &FrameTree, mask: u8) -> Result<ConstraintSet, ConstrError> {
    let ctx = FrameContext { tree: t, parent: FrameId::WORLD };
    validate(DofLocks::from_bits(mask)?, LockFrameRef::Name("lock"), &ctx, &s.start, &WeightSpec::default(), 2000)
}

fn free_of(locks: DofLocks, params: &[f64]) -> Vec<f64> {
    locks.free().map(|d| params[d.index()]).collect()
}

/// Solves the small symmetric system `a x = b` by Gaussian elimination with partial pivoting.
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Vec<f64> {
    let n = b.len();
    for k in 0..n {
        let p = (k..n).fold(k, |p, i| if a[i][k].abs() > a[p][k].abs() { i } else { p });
        a.swap(k, p);
        b.swap(k, p);
        for i in k + 1..n {
            let m = a[i][k] / a[k][k];
            for j in k..n {
                a[i][j] -= m * a[k][j];
            }
            b[i] -= m * b[k];
        }
    }
    let mut x = vec![0.0; n];
    for k in (0..n).rev() {
        let mut s = b[k];
        for j in k + 1..n {
            s -= a[k][j] * x[j];
        }
        x[k] = s / a[k][k];
    }
    x
}

/// The AC-1 test loop: Gauss-Newton on point-to-point residuals from all-zero free parameters.
fn gauss_newton(cs: &ConstraintSet, pts: &[[f64; 3]], targets: &[[f64; 3]]) -> Vec<f64> {
    let n = cs.n_free();
    let mut p = vec![0.0; n];
    for _ in 0..12 {
        let lin = linearise(cs, &p).unwrap();
        let pose = Transform::Rigid(lin.pose());
        let mut rows = Vec::with_capacity(3 * pts.len());
        let mut r = Vec::with_capacity(3 * pts.len());
        for (x, y) in pts.iter().zip(targets) {
            let m = pose.apply_point(x).unwrap();
            rows.extend(lin.jacobian_rows(x).unwrap());
            r.extend((0..3).map(|i| m[i] - y[i]));
        }
        let j = reduced_jacobian(cs, &rows).unwrap();
        assert_eq!((j.rows(), j.cols()), (rows.len(), n));
        let mut a = vec![vec![0.0; n]; n];
        let mut g = vec![0.0; n];
        for (k, rk) in r.iter().enumerate() {
            let row = j.row(k).unwrap();
            for u in 0..n {
                g[u] -= row[u] * rk;
                for v in 0..n {
                    a[u][v] += row[u] * row[v];
                }
            }
        }
        let dx = solve(a, g);
        for (pi, di) in p.iter_mut().zip(&dx) {
            *pi += di;
        }
    }
    p
}

// ------------------------------------------------------------------ AC-1, AC-3, REQ-001..003, REQ-008

#[test]
fn ac1_locks_sweep_against_reference() {
    let s = sweep();
    assert_eq!(B_MAX.to_bits(), s.b_max.to_bits(), "B_MAX is the binary64 nearest 4 pi / 9");
    let t = tree(s.lock_id, s.lock);
    for c in &s.cases {
        let Some(ok) = &c.ok else {
            // AC-3: all six locked.
            assert_eq!(c.mask, 63);
            assert_eq!(set(&s, &t, c.mask).unwrap_err(), ConstrError::AllDofLocked);
            continue;
        };
        let cs = set(&s, &t, c.mask).unwrap();
        let locks = cs.locks();
        assert_eq!(cs.locks_digest().as_str(), ok.locks, "locks digest, mask {}", c.mask);
        let free = free_of(locks, &ok.params);
        let lin = linearise(&cs, &free).unwrap();
        // REQ-001: locked parameters are exactly +0.0.
        assert!(same_bits(&lin.params(), &ok.params), "params, mask {}", c.mask);
        let sc: Vec<f64> = (0..3).flat_map(|k| [lin.sin()[k], lin.cos()[k]]).collect();
        assert!(same_bits(&sc, &ok.sincos), "fid-num sin/cos vs exact reference, mask {}", c.mask);
        assert!(same_bits(&flat(&lin.pose()), &ok.pose), "pose bits, mask {}", c.mask);
        let pose = Transform::Rigid(lin.pose());
        let moved: Vec<[f64; 3]> = s.points.iter().map(|x| pose.apply_point(x).unwrap()).collect();
        assert_eq!(digest_of(moved.iter().flatten().copied()).as_str(), ok.targets, "targets, mask {}", c.mask);
        for (idx, want) in &ok.jac {
            let rows = lin.jacobian_rows(&s.points[*idx]).unwrap();
            assert!(same_bits(&rows.concat(), want), "jacobian at point {idx}, mask {}", c.mask);
        }
        // AC-1: recover the pose with the lock pattern applied.
        let got = gauss_newton(&cs, &s.points, &moved);
        for (d, (g, want)) in locks.free().zip(got.iter().zip(&free)) {
            assert!((g - want).abs() <= 1e-9, "mask {} {}: {g} vs {want}", c.mask, d.name());
        }
        let fin = linearise(&cs, &got).unwrap();
        let rel = verify_locks(&cs, &fin.pose()).unwrap().relative();
        for d in Dof::ALL {
            if locks.is_locked(d) {
                assert_eq!(fin.params()[d.index()].to_bits(), 0, "locked parameter is +0.0");
                assert!(rel[d.index()].abs() <= LOCK_TOL, "mask {} {} drift {}", c.mask, d.name(), rel[d.index()]);
            }
        }
    }
}

#[test]
fn default_lock_frame_digests() {
    let s = sweep();
    let t = FrameTree::new();
    let ctx = FrameContext { tree: &t, parent: FrameId::WORLD };
    for (mask, want) in &s.defaults {
        let cs = validate(
            DofLocks::from_bits(*mask).unwrap(),
            LockFrameRef::Parent,
            &ctx,
            &s.start,
            &WeightSpec::default(),
            1,
        )
        .unwrap();
        assert_eq!(cs.locks_digest().as_str(), want);
        assert_eq!(cs.lock_frame_id(), FrameId::WORLD);
        // The parent named explicitly resolves to the exact identity: the same digest.
        let by_id = validate(
            DofLocks::from_bits(*mask).unwrap(),
            LockFrameRef::Id(FrameId::WORLD),
            &ctx,
            &s.start,
            &WeightSpec::default(),
            1,
        )
        .unwrap();
        assert_eq!(by_id.locks_digest(), cs.locks_digest());
    }
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_reduced_jacobian_column_order() {
    let s = sweep();
    let t = tree(s.lock_id, s.lock);
    let locks = DofLocks::NONE.with(Dof::Tz).with(Dof::Rx).with(Dof::Ry);
    let cs = set(&s, &t, locks.bits()).unwrap();
    assert_eq!(cs.n_free(), 3);
    assert_eq!(locks.free().collect::<Vec<_>>(), vec![Dof::Tx, Dof::Ty, Dof::Rz]);
    let lin = linearise(&cs, &[0.001, -0.002, 0.03]).unwrap();
    let rows: Vec<[f64; 6]> = s.points[..4].iter().flat_map(|x| lin.jacobian_rows(x).unwrap()).collect();
    let j = reduced_jacobian(&cs, &rows).unwrap();
    assert_eq!((j.rows(), j.cols()), (12, 3));
    for (i, r) in rows.iter().enumerate() {
        let want = [r[0], r[1], r[5]];
        assert!(same_bits(j.row(i).unwrap(), &want));
    }
    assert_eq!(j.get(0, 3), None);
    assert_eq!(j.row(12), None);
    let mut bad = rows.clone();
    bad[5][2] = f64::NAN;
    assert_eq!(reduced_jacobian(&cs, &bad).unwrap_err(), ConstrError::NonFiniteRow { row: 5 });
    const { assert!(MAX_JACOBIAN_ROWS >= 3 * 2000) };
}

// ------------------------------------------------------------------ REQ-004, REQ-005 and refusals

#[test]
fn frame_refusals() {
    let s = sweep();
    let t = tree(s.lock_id, s.lock);
    let w = WeightSpec::default();
    let ctx = FrameContext { tree: &t, parent: FrameId::WORLD };
    let l = DofLocks::NONE;
    let unknown = FrameId::from_bytes([9; 16]);
    let e = validate(l, LockFrameRef::Id(unknown), &ctx, &s.start, &w, 1).unwrap_err();
    assert_eq!((e.code(), e.clone()), ("frame_not_found", ConstrError::FrameNotFound { role: "lock" }));
    let e = validate(l, LockFrameRef::Name("nope"), &ctx, &s.start, &w, 1).unwrap_err();
    assert_eq!(e, ConstrError::FrameNotFound { role: "lock" });
    // Names match ignoring ASCII case, as the tree's names do.
    assert!(validate(l, LockFrameRef::Name("LOCK"), &ctx, &s.start, &w, 1).is_ok());
    let bad_parent = FrameContext { tree: &t, parent: unknown };
    let e = validate(l, LockFrameRef::Parent, &bad_parent, &s.start, &w, 1).unwrap_err();
    assert_eq!(e, ConstrError::FrameNotFound { role: "parent" });
    // all_dof_locked is checked first.
    let e = validate(DofLocks::ALL, LockFrameRef::Id(unknown), &bad_parent, &s.start, &w, 0).unwrap_err();
    assert_eq!(e.code(), "all_dof_locked");
    // A lock frame with a scale is refused, not silently treated as rigid.
    let mut t2 = FrameTree::new();
    let sim = Similarity::new(s.lock, 1.5).unwrap();
    t2.insert(Frame {
        id: s.lock_id,
        name: "scaled".to_string(),
        kind: FrameKind::User,
        parent: Some(FrameId::WORLD),
        to_parent: Transform::Similarity(sim),
    })
    .unwrap();
    let ctx2 = FrameContext { tree: &t2, parent: FrameId::WORLD };
    let e = validate(l, LockFrameRef::Name("scaled"), &ctx2, &s.start, &w, 1).unwrap_err();
    assert_eq!(e, ConstrError::LockFrameNotRigid);
    assert_eq!(DofLocks::from_bits(0x40).unwrap_err(), ConstrError::InvalidLocks { bits: 0x40 });
}

#[test]
fn gimbal_guard_and_parameter_checks() {
    let s = sweep();
    let t = tree(s.lock_id, s.lock);
    let cs = set(&s, &t, 0).unwrap();
    let at_limit = [0.0, 0.0, 0.0, 0.1, B_MAX, -0.2];
    assert!(pose_from_params(&cs, &at_limit).is_ok());
    let above = f64::from_bits(B_MAX.to_bits() + 1);
    for b in [above, -above, 2.0] {
        let e = pose_from_params(&cs, &[0.0, 0.0, 0.0, 0.1, b, -0.2]).unwrap_err();
        assert_eq!((e.code(), e), ("lock_parameterisation_singular", ConstrError::LockParameterisationSingular { b }));
    }
    // A locked b is never refused: it is the constant 0.
    let lb = set(&s, &t, DofLocks::NONE.with(Dof::Ry).bits()).unwrap();
    assert!(pose_from_params(&lb, &[0.0, 0.0, 0.0, 1.5, 1.5]).is_ok());
    assert_eq!(pose_from_params(&cs, &[0.0; 5]).unwrap_err(), ConstrError::ParameterCount { expected: 6, got: 5 });
    assert_eq!(
        pose_from_params(&cs, &[0.0, f64::INFINITY, f64::NAN, 0.0, 0.0, 0.0]).unwrap_err(),
        ConstrError::NonFiniteParameter { index: 1 }
    );
    let lin = linearise(&cs, &[0.0; 6]).unwrap();
    assert_eq!(lin.jacobian_rows(&[f64::NAN, 0.0, 0.0]).unwrap_err().code(), "non_finite");
    assert_eq!(pose_from_params(&cs, &[f64::MAX, f64::MAX, f64::MAX, 0.0, 0.0, 0.0]).unwrap_err().code(), "non_finite");
}

/// (locks, free parameters, rotation, shift, the dof that must drift or `None`).
type DriftCase = (DofLocks, Vec<f64>, [[f64; 3]; 3], [f64; 3], Option<Dof>);

/// `base` followed by the motion `x -> rot x + shift` expressed in the lock frame.
fn nudge(cs: &ConstraintSet, base: &Rigid, rot: [[f64; 3]; 3], shift: [f64; 3]) -> Rigid {
    let m = Transform::Rigid(Rigid::new(rot, shift).unwrap());
    let lt = Transform::Rigid(cs.lock_frame());
    let conj = lt.compose(&m).unwrap().compose(&lt.inverse().unwrap()).unwrap();
    match conj.compose(&Transform::Rigid(*base)).unwrap() {
        Transform::Rigid(r) => r,
        _ => unreachable!(),
    }
}

const E: f64 = 1e-9;
const I3: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
const ROT_X: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, -E], [0.0, E, 1.0]];
const ROT_Y: [[f64; 3]; 3] = [[1.0, 0.0, E], [0.0, 1.0, 0.0], [-E, 0.0, 1.0]];
const ROT_Z: [[f64; 3]; 3] = [[1.0, -E, 0.0], [E, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// REQ-002: a pose that moves a locked component is refused, never certified: locked
/// translations with free rotations, and each locked rotation a, b and c, each nudged by
/// 1e-9 in the lock frame. A nudge of a free component stays certified. (A rotation nudge
/// also turns the translation, so the case that rotates with tx locked has ty = 0.)
#[test]
fn lock_drift_is_refused() {
    let s = sweep();
    let t = tree(s.lock_id, s.lock);
    let x = [E, 0.0, 0.0];
    let y = [0.0, E, 0.0];
    let z = [0.0; 3];
    // (locks, free parameters, rotation, shift, the dof that must drift or None)
    let cases: Vec<DriftCase> = vec![
        (DofLocks::NONE.with(Dof::Tx).with(Dof::Rz), vec![0.001, 0.002, 0.01, -0.02], I3, x, Some(Dof::Tx)),
        (DofLocks::NONE.with(Dof::Tx).with(Dof::Rz), vec![0.0, 0.002, 0.01, -0.02], ROT_Z, z, Some(Dof::Rz)),
        (DofLocks::NONE.with(Dof::Tx).with(Dof::Rz), vec![0.001, 0.002, 0.01, -0.02], I3, y, None),
        (DofLocks::NONE.with(Dof::Rx), vec![0.001, 0.002, 0.003, 0.02, -0.03], ROT_X, z, Some(Dof::Rx)),
        (DofLocks::NONE.with(Dof::Rx), vec![0.001, 0.002, 0.003, 0.02, -0.03], ROT_Z, z, None),
        (DofLocks::NONE.with(Dof::Ry), vec![0.001, 0.002, 0.003, 0.02, -0.03], ROT_Y, z, Some(Dof::Ry)),
        // Translation locked, all three rotations free.
        (DofLocks::NONE.with(Dof::Ty), vec![0.001, 0.003, 0.02, -0.03, 0.04], I3, y, Some(Dof::Ty)),
        (DofLocks::NONE.with(Dof::Ty), vec![0.001, 0.003, 0.02, -0.03, 0.04], I3, x, None),
    ];
    for (i, (locks, free, rot, shift, want)) in cases.into_iter().enumerate() {
        let cs = set(&s, &t, locks.bits()).unwrap();
        let base = pose_from_params(&cs, &free).unwrap();
        assert!(base.is_for(&cs));
        assert_eq!(verify_locks(&cs, &base.pose()).unwrap(), base, "case {i}");
        let moved = nudge(&cs, &base.pose(), rot, shift);
        match (verify_locks(&cs, &moved), want) {
            (Err(ConstrError::LockDrift { dof, value }), Some(d)) => {
                assert_eq!(dof, d, "case {i}");
                assert!(value.abs() > LOCK_TOL && (value.abs() - E).abs() < 1e-11, "case {i}: {value}");
            }
            (Ok(c), None) => assert!(c.is_for(&cs), "case {i}"),
            (got, want) => panic!("case {i}: {got:?}, expected drift of {want:?}"),
        }
    }
    // A certificate names its set: another lock pattern's set does not accept it.
    let a = set(&s, &t, DofLocks::NONE.with(Dof::Tx).bits()).unwrap();
    let b = set(&s, &t, DofLocks::NONE.with(Dof::Ty).bits()).unwrap();
    let c = pose_from_params(&a, &[0.0; 5]).unwrap();
    assert!(c.is_for(&a) && !c.is_for(&b));
}

// ------------------------------------------------------------------ AC-4, AC-5, AC-6

struct WCase {
    name: String,
    n: usize,
    spec: WeightSpec,
    expect: Vec<String>,
}

fn weight_cases() -> (Vec<WCase>, Vec<String>) {
    let text = read("constr-weights/cases.txt");
    let mut out = Vec::new();
    let mut seeded = Vec::new();
    let mut cur: Option<WCase> = None;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split(' ').collect();
        match f[0] {
            "case" => {
                cur = Some(WCase { name: f[1].to_string(), n: 0, spec: WeightSpec::default(), expect: Vec::new() })
            }
            "n" => cur.as_mut().unwrap().n = f[1].parse().unwrap(),
            "per_point" => {
                cur.as_mut().unwrap().spec.per_point = if f[1] == "none" { None } else { Some(hexs(&f[1..])) }
            }
            "region" => {
                cur.as_mut().unwrap().spec.per_region.push(RegionWeight {
                    weight: hex(f[1]),
                    members: f[2..].iter().map(|m| m.parse().unwrap()).collect(),
                })
            }
            "expect" => cur.as_mut().unwrap().expect = f[1..].iter().map(|x| x.to_string()).collect(),
            "end" => out.push(cur.take().unwrap()),
            "seeded" => seeded = f[1..].iter().map(|x| x.to_string()).collect(),
            other => panic!("unknown line {other}"),
        }
    }
    (out, seeded)
}

fn err_detail(e: &ConstrError) -> String {
    match e {
        ConstrError::InvalidWeight(WeightInput::PerPointLength { .. }) => "per_point_length".to_string(),
        ConstrError::InvalidWeight(w) => w.to_string(),
        ConstrError::InvalidRegion { region } => format!("region {region}"),
        e => panic!("unexpected {e:?}"),
    }
}

fn weights_of(spec: &WeightSpec, n: usize) -> Result<ConstraintSet, ConstrError> {
    let t = FrameTree::new();
    let ctx = FrameContext { tree: &t, parent: FrameId::WORLD };
    validate(DofLocks::NONE, LockFrameRef::Parent, &ctx, &Rigid::IDENTITY, spec, n)
}

#[test]
fn ac4_ac5_weight_cases() {
    let (cases, _) = weight_cases();
    assert!(cases.len() >= 19);
    for c in &cases {
        match (weights_of(&c.spec, c.n), c.expect[0].as_str()) {
            (Ok(cs), "ok") => {
                assert_eq!(cs.weights_digest().as_str(), c.expect[1], "{}", c.name);
                let want = hexs(&c.expect[2..].iter().map(|s| s.as_str()).collect::<Vec<_>>());
                assert!(same_bits(cs.combined_weights(), &want), "{}", c.name);
                assert_eq!(cs.n_used(), want.iter().filter(|w| **w > 0.0).count());
            }
            (Err(e), "err") => {
                assert_eq!(e.code(), c.expect[1], "{}", c.name);
                assert_eq!(err_detail(&e), c.expect[2..].join(" "), "{}", c.name);
            }
            (got, _) => panic!("{}: got {got:?}, expected {:?}", c.name, c.expect),
        }
    }
    // AC-4 by its rule, beside the reference: both regions (2 and 0.5) give w exactly; a zero
    // region excludes its point.
    let c = cases.iter().find(|c| c.name == "ac4_overlap").unwrap();
    let cs = weights_of(&c.spec, c.n).unwrap();
    let pp = c.spec.per_point.as_ref().unwrap();
    let w = cs.combined_weights();
    assert_eq!(w[4].to_bits(), pp[4].to_bits());
    assert_eq!(w[5].to_bits(), pp[5].to_bits());
    assert_eq!(w[8].to_bits(), 0);
    // AC-5.
    let c = cases.iter().find(|c| c.name == "negative_weight").unwrap();
    assert_eq!(weights_of(&c.spec, c.n).unwrap_err(), ConstrError::InvalidWeight(WeightInput::PerPoint { index: 3 }));
}

#[test]
fn weight_limits() {
    let regions = vec![RegionWeight { members: Vec::new(), weight: 1.0 }; MAX_REGIONS + 1];
    let spec = WeightSpec { per_point: None, per_region: regions };
    assert_eq!(weights_of(&spec, 2).unwrap_err(), ConstrError::InputLimit("regions"));
    let spec = WeightSpec::default();
    assert_eq!(weights_of(&spec, (1 << 26) + 1).unwrap_err(), ConstrError::InputLimit("points"));
}

struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// The AC-6 input: the generation rule of `seeded_case` in the reference script.
fn seeded_spec(seed: u64, n: usize, n_regions: usize) -> WeightSpec {
    let mut rng = SplitMix64(seed);
    let two_m52 = f64::from_bits(0x3CB0_0000_0000_0000);
    let pp: Vec<f64> = (0..n).map(|_| (rng.next() >> 11) as f64 * two_m52).collect();
    let mut regions = Vec::new();
    for r in 0..n_regions {
        let weight = if r == n_regions - 1 { 0.0 } else { (rng.next() >> 11) as f64 * two_m52 };
        let members = (0..n as u32).filter(|_| rng.next() % 4 == 0).collect();
        regions.push(RegionWeight { members, weight });
    }
    WeightSpec { per_point: Some(pp), per_region: regions }
}

#[test]
fn ac6_weight_digest_is_portable() {
    let (_, s) = weight_cases();
    let (seed, n, k, used) =
        (s[0].parse().unwrap(), s[1].parse().unwrap(), s[2].parse().unwrap(), s[3].parse::<usize>().unwrap());
    let spec = seeded_spec(seed, n, k);
    let cs = weights_of(&spec, n).unwrap();
    assert_eq!(cs.weights_digest().as_str(), s[4]);
    assert_eq!(cs.n_used(), used);
    // The digest is SHA-256 of the little-endian bytes in index order, nothing else.
    assert_eq!(digest_of(cs.combined_weights().iter().copied()), cs.weights_digest());
}

// ------------------------------------------------------------------ output pin

/// Output-bit pin: regression pin of this crate's own output over the whole sweep (locks
/// digests, the pose and Jacobian rows at the true parameters, the Gauss-Newton loop's
/// recovered parameters and the lock certificate) and the seeded weights, recorded from a
/// local x86-64 run and checked by CI on every target. One value serves all targets because
/// they agree bit for bit; a target that diverged would get its own `#[cfg(target_arch)]`
/// value here, and the bit-exact reference checks above say which side moved.
#[cfg(not(target_arch = "wasm32"))]
const OUTPUT_PIN: &str = "sha256:4adcd37352e7b5f333c3a0356efc2c3d89eefba065c9ce5132258cbfd4bb9eb7";
#[cfg(target_arch = "wasm32")]
const OUTPUT_PIN: &str = "sha256:4adcd37352e7b5f333c3a0356efc2c3d89eefba065c9ce5132258cbfd4bb9eb7";

#[test]
fn output_bits_pinned() {
    let s = sweep();
    let t = tree(s.lock_id, s.lock);
    let mut h = Sha256::new();
    for c in &s.cases {
        let Some(ok) = &c.ok else { continue };
        let cs = set(&s, &t, c.mask).unwrap();
        h.update(cs.locks_digest().as_str().as_bytes());
        let lin = linearise(&cs, &free_of(cs.locks(), &ok.params)).unwrap();
        let pose = Transform::Rigid(lin.pose());
        let moved: Vec<[f64; 3]> = s.points.iter().map(|x| pose.apply_point(x).unwrap()).collect();
        for x in s.points.iter().step_by(97) {
            for row in lin.jacobian_rows(x).unwrap() {
                for v in row {
                    h.update(&v.to_le_bytes());
                }
            }
        }
        let got = gauss_newton(&cs, &s.points, &moved);
        let fin = linearise(&cs, &got).unwrap();
        for v in got.iter().chain(&flat(&fin.pose())).chain(&fin.relative()) {
            h.update(&v.to_le_bytes());
        }
    }
    let spec = seeded_spec(11, 100_000, 16);
    h.update(weights_of(&spec, 100_000).unwrap().weights_digest().as_str().as_bytes());
    let pin = Digest::from_bytes(h.finish());
    assert_eq!(pin.as_str(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
