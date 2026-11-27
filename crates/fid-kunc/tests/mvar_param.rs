//! Acceptance criteria of spec node `UNC.PROP.MVAR.PARAM` (AC-1..AC-4,
//! REQ-UNC.PROP.MVAR.PARAM-001..004). AC-H1 is a hypothesis over the `UNC.RDL.GEN.LSNULL` and
//! `UNC.RDL.GEN.GEOM` families of a frozen RDL release, which do not exist yet; it is not
//! tested here.
//!
//! Expected parameters, bases, conversion Jacobians, propagated covariances and eigenvalues
//! come from the independent stdlib Python reference `fixtures/unc/mvar/param/make_fixtures.py`
//! (80-digit `decimal`, Jacobians by central differences, eigenvalues by its own decimal
//! Jacobi), never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_fit::contract::{FittedFeature, Primitive, V3};
use fid_kunc::prop::mvar::param::{
    check_covariance, from_param, tangent_basis, to_param, CovarianceDefect, FeatureCovariance, FeatureType,
    ParamError, ASYM_REL_TOL, PSD_REL_TOL, VERSION,
};
use fid_num::la::Mat;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/mvar/param/");
const U: f64 = 1.0 / 9_007_199_254_740_992.0;

// ------------------------------------------------------------------------ a minimal JSON reader

#[derive(Debug, Clone)]
enum Json {
    Null,
    Bool,
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) {
        self.ws();
        assert_eq!(self.s[self.i], c, "expected {} at {}", c as char, self.i);
        self.i += 1;
    }

    fn value(&mut self) -> Json {
        self.ws();
        match self.s[self.i] {
            b'{' => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.s[self.i] == b'}' {
                    self.i += 1;
                    return Json::Obj(out);
                }
                loop {
                    let Json::Str(k) = self.value() else { panic!("key") };
                    self.eat(b':');
                    let v = self.value();
                    assert!(out.iter().all(|(o, _)| *o != k), "duplicate key {k}");
                    out.push((k, v));
                    self.ws();
                    let c = self.s[self.i];
                    self.i += 1;
                    if c == b'}' {
                        return Json::Obj(out);
                    }
                    assert_eq!(c, b',');
                }
            }
            b'[' => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.s[self.i] == b']' {
                    self.i += 1;
                    return Json::Arr(out);
                }
                loop {
                    out.push(self.value());
                    self.ws();
                    let c = self.s[self.i];
                    self.i += 1;
                    if c == b']' {
                        return Json::Arr(out);
                    }
                    assert_eq!(c, b',');
                }
            }
            b'"' => {
                self.i += 1;
                let start = self.i;
                while self.s[self.i] != b'"' {
                    assert_ne!(self.s[self.i], b'\\', "escapes are not used in these fixtures");
                    self.i += 1;
                }
                let out = std::str::from_utf8(&self.s[start..self.i]).unwrap().to_string();
                self.i += 1;
                Json::Str(out)
            }
            b'n' => {
                self.i += 4;
                Json::Null
            }
            b't' => {
                self.i += 4;
                Json::Bool
            }
            b'f' => {
                self.i += 5;
                Json::Bool
            }
            _ => {
                let start = self.i;
                while self.i < self.s.len() && b"+-.0123456789eE".contains(&self.s[self.i]) {
                    self.i += 1;
                }
                Json::Num(std::str::from_utf8(&self.s[start..self.i]).unwrap().to_string())
            }
        }
    }
}

fn parse(text: &str) -> Json {
    let mut p = Parser { s: text.as_bytes(), i: 0 };
    let v = p.value();
    p.ws();
    assert_eq!(p.i, p.s.len(), "trailing data");
    v
}

impl Json {
    fn get(&self, key: &str) -> &Json {
        match self {
            Json::Obj(kv) => &kv.iter().find(|(k, _)| k == key).unwrap_or_else(|| panic!("missing {key}")).1,
            _ => panic!("not an object looking for {key}"),
        }
    }

    fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            other => panic!("not an array: {other:?}"),
        }
    }

    fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            other => panic!("not a string: {other:?}"),
        }
    }

    fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    fn int(&self) -> i64 {
        match self {
            Json::Num(n) => n.parse().unwrap(),
            other => panic!("not a number: {other:?}"),
        }
    }

    fn f(&self) -> f64 {
        f64::from_bits(u64::from_str_radix(self.str(), 16).unwrap())
    }

    fn fs(&self) -> Vec<f64> {
        self.arr().iter().map(Json::f).collect()
    }

    fn v3(&self) -> V3 {
        let v = self.fs();
        [v[0], v[1], v[2]]
    }
}

fn load(name: &str) -> Json {
    parse(&std::fs::read_to_string(format!("{FIX}{name}")).unwrap_or_else(|e| panic!("{name}: {e}")))
}

// ---------------------------------------------------------------------------------- helpers

/// Distance in units in the last place (`+0.0` and `-0.0` are equal).
fn ulps(a: f64, b: f64) -> u64 {
    if a == b {
        return 0;
    }
    let key = |x: f64| {
        let b = x.to_bits() as i64;
        if b < 0 {
            i64::MIN - b
        } else {
            b
        }
    };
    key(a).abs_diff(key(b))
}

fn feature_of(prim: &str, f: &Json) -> FittedFeature {
    match prim {
        "point" => FittedFeature::Point { c: f.get("c").v3() },
        "line3" => FittedFeature::Line3 { p: f.get("p").v3(), d: f.get("d").v3() },
        "plane" => FittedFeature::Plane { p: f.get("p").v3(), n: f.get("n").v3() },
        "circle3" => FittedFeature::Circle3 { c: f.get("c").v3(), n: f.get("n").v3(), r: f.get("r").f() },
        "sphere" => FittedFeature::Sphere { c: f.get("c").v3(), r: f.get("r").f() },
        "cylinder" => FittedFeature::Cylinder { p: f.get("p").v3(), d: f.get("d").v3(), r: f.get("r").f() },
        "cone" => {
            FittedFeature::Cone { apex: f.get("apex").v3(), d: f.get("d").v3(), half_angle: f.get("half_angle").f() }
        }
        other => panic!("primitive {other}"),
    }
}

/// The kernel-form vector (the column order of the conversion Jacobians).
fn flatten(f: &FittedFeature) -> Vec<f64> {
    let mut out = Vec::new();
    let mut v3 = |v: &V3| out.extend_from_slice(v);
    match f {
        FittedFeature::Point { c } => v3(c),
        FittedFeature::Line3 { p, d } => {
            v3(p);
            v3(d);
        }
        FittedFeature::Plane { p, n } => {
            v3(p);
            v3(n);
        }
        FittedFeature::Circle3 { c, n, r } => {
            v3(c);
            v3(n);
            out.push(*r);
        }
        FittedFeature::Sphere { c, r } => {
            v3(c);
            out.push(*r);
        }
        FittedFeature::Cylinder { p, d, r } => {
            v3(p);
            v3(d);
            out.push(*r);
        }
        FittedFeature::Cone { apex, d, half_angle } => {
            v3(apex);
            v3(d);
            out.push(*half_angle);
        }
        other => panic!("not a fcov-param@1 type: {other:?}"),
    }
    out
}

fn square(n: usize, data: Vec<f64>) -> Mat {
    Mat::new(n, n, data).unwrap()
}

fn assert_close(what: &str, got: &[f64], want: &[f64], tol: f64) {
    assert_eq!(got.len(), want.len(), "{what}: length");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!((g - w).abs() <= tol, "{what}[{i}]: got {g:e}, want {w:e}, tol {tol:e}");
    }
}

fn cases() -> Vec<Json> {
    load("features.json").get("cases").arr().to_vec()
}

fn cone_ref(c: &Json) -> Option<V3> {
    let q = c.get("cone_reference");
    if q.is_null() {
        None
    } else {
        Some(q.v3())
    }
}

