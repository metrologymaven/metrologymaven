//! ALN.DATUM.DOF acceptance criteria AC-1 to AC-6. Expected counts, free-DoF names, warnings,
//! directives and projectors come from the independent reference
//! `fixtures/aln/datum/make_dof_cases.py` (exact rational arithmetic for the counts, free
//! subspaces and projectors), never from running this crate. AC-6 (bit-identity across
//! platforms) is the output pin at the end, checked on every CI target.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align_solve::{
    accumulate, constrained_basis, DofAccount, DofError, DofWarning, FeatureType, FreeDirective, Motion,
    NominalFeature, RadiusBasis, RuleBasis, Through, Twist, UnconstrainedPolicy, DOF_RANK_TOL, MAX_DOF_PAIRS,
};
use fid_kci::kci::{to_hex, Sha256};

// ------------------------------------------------------------------ a minimal JSON reader

#[derive(Debug, Clone, PartialEq)]
enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    fn get(&self, k: &str) -> &J {
        match self {
            J::Obj(m) => &m.iter().find(|(key, _)| key == k).unwrap_or_else(|| panic!("missing key {k}")).1,
            _ => panic!("not an object looking up {k}"),
        }
    }
    fn opt(&self, k: &str) -> Option<&J> {
        match self {
            J::Obj(m) => m.iter().find(|(key, _)| key == k).map(|(_, v)| v),
            _ => None,
        }
    }
    fn arr(&self) -> &[J] {
        match self {
            J::Arr(a) => a,
            _ => panic!("not an array"),
        }
    }
    fn num(&self) -> f64 {
        match self {
            J::Num(x) => *x,
            _ => panic!("not a number: {self:?}"),
        }
    }
    fn usize(&self) -> usize {
        let x = self.num();
        assert!(x >= 0.0 && x.fract() == 0.0);
        x as usize
    }
    fn str(&self) -> &str {
        match self {
            J::Str(s) => s,
            _ => panic!("not a string: {self:?}"),
        }
    }
    fn vec3(&self) -> [f64; 3] {
        let a = self.arr();
        assert_eq!(a.len(), 3);
        [a[0].num(), a[1].num(), a[2].num()]
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
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
                let mut m = Vec::new();
                self.ws();
                if self.b[self.i] == b'}' {
                    self.i += 1;
                    return J::Obj(m);
                }
                loop {
                    self.ws();
                    let k = match self.value() {
                        J::Str(s) => s,
                        v => panic!("bad key {v:?}"),
                    };
                    assert!(m.iter().all(|(key, _)| key != &k), "duplicate key {k}");
                    self.eat(b':');
                    let v = self.value();
                    m.push((k, v));
                    self.ws();
                    if self.b[self.i] == b',' {
                        self.i += 1;
                    } else {
                        self.eat(b'}');
                        return J::Obj(m);
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.b[self.i] == b']' {
                    self.i += 1;
                    return J::Arr(a);
                }
                loop {
                    a.push(self.value());
                    self.ws();
                    if self.b[self.i] == b',' {
                        self.i += 1;
                    } else {
                        self.eat(b']');
                        return J::Arr(a);
                    }
                }
            }
            b'"' => {
                self.i += 1;
                let start = self.i;
                while self.b[self.i] != b'"' {
                    assert_ne!(self.b[self.i], b'\\', "escapes are not used by the fixture");
                    self.i += 1;
                }
                let s = std::str::from_utf8(&self.b[start..self.i]).unwrap().to_string();
                self.i += 1;
                J::Str(s)
            }
            b'n' => {
                assert_eq!(&self.b[self.i..self.i + 4], b"null");
                self.i += 4;
                J::Null
            }
            b't' => {
                assert_eq!(&self.b[self.i..self.i + 4], b"true");
                self.i += 4;
                J::Bool(true)
            }
            b'f' => {
                assert_eq!(&self.b[self.i..self.i + 5], b"false");
                self.i += 5;
                J::Bool(false)
            }
            _ => {
                let start = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    self.i += 1;
                }
                J::Num(std::str::from_utf8(&self.b[start..self.i]).unwrap().parse().unwrap())
            }
        }
    }
}