// ----------------------------------------------------------------------------------- AC-1

/// AC-1: every supported type round-trips feature -> parameters -> feature to 1 ulp per
/// component, and builds a valid `FeatureCovariance`. The cone's parameter-form components
/// (reference point, axis, radius, half-angle) round-trip exactly; its apex is derived
/// (`p - (r / tan) d`) and is held to the rounding bound `8 u (|apex_i| + s)` instead (see the
/// PR and the leaf's Open questions).
#[test]
fn ac1_round_trip_every_type() {
    let mut seen = Vec::new();
    for c in cases() {
        let name = c.get("name").str().to_string();
        let prim = c.get("primitive").str();
        let input = feature_of(prim, c.get("feature"));
        let q = cone_ref(&c);
        let tp = to_param(&input, q).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(tp.ftype.slug(), c.get("type").str(), "{name}");
        seen.push(tp.ftype);
        let back = from_param(tp.ftype, &tp.params, tp.ref_dir).unwrap_or_else(|e| panic!("{name}: {e}"));
        // The kernel's canonical form is what the parameters describe (a sign-flipped line
        // comes back with the canonical sign).
        let canon = input.canonical().unwrap();
        let (want, got) = (flatten(&canon), flatten(&back.feature));
        if c.get("expected").get("sign").int() == 1 {
            for (i, (a, b)) in flatten(&input).iter().zip(&want).enumerate() {
                assert!(ulps(*a, *b) <= 1, "{name}: input is canonical to 1 ulp at {i}");
            }
        }
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            if tp.ftype == FeatureType::Cone && i < 3 {
                let s = c.get("expected").get("reference_distance").f();
                let bound = 8.0 * U * (w.abs() + s);
                assert!((g - w).abs() <= bound, "{name}: apex[{i}] {g:e} vs {w:e} (bound {bound:e})");
            } else {
                assert!(ulps(*g, *w) <= 1, "{name}: component {i}: {g:e} vs {w:e}");
            }
        }
        if let Some(cr) = back.cone_reference {
            assert_eq!(cr.point.to_vec(), tp.params[..3].to_vec(), "{name}: cone reference point");
            assert_eq!(cr.radius.to_bits(), tp.params[5].to_bits(), "{name}: cone radius");
        }
        // A valid covariance from the kernel form, and the same round trip through it.
        let kd = tp.ftype.kernel_dim();
        let kcov = square(kd, c.get("kernel_cov").fs());
        let fc = FeatureCovariance::from_kernel(&input, q, &kcov).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(fc.params(), &tp.params[..], "{name}");
        assert_eq!(flatten(&fc.feature().unwrap().feature), got, "{name}");
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen, FeatureType::ALL.to_vec(), "one case per supported type");
}

/// REQ-001/002: parameters, units, reference direction and tangent basis against the
/// reference.
#[test]
fn req001_002_parameters_units_and_basis() {
    for c in cases() {
        let name = c.get("name").str();
        let e = c.get("expected");
        let input = feature_of(c.get("primitive").str(), c.get("feature"));
        let tp = to_param(&input, cone_ref(&c)).unwrap();
        // Copied components are exact; the cone's reference point and radius are computed.
        assert_close(name, &tp.params, &e.get("params").fs(), 2e-16);
        if tp.ftype != FeatureType::Cone {
            for (g, w) in tp.params.iter().zip(e.get("params").fs()) {
                assert!(g.to_bits() == w.to_bits() || *g == w, "{name}: copied parameter {g:e} vs {w:e}");
            }
        }
        let units: Vec<&str> = tp.ftype.units().iter().map(|u| u.as_str()).collect();
        let want: Vec<&str> = e.get("units").arr().iter().map(Json::str).collect();
        assert_eq!(units, want, "{name}: units");
        assert_eq!(tp.ftype.components().len(), tp.ftype.dim(), "{name}");
        assert_eq!(tp.sign, e.get("sign").int() as f64, "{name}: canonical sign");
        match (tp.ref_dir, e.get("ref_dir")) {
            (None, j) => assert!(j.is_null(), "{name}: reference direction expected"),
            (Some(r), j) => {
                for (g, w) in r.iter().zip(j.fs()) {
                    assert!(ulps(*g, w) <= 2, "{name}: ref_dir {g:e} vs {w:e}");
                }
                let b = tangent_basis(r).unwrap();
                assert_eq!(b.axis as i64, e.get("basis_axis").int(), "{name}: basis axis");
                assert_close(name, &b.e1, &e.get("e1").fs(), 4.0 * U);
                assert_close(name, &b.e2, &e.get("e2").fs(), 4.0 * U);
                // From (0, 0) the reference comes back bit for bit.
                let back = from_param(tp.ftype, &tp.params, tp.ref_dir).unwrap();
                let d = &flatten(&back.feature)[3..6];
                assert_eq!(d, &r[..], "{name}: direction at (0, 0)");
            }
        }
    }
}

/// The conversion Jacobians and the propagated covariance against central differences and
/// the 80-digit sandwich of the reference.
#[test]
fn jacobians_and_propagated_covariance() {
    for c in cases() {
        let name = c.get("name").str();
        let e = c.get("expected");
        let input = feature_of(c.get("primitive").str(), c.get("feature"));
        let q = cone_ref(&c);
        let tp = to_param(&input, q).unwrap();
        assert_eq!((tp.jacobian.rows(), tp.jacobian.cols()), (tp.ftype.dim(), tp.ftype.kernel_dim()));
        assert_close(name, tp.jacobian.data(), &e.get("to_jacobian").fs(), 1e-14);
        let kcov = square(tp.ftype.kernel_dim(), c.get("kernel_cov").fs());
        let fc = FeatureCovariance::from_kernel(&input, q, &kcov).unwrap();
        let n = tp.ftype.dim();
        let want = e.get("cov").fs();
        let got = fc.cov().data();
        for i in 0..n {
            for j in 0..n {
                let scale = (want[i * n + i] * want[j * n + j]).sqrt();
                let (g, w) = (got[i * n + j], want[i * n + j]);
                assert!((g - w).abs() <= 1e-12 * scale, "{name}: cov[{i}][{j}] {g:e} vs {w:e}");
                assert_eq!(g.to_bits(), got[j * n + i].to_bits(), "{name}: exactly symmetric");
            }
        }
        let (lo, hi) = (e.get("min_eig").f(), e.get("max_eig").f());
        let ck = fc.check();
        assert!((ck.max_eig - hi).abs() <= 1e-12 * hi, "{name}: max eig {} vs {hi:e}", ck.max_eig);
        assert!((ck.min_eig - lo).abs() <= 1e-12 * hi, "{name}: min eig {} vs {lo:e}", ck.min_eig);
        assert_eq!(fc.units(), tp.ftype.units());
        assert_eq!(fc.version(), VERSION);
    }
}

#[test]
fn from_param_tilted_directions() {
    let doc = load("features.json");
    for c in doc.get("from_cases").arr() {
        let name = c.get("name").str();
        let prim = Primitive::parse(c.get("primitive").str()).unwrap();
        let ftype = FeatureType::of(VERSION, prim).unwrap();
        let fp = from_param(ftype, &c.get("params").fs(), Some(c.get("ref_dir").v3())).unwrap();
        assert_close(name, &flatten(&fp.feature), &c.get("expected").get("kernel").fs(), 4e-16);
        assert_eq!((fp.jacobian.rows(), fp.jacobian.cols()), (ftype.kernel_dim(), ftype.dim()));
        assert_close(name, fp.jacobian.data(), &c.get("expected").get("from_jacobian").fs(), 1e-14);
        // The rebuilt direction is unit length.
        let d = &flatten(&fp.feature)[3..6];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        assert!((len - 1.0).abs() <= 4.0 * U, "{name}: |d| = {len}");
    }
}