fn fixture() -> J {
    let file = format!("{}/fixtures/aln/datum/dof-cases.json", fid_testkit::WORKSPACE_ROOT);
    let bytes = std::fs::read(file).unwrap();
    assert!(!bytes.contains(&b'\r'), "fixture must use LF line endings");
    let mut p = Parser { b: &bytes, i: 0 };
    let v = p.value();
    p.ws();
    assert_eq!(p.i, bytes.len(), "trailing bytes");
    assert_eq!(v.get("format").str(), "aln-dof-cases/1");
    v
}

fn features(case: &J) -> Vec<NominalFeature> {
    case.get("pairs")
        .arr()
        .iter()
        .map(|p| {
            let ty = FeatureType::from_name(p.get("type").str()).unwrap();
            let direction = match p.get("direction") {
                J::Null => None,
                d => Some(d.vec3()),
            };
            NominalFeature::new(ty, p.get("location").vec3(), direction)
        })
        .collect()
}

fn policy(case: &J) -> UnconstrainedPolicy {
    match case.get("policy").str() {
        "keep_parent" => UnconstrainedPolicy::KeepParent,
        "reject" => UnconstrainedPolicy::Reject,
        p => panic!("unknown policy {p}"),
    }
}

fn warning_strings(a: &DofAccount) -> Vec<String> {
    a.warnings()
        .iter()
        .map(|w| match w {
            DofWarning::RedundantPair { pair } => format!("redundant_pair:{pair}"),
            other => panic!("unexpected warning {other:?}"),
        })
        .collect()
}

fn parse_ratio(s: &str) -> f64 {
    let (n, d) = s.split_once('/').unwrap();
    n.parse::<f64>().unwrap() / d.parse::<f64>().unwrap()
}

// ------------------------------------------------------------------ AC-1: the fixture

#[test]
fn ac1_fixture_counts_names_and_projectors() {
    let fx = fixture();
    let cases = fx.get("cases").arr();
    assert_eq!(cases.len(), 22);
    let mut checked = 0usize;
    for case in cases {
        let name = case.get("name").str();
        let got = accumulate(&features(case), policy(case));
        let expect = case.get("expect");
        if let Some(err) = expect.opt("error") {
            let e = got.expect_err(name);
            assert_eq!(e.code(), err.get("code").str(), "{name}");
            match err.get("pair") {
                J::Null => assert_eq!(e.pair(), None, "{name}"),
                p => assert_eq!(e.pair(), Some(p.usize()), "{name}"),
            }
            if let Some(names) = err.opt("free_names") {
                let DofError::Underconstrained { free } = &e else { panic!("{name}: {e:?}") };
                let want: Vec<&str> = names.arr().iter().map(J::str).collect();
                let got: Vec<&str> = free.iter().map(|d| d.name()).collect();
                assert_eq!(got, want, "{name}");
            }
            checked += 1;
            continue;
        }
        let ok = expect.get("ok");
        let a = got.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(a.radius_basis().as_str(), ok.get("radius_basis").str(), "{name}");
        let r2 = parse_ratio(ok.get("radius_sq").str());
        let r = a.radius();
        assert!((r * r - r2).abs() <= 1e-15 * r2, "{name}: radius {r} vs {r2}");
        let pp = ok.get("per_pair").arr();
        assert_eq!(a.per_pair().len(), pp.len(), "{name}");
        for (i, (g, e)) in a.per_pair().iter().zip(pp).enumerate() {
            assert_eq!(g.nominal(), e.get("nominal").usize(), "{name} pair {i} nominal");
            assert_eq!(g.effective(), e.get("effective").usize(), "{name} pair {i} effective");
            assert_eq!(g.free_before(), e.get("free_before").usize(), "{name} pair {i}");
            assert_eq!(g.free_after(), e.get("free_after").usize(), "{name} pair {i}");
            let want = e.get("projector_before").arr();
            assert_eq!(want.len(), 36);
            for (k, w) in want.iter().enumerate() {
                let x = g.projector_before()[k / 6][k % 6];
                assert!((x - w.num()).abs() <= 1e-12, "{name} pair {i} projector[{k}] {x} vs {}", w.num());
            }
            // The rank decisions are far from the threshold in every hand-built case.
            if let Some(s) = g.kept_min() {
                assert!(s > 1e-3, "{name} pair {i} kept {s}");
            }
            if let Some(s) = g.dropped_max() {
                assert!(s < 1e-12, "{name} pair {i} dropped {s}");
            }
        }
        assert_eq!(a.free_count(), ok.get("free_count").usize(), "{name}");
        let want: Vec<&str> = ok.get("free_names").arr().iter().map(J::str).collect();
        assert_eq!(a.free_names(), want, "{name}");
        assert_eq!(a.directive().as_str(), ok.get("directive").str(), "{name}");
        let want: Vec<String> = ok.get("warnings").arr().iter().map(|w| w.str().to_string()).collect();
        assert_eq!(warning_strings(&a), want, "{name}");
        assert_eq!(a.naming_fallbacks(), 0, "{name}");
        assert_eq!(a.rule_basis(), RuleBasis::DefaultUnverified);
        assert_eq!(a.interpretation(), "provisional");
        assert_eq!(a.rank_tolerance(), DOF_RANK_TOL);
        assert!(a.rank_decisions() > 0);
        checked += 1;
    }
    assert_eq!(checked, cases.len());
}

// ------------------------------------------------------------------ AC-2 to AC-5 by hand

fn plane(p: [f64; 3], n: [f64; 3]) -> NominalFeature {
    NominalFeature::new(FeatureType::Plane, p, Some(n))
}

fn axial(t: FeatureType, p: [f64; 3], a: [f64; 3]) -> NominalFeature {
    NominalFeature::new(t, p, Some(a))
}

fn point(t: FeatureType, p: [f64; 3]) -> NominalFeature {
    NominalFeature::new(t, p, None)
}

#[test]
fn ac2_plane_cylinder_point() {
    let pairs = [
        plane([0.0; 3], [0.0, 0.0, 1.0]),
        axial(FeatureType::Cylinder, [0.0, 0.0, 0.01], [1.0, 0.0, 0.0]),
        point(FeatureType::Point, [0.05, 0.03, 0.0]),
    ];
    let a = accumulate(&pairs, UnconstrainedPolicy::Reject).unwrap();
    let eff: Vec<usize> = a.per_pair().iter().map(|p| p.effective()).collect();
    let nom: Vec<usize> = a.per_pair().iter().map(|p| p.nominal()).collect();
    assert_eq!(eff, [3, 2, 1]);
    assert_eq!(nom, [3, 4, 3]);
    assert_eq!(a.free_count(), 0);
    assert_eq!(a.directive(), FreeDirective::FullyConstrained);
    assert!(a.warnings().is_empty());
}