// ----------------------------------------------------------------------------------- AC-2

#[test]
fn ac2_covariance_validation() {
    let doc = load("not_psd.json");
    assert_eq!(doc.get("tolerances").get("asym_rel").str(), "1e-12");
    assert_eq!(doc.get("tolerances").get("psd_rel").str(), "1e-12");
    assert_eq!((ASYM_REL_TOL, PSD_REL_TOL), (1e-12, 1e-12));
    let mut verdicts = Vec::new();
    for c in doc.get("cases").arr() {
        let name = c.get("name").str();
        let e = c.get("expected");
        let cov = square(6, c.get("cov").fs());
        let got = FeatureCovariance::new(
            VERSION,
            Primitive::Cylinder,
            c.get("params").fs(),
            &cov,
            Some(c.get("ref_dir").v3()),
        );
        let (lo, hi) = (e.get("min_eig").f(), e.get("max_eig").f());
        let verdict = e.get("verdict").str();
        verdicts.push(verdict.to_string());
        match (verdict, got) {
            (
                "not_psd",
                Err(
                    err @ ParamError::InvalidCovariance(CovarianceDefect::NotPositiveSemidefinite { min_eig, max_eig }),
                ),
            ) => {
                assert_eq!(err.code(), "invalid_covariance");
                assert!((min_eig - lo).abs() <= 1e-12 * hi, "{name}: smallest eigenvalue {min_eig:e} vs {lo:e}");
                assert!((max_eig - hi).abs() <= 1e-12 * hi, "{name}: largest eigenvalue {max_eig:e} vs {hi:e}");
                assert!(min_eig < 0.0);
            }
            ("asymmetric", Err(err @ ParamError::InvalidCovariance(CovarianceDefect::Asymmetric { asym, i, j }))) => {
                assert_eq!(err.code(), "invalid_covariance");
                assert!(ulps(asym, e.get("asym").f()) <= 2, "{name}: asymmetry {asym:e}");
                let pair: Vec<i64> = e.get("asym_pair").arr().iter().map(Json::int).collect();
                assert_eq!(vec![i as i64, j as i64], pair, "{name}: reported pair");
            }
            ("ok", Ok(fc)) => {
                let ck = fc.check();
                assert!(ulps(ck.asym, e.get("asym").f()) <= 2, "{name}: asymmetry {:e}", ck.asym);
                assert!((ck.min_eig - lo).abs() <= 1e-12 * hi, "{name}: smallest eigenvalue");
                // The stored matrix is the symmetrised one.
                let d = fc.cov().data();
                for i in 0..6 {
                    for j in 0..6 {
                        assert_eq!(d[i * 6 + j].to_bits(), d[j * 6 + i].to_bits(), "{name}: symmetric");
                    }
                }
            }
            (v, other) => panic!("{name}: expected {v}, got {other:?}"),
        }
    }
    verdicts.sort();
    assert_eq!(verdicts, ["asymmetric", "not_psd", "ok", "ok"]);
}

#[test]
fn ac2_negative_definite_and_zero_matrices() {
    let neg = square(3, vec![-1e-12, 0.0, 0.0, 0.0, -2e-12, 0.0, 0.0, 0.0, -3e-12]);
    match check_covariance(&neg, 3) {
        Err(ParamError::InvalidCovariance(CovarianceDefect::NotPositiveSemidefinite { min_eig, max_eig })) => {
            assert_eq!((min_eig, max_eig), (-3e-12, -1e-12));
        }
        other => panic!("{other:?}"),
    }
    let zero = square(3, vec![0.0; 9]);
    let (_, ck) = check_covariance(&zero, 3).unwrap();
    assert_eq!((ck.asym, ck.min_eig, ck.max_eig), (0.0, 0.0, 0.0));
}

// ----------------------------------------------------------------------------------- AC-3