#[test]
fn ac3_coaxial_cylinder_leaves_rotation_free() {
    let pairs = [plane([0.0; 3], [0.0, 0.0, 1.0]), axial(FeatureType::Cylinder, [0.01, 0.02, 0.0], [0.0, 0.0, 1.0])];
    let a = accumulate(&pairs, UnconstrainedPolicy::KeepParent).unwrap();
    let eff: Vec<usize> = a.per_pair().iter().map(|p| p.effective()).collect();
    assert_eq!(eff, [3, 2]);
    assert_eq!(a.free_count(), 1);
    let d = &a.free()[0];
    assert_eq!(d.motion(), Motion::Rotation);
    assert_eq!(d.name(), "rotation about +z through the axis of pair 1 (cylinder)");
    assert_eq!(d.through(), Some(Through::Axis { pair: 1, feature_type: FeatureType::Cylinder }));
    let q = d.point().unwrap();
    assert!((q[0] - 0.01).abs() < 1e-15 && (q[1] - 0.02).abs() < 1e-15 && q[2].abs() < 1e-15, "{q:?}");
    assert!(d.pitch().unwrap().abs() < 1e-15);
    assert_eq!(a.directive(), FreeDirective::HoldAtParent);

    let e = accumulate(&pairs, UnconstrainedPolicy::Reject).unwrap_err();
    assert_eq!(e.code(), "underconstrained");
    let DofError::Underconstrained { free } = &e else { panic!("{e:?}") };
    assert_eq!(free.len(), 1);
    assert_eq!(free[0].name(), "rotation about +z through the axis of pair 1 (cylinder)");
    assert!(e.to_string().contains("rotation about +z"));
}

#[test]
fn ac4_parallel_planes_redundant() {
    let pairs = [plane([0.0; 3], [0.0, 0.0, 1.0]), plane([0.0, 0.0, 0.05], [0.0, 0.0, -1.0])];
    let a = accumulate(&pairs, UnconstrainedPolicy::KeepParent).unwrap();
    assert_eq!(a.per_pair()[1].effective(), 0);
    assert_eq!(a.warnings(), &[DofWarning::RedundantPair { pair: 1 }]);
    assert_eq!(a.warnings()[0].code(), "redundant_pair");
    assert_eq!(a.free_count(), 3);
}

#[test]
fn ac5_torus_unsupported() {
    let t = axial(FeatureType::Torus, [0.0; 3], [0.0, 0.0, 1.0]);
    let e = accumulate(&[plane([0.0; 3], [0.0, 0.0, 1.0]), t], UnconstrainedPolicy::KeepParent).unwrap_err();
    assert_eq!(e, DofError::UnsupportedFeatureType { pair: 1, feature_type: FeatureType::Torus });
    assert_eq!(e.code(), "unsupported_feature_type");
    assert_eq!(constrained_basis(&t).unwrap_err().code(), "unsupported_feature_type");
    assert_eq!(FeatureType::Torus.constrained_count(), None);
}

// ------------------------------------------------------------------ REQ-001: the table

#[test]
fn constrained_counts_and_invariance() {
    let cases = [
        (plane([0.1, 0.2, 0.3], [0.0, 0.6, 0.8]), 3),
        (axial(FeatureType::Line, [0.1, 0.2, 0.3], [1.0, 2.0, 2.0]), 4),
        (axial(FeatureType::Cylinder, [0.1, 0.2, 0.3], [0.0, 0.0, -3.0]), 4),
        (axial(FeatureType::Cone, [0.1, 0.2, 0.3], [0.0, 1.0, 0.0]), 5),
        (axial(FeatureType::Circle3d, [0.1, 0.2, 0.3], [1.0, 0.0, 0.0]), 5),
        (point(FeatureType::Sphere, [0.1, 0.2, 0.3]), 3),
        (point(FeatureType::Point, [0.1, 0.2, 0.3]), 3),
    ];
    for (f, n) in cases {
        let b = constrained_basis(&f).unwrap();
        assert_eq!(b.constrained(), n, "{:?}", f.feature_type);
        assert_eq!(b.dim(), 6 - n);
        assert_eq!(f.feature_type.constrained_count(), Some(n));
        // Every basis twist leaves the feature invariant to first order: the velocity at the
        // location is along the axis (or in the plane, or zero), and the direction only turns
        // about itself.
        let p = f.location;
        for t in b.vectors() {
            let (w, v) = ([t[0], t[1], t[2]], [t[3], t[4], t[5]]);
            let vel = add(v, cross(w, p));
            match f.feature_type {
                FeatureType::Plane => {
                    let n = unit3(f.direction.unwrap());
                    assert!(dot3(vel, n).abs() < 1e-15 && norm3(cross(w, n)) < 1e-15);
                }
                FeatureType::Line | FeatureType::Cylinder => {
                    let a = unit3(f.direction.unwrap());
                    assert!(norm3(cross(vel, a)) < 1e-15 && norm3(cross(w, a)) < 1e-15);
                }
                FeatureType::Cone | FeatureType::Circle3d => {
                    let a = unit3(f.direction.unwrap());
                    assert!(norm3(vel) < 1e-15 && norm3(cross(w, a)) < 1e-15);
                }
                _ => assert!(norm3(vel) < 1e-15),
            }
        }
    }
    for t in FeatureType::ALL {
        assert_eq!(FeatureType::from_name(t.as_str()), Some(t));
    }
    assert_eq!(FeatureType::from_name("Plane"), None);
    assert_eq!(FeatureType::from_name("ellipse"), None);
}