#[test]
fn ac3_parameterisation_missing() {
    let torus = FittedFeature::Torus { c: [0.0; 3], n: [0.0, 0.0, 1.0], major_r: 0.05, minor_r: 0.01 };
    let err = to_param(&torus, None).unwrap_err();
    assert_eq!(err, ParamError::ParameterisationMissing { primitive: Primitive::Torus, version: VERSION.to_string() });
    assert_eq!(err.code(), "parameterisation_missing");
    let cov = square(6, vec![0.0; 36]);
    let err = FeatureCovariance::new(VERSION, Primitive::Torus, vec![0.0; 6], &cov, None).unwrap_err();
    assert_eq!(err.code(), "parameterisation_missing");
    for p in Primitive::ALL {
        let defined = FeatureType::of(VERSION, *p).is_ok();
        let want = matches!(
            p,
            Primitive::Point
                | Primitive::Line3
                | Primitive::Plane
                | Primitive::Circle3
                | Primitive::Sphere
                | Primitive::Cylinder
                | Primitive::Cone
        );
        assert_eq!(defined, want, "{}", p.slug());
    }
    // Another version has no parameterisations at all.
    let err = FeatureType::of("fcov-param@2", Primitive::Cylinder).unwrap_err();
    assert_eq!(
        err,
        ParamError::ParameterisationMissing { primitive: Primitive::Cylinder, version: "fcov-param@2".to_string() }
    );
}

// ----------------------------------------------------------------------------------- AC-4

#[test]
fn ac4_cylinder_along_z_axis_block_is_2x2_full_rank() {
    let c = cases().into_iter().find(|c| c.get("name").str() == "cylinder_z").unwrap();
    let e = c.get("expected");
    let input = feature_of("cylinder", c.get("feature"));
    let kcov_data = c.get("kernel_cov").fs();
    // The kernel form's 3 x 3 direction block is singular (a unit vector has 2 DoF) ...
    let mut dir3 = Vec::new();
    for i in 3..6 {
        for j in 3..6 {
            dir3.push(kcov_data[i * 7 + j]);
        }
    }
    let (_, ck3) = check_covariance(&square(3, dir3), 3).unwrap();
    assert_eq!(ck3.min_eig, 0.0, "kernel direction block is rank 2");
    // ... the tangent-plane block is 2 x 2 and full rank.
    let fc = FeatureCovariance::from_kernel(&input, None, &square(7, kcov_data)).unwrap();
    let blk = fc.direction_block().unwrap().unwrap();
    let flat = [blk[0][0], blk[0][1], blk[1][0], blk[1][1]];
    assert_close("dir_block", &flat, &e.get("dir_block").fs(), 1e-24);
    let (_, ck2) = check_covariance(&square(2, flat.to_vec()), 2).unwrap();
    assert!(ck2.min_eig > 0.0 && ck2.min_eig >= 1e-3 * ck2.max_eig, "full rank: {ck2:?}");
    assert!((ck2.min_eig - e.get("dir_block_min_eig").f()).abs() <= 1e-12 * ck2.max_eig);
    let b = fc.tangent_basis().unwrap().unwrap();
    assert_eq!((b.axis, b.e1, b.e2), (0, [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]));
    assert_eq!(fc.ref_dir(), Some([0.0, 0.0, 1.0]));
}

// ------------------------------------------------------------------------------ error paths