// ------------------------------------------------------------------ REQ-003, REQ-004

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm3(a: [f64; 3]) -> f64 {
    dot3(a, a).sqrt()
}
fn unit3(a: [f64; 3]) -> [f64; 3] {
    let n = norm3(a);
    [a[0] / n, a[1] / n, a[2] / n]
}

/// Distance of `x` from the span of `basis` (Gram-Schmidt in the test, twice).
fn off_span(basis: &[Twist], x: &Twist) -> f64 {
    let mut on: Vec<Twist> = Vec::new();
    for b in basis {
        let mut r = *b;
        for _ in 0..2 {
            for q in &on {
                let g: f64 = q.iter().zip(&r).map(|(a, b)| a * b).sum();
                for (ri, qi) in r.iter_mut().zip(q) {
                    *ri -= g * qi;
                }
            }
        }
        let n = r.iter().map(|x| x * x).sum::<f64>().sqrt();
        on.push(r.map(|x| x / n));
    }
    let mut r = *x;
    for _ in 0..2 {
        for q in &on {
            let g: f64 = q.iter().zip(&r).map(|(a, b)| a * b).sum();
            for (ri, qi) in r.iter_mut().zip(q) {
                *ri -= g * qi;
            }
        }
    }
    r.iter().map(|x| x * x).sum::<f64>().sqrt()
}

fn ok_cases() -> Vec<(String, Vec<NominalFeature>)> {
    let fx = fixture();
    fx.get("cases")
        .arr()
        .iter()
        .filter(|c| c.get("expect").opt("ok").is_some())
        .map(|c| (c.get("name").str().to_string(), features(c)))
        .collect()
}

#[test]
fn free_basis_is_orthonormal_and_invariant() {
    for (name, pairs) in ok_cases() {
        let a = accumulate(&pairs, UnconstrainedPolicy::KeepParent).unwrap();
        let free = a.free();
        for (i, x) in free.iter().enumerate() {
            for (j, y) in free.iter().enumerate() {
                let g: f64 = x.scaled().iter().zip(y.scaled().iter()).map(|(p, q)| p * q).sum();
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((g - want).abs() < 1e-12, "{name}: <{i},{j}> = {g}");
            }
            // Every free motion leaves every nominal feature invariant.
            for (k, f) in pairs.iter().enumerate() {
                let b = constrained_basis(f).unwrap();
                let t = x.twist();
                let s = t.iter().map(|v| v * v).sum::<f64>().sqrt();
                assert!(off_span(b.vectors(), &t) <= 1e-12 * s, "{name}: free {i} moves pair {k}");
            }
            let d = x.direction();
            assert!((norm3(d) - 1.0).abs() < 1e-15);
            match x.motion() {
                Motion::Translation => assert!(x.point().is_none() && x.pitch().is_none()),
                Motion::Rotation => assert!(x.point().is_some() && x.pitch().unwrap().abs() < 1e-12),
                Motion::Twist => panic!("{name}: no screw motion expected"),
            }
        }
    }
}

/// Deterministic pseudo-random twists (splitmix64), test-only.
fn twists(seed: u64, n: usize) -> Vec<Twist> {
    let mut s = seed;
    let mut next = || {
        s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 11) as f64) * (1.0 / 9_007_199_254_740_992.0) * 2.0 - 1.0
    };
    (0..n).map(|_| [next(), next(), next(), next(), next(), next()]).collect()
}

#[test]
fn projectors_keep_higher_priority_features_invariant() {
    for (name, pairs) in ok_cases() {
        let a = accumulate(&pairs, UnconstrainedPolicy::KeepParent).unwrap();
        for (i, p) in a.per_pair().iter().enumerate() {
            let m = p.projector_before();
            // Symmetric, idempotent, trace = free dimension.
            let mut tr = 0.0;
            for r in 0..6 {
                tr += m[r][r];
                for c in 0..6 {
                    assert_eq!(m[r][c].to_bits(), m[c][r].to_bits(), "{name} pair {i}: not symmetric");
                    let sq: f64 = (0..6).map(|k| m[r][k] * m[k][c]).sum();
                    assert!((sq - m[r][c]).abs() < 1e-12, "{name} pair {i}: not idempotent");
                }
            }
            assert!((tr - p.free_before() as f64).abs() < 1e-12, "{name} pair {i}: trace {tr}");
            for t in twists(7 + i as u64, 16) {
                let y = a.project(i, &t).unwrap().unwrap();
                let s = y.iter().map(|v| v * v).sum::<f64>().sqrt();
                for (k, f) in pairs.iter().enumerate().take(i) {
                    let b = constrained_basis(f).unwrap();
                    assert!(off_span(b.vectors(), &y) <= 1e-12 * (1.0 + s), "{name}: pair {i} update moves pair {k}");
                }
            }
        }
        assert!(a.project(pairs.len(), &[0.0; 6]).is_none());
    }
    let a = accumulate(&[plane([0.0; 3], [0.0, 0.0, 1.0])], UnconstrainedPolicy::KeepParent).unwrap();
    let e = a.project(0, &[f64::NAN, 0.0, 0.0, 0.0, 0.0, 0.0]).unwrap().unwrap_err();
    assert_eq!(e.code(), "nan_input");
}

#[test]
fn off_axis_rotation_is_named_by_point() {
    // A plane and a sphere: the free rotation about +z goes through the sphere centre.
    let pairs = [plane([0.0; 3], [0.0, 0.0, 1.0]), point(FeatureType::Sphere, [0.03, 0.04, 0.0])];
    let a = accumulate(&pairs, UnconstrainedPolicy::KeepParent).unwrap();
    assert_eq!(a.free_names(), ["rotation about +z through the centre of pair 1 (sphere)"]);
    // A line far from the origin: rotation about its axis and translation along it.
    let pairs = [axial(FeatureType::Line, [0.3, 0.4, 0.0], [0.0, 0.0, 1.0])];
    let a = accumulate(&pairs, UnconstrainedPolicy::KeepParent).unwrap();
    assert_eq!(a.free_names(), ["translation along +z", "rotation about +z through the axis of pair 0 (line)"]);
    assert_eq!(a.radius_basis(), RadiusBasis::MaxLocation);
    assert!((a.radius() - 0.5).abs() < 1e-16);
}

// ------------------------------------------------------------------ error paths