#[test]
fn refusals_name_their_field() {
    let cyl = FittedFeature::Cylinder { p: [0.0; 3], d: [0.0, 0.0, 1.0], r: 0.01 };
    let cone = FittedFeature::Cone { apex: [0.0, 0.0, -0.1], d: [0.0, 0.0, 1.0], half_angle: 0.3 };
    let field = |r: Result<_, ParamError>| match r {
        Err(ParamError::InvalidParams { field, .. }) => field,
        other => panic!("{other:?}"),
    };
    // A cone needs its reference point; no other type takes one (never silently ignored).
    assert_eq!(field(to_param(&cone, None).map(|_| ())), "cone_reference");
    assert_eq!(field(to_param(&cyl, Some([0.0; 3])).map(|_| ())), "cone_reference");
    // The reference point must lie beyond the apex.
    assert_eq!(field(to_param(&cone, Some([0.0, 0.0, -0.2])).map(|_| ())), "cone_reference");
    assert_eq!(field(to_param(&cone, Some([0.0, 0.0, -0.1])).map(|_| ())), "cone_reference");
    assert_eq!(field(to_param(&cone, Some([f64::NAN, 0.0, 0.0])).map(|_| ())), "cone_reference");
    // The kernel's own refusals pass through.
    let bad_r = FittedFeature::Cylinder { p: [0.0; 3], d: [0.0, 0.0, 1.0], r: -1.0 };
    assert_eq!(to_param(&bad_r, None).unwrap_err().code(), "invalid_feature");
    // Parameter vectors.
    let ok = [0.0, 0.0, 0.0, 0.0, 0.0, 0.01];
    let z = Some([0.0, 0.0, 1.0]);
    assert_eq!(field(from_param(FeatureType::Cylinder, &ok[..5], z).map(|_| ())), "params");
    assert_eq!(
        field(from_param(FeatureType::Cylinder, &[0.0, f64::INFINITY, 0.0, 0.0, 0.0, 0.01], z).map(|_| ())),
        "params"
    );
    assert_eq!(field(from_param(FeatureType::Cylinder, &ok, None).map(|_| ())), "ref_dir");
    assert_eq!(field(from_param(FeatureType::Point, &ok[..3], z).map(|_| ())), "ref_dir");
    assert_eq!(field(from_param(FeatureType::Cylinder, &ok, Some([0.0, 0.0, 1.001])).map(|_| ())), "ref_dir");
    assert_eq!(field(from_param(FeatureType::Cylinder, &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0], z).map(|_| ())), "r");
    let cone_p = [0.0, 0.0, 0.0, 0.0, 0.0, 0.01, 1.6];
    assert_eq!(field(from_param(FeatureType::Cone, &cone_p, z).map(|_| ())), "half_angle");
    // Covariance shape.
    let err =
        FeatureCovariance::new(VERSION, Primitive::Cylinder, ok.to_vec(), &square(5, vec![0.0; 25]), z).unwrap_err();
    assert_eq!(err, ParamError::DimensionMismatch { expected: 6, rows: 5, cols: 5 });
    let err = FeatureCovariance::from_kernel(&cyl, None, &square(6, vec![0.0; 36])).unwrap_err();
    assert_eq!(err, ParamError::DimensionMismatch { expected: 7, rows: 6, cols: 6 });
}

#[test]
fn tangent_basis_rules() {
    // Ties go to the lower index: (1, 1, 1) / sqrt 3 crosses with x.
    let s = 1.0 / 3.0_f64.sqrt();
    assert_eq!(tangent_basis([s, s, s]).unwrap().axis, 0);
    assert_eq!(tangent_basis([0.6, 0.0, 0.8]).unwrap().axis, 1);
    assert_eq!(tangent_basis([0.6, 0.8, 0.0]).unwrap().axis, 2);
    assert_eq!(tangent_basis([0.0, 0.0, -1.0]).unwrap().axis, 0);
    // Right-handed and orthonormal.
    for r in [[s, s, s], [0.6, 0.0, 0.8], [0.0, -0.6, 0.8], [0.0, 0.0, -1.0]] {
        let b = tangent_basis(r).unwrap();
        let (e1, e2) = (b.e1, b.e2);
        let x = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        assert_close("e1 x e2", &x, &r, 8.0 * U);
        let d = |a: &V3, b: &V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        assert!(d(&e1, &e2).abs() <= 4.0 * U && d(&e1, &r).abs() <= 4.0 * U && d(&e2, &r).abs() <= 4.0 * U);
    }
    assert!(tangent_basis([0.0, 0.0, 2.0]).is_err());
    assert!(tangent_basis([f64::NAN, 0.0, 1.0]).is_err());
}