#[test]
fn refusals() {
    let ok = plane([0.0; 3], [0.0, 0.0, 1.0]);
    let kp = UnconstrainedPolicy::KeepParent;
    assert_eq!(accumulate(&[], kp).unwrap_err(), DofError::EmptyPairs);
    let many = vec![ok; MAX_DOF_PAIRS + 1];
    assert_eq!(accumulate(&many, kp).unwrap_err().code(), "input_limit");

    let nan = plane([0.0, f64::NAN, 0.0], [0.0, 0.0, 1.0]);
    assert_eq!(
        accumulate(&[ok, nan], kp).unwrap_err(),
        DofError::NanInput { pair: Some(1), field: "location", index: 1 }
    );
    let inf = axial(FeatureType::Cylinder, [0.0; 3], [0.0, f64::NEG_INFINITY, 0.0]);
    assert_eq!(
        accumulate(&[inf], kp).unwrap_err(),
        DofError::InfiniteInput { pair: Some(0), field: "direction", index: 1 }
    );
    let nan_dir = axial(FeatureType::Cone, [0.0; 3], [f64::NAN, 0.0, 1.0]);
    assert_eq!(accumulate(&[nan_dir], kp).unwrap_err().code(), "nan_input");
    assert_eq!(
        accumulate(&[NominalFeature::new(FeatureType::Line, [0.0; 3], None)], kp).unwrap_err(),
        DofError::MissingDirection { pair: 0 }
    );
    assert_eq!(
        accumulate(&[NominalFeature::new(FeatureType::Point, [0.0; 3], Some([1.0, 0.0, 0.0]))], kp).unwrap_err(),
        DofError::UnexpectedDirection { pair: 0 }
    );
    assert_eq!(
        accumulate(&[ok, axial(FeatureType::Circle3d, [0.0; 3], [0.0, -0.0, 0.0])], kp).unwrap_err(),
        DofError::ZeroDirection { pair: 1 }
    );
    // The first refusal in pair order wins.
    let e = accumulate(&[nan, axial(FeatureType::Torus, [0.0; 3], [0.0, 0.0, 1.0])], kp).unwrap_err();
    assert_eq!(e.pair(), Some(0));
    // A subnormal direction is still a direction.
    let tiny = accumulate(&[plane([0.0; 3], [0.0, 0.0, 1e-320])], kp).unwrap();
    assert_eq!(tiny.per_pair()[0].effective(), 3);
}

// ------------------------------------------------------------------ AC-6: determinism

#[test]
fn ac6_repeatable() {
    for (name, pairs) in ok_cases() {
        let a = accumulate(&pairs, UnconstrainedPolicy::KeepParent).unwrap();
        let b = accumulate(&pairs, UnconstrainedPolicy::KeepParent).unwrap();
        assert_eq!(a, b, "{name}");
    }
}

/// Output-bit pin: a regression pin of this crate's own output over the fixture's accepted
/// cases (radius, every projector, every free basis vector in both coordinates, its point and
/// pitch, and the kept/dropped singular values), recorded from a local x86-64 run and checked by
/// CI on every target (Linux, Windows, macOS, wasm32). The expected values above come from the
/// independent reference; this pin only shows that every platform produces the same bits.
const OUTPUT_PIN: &str = "9b159af4a263a5b3e23a81eff86aa60fb59ee3785c27492953e8e3a24305d255";

#[test]
fn ac6_output_bits_pinned() {
    let mut h = Sha256::new();
    for (name, pairs) in ok_cases() {
        let a = accumulate(&pairs, UnconstrainedPolicy::KeepParent).unwrap();
        h.update(name.as_bytes());
        h.update(&a.radius().to_le_bytes());
        for p in a.per_pair() {
            for row in p.projector_before() {
                for x in row {
                    h.update(&x.to_le_bytes());
                }
            }
            for x in [p.kept_min(), p.dropped_max()].into_iter().flatten() {
                h.update(&x.to_le_bytes());
            }
        }
        for d in a.free() {
            h.update(d.name().as_bytes());
            for x in d.scaled().iter().chain(d.twist().iter()).chain(d.direction().iter()) {
                h.update(&x.to_le_bytes());
            }
            for x in d.point().into_iter().flatten().chain(d.pitch()) {
                h.update(&x.to_le_bytes());
            }
        }
    }
    let pin = to_hex(&h.finish());
    assert_eq!(pin, OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
