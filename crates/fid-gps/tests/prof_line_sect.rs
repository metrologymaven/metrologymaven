//! Acceptance tests for `GDT.PROF.LINE.SECT` (`fid_gps::prof::line::sect`).
//!
//! Expected values come from the independent oracle
//! `fixtures/gdt/prof/line/make_sect_fixtures.py` (exact rational band membership on the
//! binary64 inputs, 60-digit `decimal` in-plane deviations, and for AC-6 the SHA-256 of the
//! output that the oracle builds itself) or are hand-labelled from the leaf's rules; nothing
//! here computes an expected value with this crate. The deviation sets are built directly
//! (the section step is tested apart from `GDT.PROF.DEV`, as the leaf's Purpose says).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_geo_cad::{Label, TopologyId};
use fid_geo_dist::{SignStatus, SignUndefined};
use fid_geo_frame::Rigid;
use fid_gps::prof::dev::{pose_digest, DevOutcome, Deviation, DeviationSet, Projected};
use fid_gps::prof::line::sect::{
    build_sections, Method, Positions, SectError, SectFlag, Section, SectionSpec, Withheld, MAX_MAGNITUDE,
    MAX_SECTIONS, MIN_EXACT_MAGNITUDE,
};

type V3 = [f64; 3];

const CYL: &str = include_str!("../../../fixtures/gdt/prof/line/cyl_sections.json");
const CONE: &str = include_str!("../../../fixtures/gdt/prof/line/cone_inplane.json");
const CASES: &str = include_str!("../../../fixtures/gdt/prof/line/cases.json");
const PIN: &str = include_str!("../../../fixtures/gdt/prof/line/det_pin.json");

const Z: V3 = [0.0, 0.0, 1.0];

// ------------------------------------------------------------------ JSON helpers

fn get<'a>(v: &'a CValue, key: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(key).unwrap_or(&CValue::Null),
        other => panic!("expected an object for {key}, got {other:?}"),
    }
}

fn has(v: &CValue, key: &str) -> bool {
    matches!(v, CValue::Obj(m) if m.contains_key(key))
}

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(x) => *x,
        CValue::I64(x) => *x as f64,
        CValue::U64(x) => *x as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("expected an array, got {other:?}"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

fn is_null(v: &CValue) -> bool {
    matches!(v, CValue::Null)
}

fn hexf(v: &CValue) -> f64 {
    let s = st(v);
    f64::from_bits(u64::from_str_radix(s.strip_prefix("0x").unwrap(), 16).unwrap())
}

fn hex3(v: &CValue) -> V3 {
    let a = arr(v);
    [hexf(&a[0]), hexf(&a[1]), hexf(&a[2])]
}

fn dec(v: &CValue) -> f64 {
    st(v).parse::<f64>().unwrap()
}

fn idx(v: &CValue) -> u32 {
    num(v) as u32
}

fn rigid(j: &CValue) -> Rigid {
    let r = arr(get(j, "r"));
    Rigid::new([hex3(&r[0]), hex3(&r[1]), hex3(&r[2])], hex3(get(j, "t"))).unwrap()
}

// ------------------------------------------------------------------ deviation sets

fn tid() -> TopologyId {
    TopologyId::Imported(Label::new("F1").unwrap())
}

fn projected(p: V3, signed: Option<f64>, normal: Option<V3>) -> DevOutcome {
    let sign = match signed {
        None => SignStatus::Undefined(SignUndefined::Boundary),
        Some(d) if d < 0.0 => SignStatus::Inside,
        Some(_) => SignStatus::Outside,
    };
    DevOutcome::Projected(Projected {
        face_id: tid(),
        foot_point: p,
        normal,
        distance: signed.map_or(0.0, f64::abs),
        signed_dev: signed,
        sign,
        approx_error: 0.0,
        on_boundary: false,
    })
}

fn set_of(outcomes: Vec<DevOutcome>, pose: &Rigid) -> DeviationSet {
    let items = outcomes.into_iter().enumerate().map(|(k, outcome)| Deviation { index: k as u32, outcome }).collect();
    DeviationSet { items, max_approx_error: Some(0.0), pose_digest: pose_digest(pose).unwrap() }
}

fn spec_of(j: &CValue) -> SectionSpec {
    let pj = get(j, "positions");
    let positions = if is_null(pj) {
        None
    } else if has(pj, "explicit") {
        Some(Positions::Explicit(arr(get(pj, "explicit")).iter().map(hexf).collect()))
    } else {
        Some(Positions::Stepped {
            start: hexf(get(pj, "start")),
            step: hexf(get(pj, "step")),
            count: idx(get(pj, "count")),
        })
    };
    let mj = get(j, "method");
    let method = match mj {
        CValue::Str(s) if s == "normal3d" => Method::Normal3d,
        _ => Method::InPlane { min_inplane: hexf(get(mj, "inplane")) },
    };
    SectionSpec { normal: hex3(get(j, "normal")), positions, half_width_m: hexf(get(j, "w")), method }
}

// ------------------------------------------------------------------ the cases fixture

fn cases() -> CValue {
    parse(CASES.as_bytes()).unwrap()
}

fn find_case<'a>(root: &'a CValue, id: &str) -> &'a CValue {
    arr(get(root, "cases")).iter().find(|c| st(get(c, "id")) == id).unwrap_or_else(|| panic!("no case {id}"))
}

fn run(c: &CValue) -> Result<Vec<Section>, SectError> {
    let pose = rigid(get(c, "pose"));
    let devs_pose = rigid(get(c, "devs_pose"));
    let points: Vec<V3> = arr(get(c, "points")).iter().map(hex3).collect();
    let outcomes = arr(get(c, "devs"))
        .iter()
        .zip(&points)
        .map(|(d, p)| {
            if is_null(d) {
                DevOutcome::NoProjection
            } else {
                let s = get(d, "signed");
                let n = get(d, "normal");
                projected(*p, (!is_null(s)).then(|| hexf(s)), (!is_null(n)).then(|| hex3(n)))
            }
        })
        .collect();
    let mut devs = set_of(outcomes, &devs_pose);
    if matches!(get(c, "swap_dev_index"), CValue::Bool(true)) {
        devs.items.swap(0, 1);
    }
    build_sections(&points, &pose, &devs, &spec_of(get(c, "spec")))
}

fn expected_error(e: &CValue) -> SectError {
    let index = |e: &CValue| if is_null(get(e, "index")) { None } else { Some(idx(get(e, "index"))) };
    match st(get(e, "code")) {
        "section_positions_missing" => SectError::SectionPositionsMissing,
        "non_unit_direction" => SectError::NonUnitDirection,
        "bad_half_width" => SectError::BadHalfWidth,
        "bad_min_inplane" => SectError::BadMinInplane,
        "overlapping_bands" => SectError::OverlappingBands { a: idx(get(e, "a")), b: idx(get(e, "b")) },
        "nan_input" => SectError::NanInput { index: index(e) },
        "out_of_range" => SectError::OutOfRange { index: index(e) },
        "deviation_count_mismatch" => SectError::DeviationCountMismatch,
        "pose_mismatch" => SectError::PoseMismatch,
        other => panic!("unknown expected code {other}"),
    }
}

fn close_rel(got: f64, want: f64, rel: f64) -> bool {
    if want == 0.0 {
        got == 0.0
    } else {
        ((got - want) / want).abs() <= rel
    }
}

fn check_case(c: &CValue) {
    let id = st(get(c, "id"));
    let expect = get(c, "expect");
    let got = run(c);
    if has(expect, "error") {
        let want = expected_error(get(expect, "error"));
        assert_eq!(got, Err(want), "{id}");
        return;
    }
    let got = got.unwrap_or_else(|e| panic!("{id}: unexpected error {e}"));
    let inplane = !matches!(get(get(c, "spec"), "method"), CValue::Str(_));
    let normal = hex3(get(get(c, "spec"), "normal"));
    let want = arr(get(expect, "sections"));
    assert_eq!(got.len(), want.len(), "{id}: section count");
    for (g, w) in got.iter().zip(want) {
        let k = g.index;
        assert_eq!(k, idx(get(w, "index")), "{id}: index");
        assert_eq!(g.normal.map(f64::to_bits), normal.map(f64::to_bits), "{id}/{k}: normal");
        assert_eq!(g.position_m.to_bits(), hexf(get(w, "position")).to_bits(), "{id}/{k}: position");
        let members: Vec<u32> = arr(get(w, "members")).iter().map(idx).collect();
        assert_eq!(g.members, members, "{id}/{k}: members");
        let devs = arr(get(w, "dev"));
        assert_eq!(g.dev_m.len(), devs.len(), "{id}/{k}: dev count");
        for (gd, wd) in g.dev_m.iter().zip(devs) {
            if inplane {
                assert!(close_rel(*gd, dec(wd), 1e-12), "{id}/{k}: inplane dev {gd} vs {}", st(wd));
            } else {
                assert_eq!(gd.to_bits(), hexf(wd).to_bits(), "{id}/{k}: normal3d dev");
            }
        }
        let withheld: Vec<(u32, Withheld)> = arr(get(w, "withheld"))
            .iter()
            .map(|x| {
                let a = arr(x);
                let r = match st(&a[1]) {
                    "no_sign" => Withheld::NoSign,
                    "no_normal" => Withheld::NoNormal,
                    o => panic!("reason {o}"),
                };
                (idx(&a[0]), r)
            })
            .collect();
        assert_eq!(g.withheld, withheld, "{id}/{k}: withheld");
        let oblique: Vec<u32> = arr(get(w, "oblique")).iter().map(idx).collect();
        assert_eq!(g.oblique, oblique, "{id}/{k}: oblique");
        assert_eq!(g.excluded_oblique as usize, oblique.len(), "{id}/{k}: oblique count");
        assert_eq!(g.excluded_no_projection, idx(get(w, "no_projection")), "{id}/{k}: no projection");
        let flags: Vec<&str> = g.flags.iter().map(|f| f.code()).collect();
        let want_flags: Vec<&str> = arr(get(w, "flags")).iter().map(st).collect();
        assert_eq!(flags, want_flags, "{id}/{k}: flags");
    }
}

fn check_ids(ids: &[&str]) {
    let root = cases();
    for id in ids {
        check_case(find_case(&root, id));
    }
}

/// Every case of the oracle's `cases.json`.
#[test]
fn all_oracle_cases() {
    let root = cases();
    let list = arr(get(&root, "cases"));
    assert!(list.len() >= 35);
    for c in list {
        check_case(c);
    }
}

// ------------------------------------------------------------------ AC-1

/// AC-1: the cylinder's three sections hold exactly their 1 200 points each and the
/// `normal3d` deviations equal the planted values (bit for bit, so within 1e-12 m).
#[test]
fn ac1_cylinder_sections() {
    let fx = parse(CYL.as_bytes()).unwrap();
    let r: f64 = st(get(&fx, "radius")).parse().unwrap();
    let mut points = Vec::new();
    let mut outcomes = Vec::new();
    let mut planted = Vec::new();
    for (i, pt) in arr(get(&fx, "points")).iter().enumerate() {
        let a = arr(pt);
        let (z, d) = (dec(&a[0]), dec(&a[1]));
        // x and y do not enter the verdict (n = z); computed here only to place the point.
        let th = 2.0 * std::f64::consts::PI * ((i % 1200) as f64) / 1200.0;
        let (c, s) = (th.cos(), th.sin());
        points.push([(r + d) * c, (r + d) * s, z]);
        outcomes.push(projected([r * c, r * s, z], Some(d), Some([c, s, 0.0])));
        planted.push(d);
    }
    assert_eq!(points.len(), 3600);
    let pose = Rigid::IDENTITY;
    let devs = set_of(outcomes, &pose);
    let positions: Vec<f64> = arr(get(&fx, "positions")).iter().map(hexf).collect();
    let spec = SectionSpec {
        normal: hex3(get(&fx, "normal")),
        positions: Some(Positions::Explicit(positions.clone())),
        half_width_m: hexf(get(&fx, "w")),
        method: Method::Normal3d,
    };
    let secs = build_sections(&points, &pose, &devs, &spec).unwrap();
    assert_eq!(secs.len(), 3);
    for (k, (sec, range)) in secs.iter().zip(arr(get(&fx, "expect_members"))).enumerate() {
        let lo = idx(&arr(range)[0]);
        let hi = idx(&arr(range)[1]);
        assert_eq!(sec.index, k as u32);
        assert_eq!(sec.position_m.to_bits(), positions[k].to_bits());
        assert_eq!(sec.members, (lo..=hi).collect::<Vec<u32>>(), "section {k}");
        assert_eq!(sec.members.len(), 1200);
        for (m, d) in sec.members.iter().zip(&sec.dev_m) {
            let want = planted[*m as usize];
            assert_eq!(d.to_bits(), want.to_bits());
            assert!((d - want).abs() <= 1e-12);
        }
        assert!(sec.flags.is_empty() && sec.withheld.is_empty() && sec.oblique.is_empty());
    }
    // Hand-labelled: a radial normal lies in a section perpendicular to the axis (m = 1 up to
    // rounding), so `inplane` keeps every member with the same deviation within 1e-12.
    let spec = SectionSpec { method: Method::InPlane { min_inplane: 0.5 }, ..spec };
    let secs = build_sections(&points, &pose, &devs, &spec).unwrap();
    for sec in &secs {
        assert_eq!(sec.members.len(), 1200);
        for (m, d) in sec.members.iter().zip(&sec.dev_m) {
            assert!(close_rel(*d, planted[*m as usize], 1e-12));
        }
    }
}

// ------------------------------------------------------------------ AC-2

/// AC-2: on the 30 deg cone every `inplane` deviation is the 3D deviation divided by
/// cos 30 deg within 1e-12 relative (and the oracle's exact `d / m` likewise).
#[test]
fn ac2_cone_inplane() {
    let fx = parse(CONE.as_bytes()).unwrap();
    let cos30 = dec(get(&fx, "cos30"));
    let points: Vec<V3> = arr(get(&fx, "points")).iter().map(hex3).collect();
    let normals: Vec<V3> = arr(get(&fx, "surface_normals")).iter().map(hex3).collect();
    let signed: Vec<f64> = arr(get(&fx, "signed")).iter().map(hexf).collect();
    let outcomes =
        points.iter().zip(&normals).zip(&signed).map(|((p, n), d)| projected(*p, Some(*d), Some(*n))).collect();
    let pose = Rigid::IDENTITY;
    let devs = set_of(outcomes, &pose);
    let spec = SectionSpec {
        normal: hex3(get(&fx, "normal")),
        positions: Some(Positions::Explicit(arr(get(&fx, "positions")).iter().map(hexf).collect())),
        half_width_m: hexf(get(&fx, "w")),
        method: Method::InPlane { min_inplane: hexf(get(&fx, "min_inplane")) },
    };
    let secs = build_sections(&points, &pose, &devs, &spec).unwrap();
    let expect: Vec<f64> = arr(get(&fx, "expect_dev")).iter().map(dec).collect();
    assert_eq!(secs.len(), 3);
    for (sec, range) in secs.iter().zip(arr(get(&fx, "expect_members"))) {
        let lo = idx(&arr(range)[0]);
        let hi = idx(&arr(range)[1]);
        assert_eq!(sec.members, (lo..=hi).collect::<Vec<u32>>());
        assert!(sec.flags.is_empty());
        for (m, d) in sec.members.iter().zip(&sec.dev_m) {
            let i = *m as usize;
            assert!(close_rel(*d, expect[i], 1e-12), "point {i}: {d} vs {}", expect[i]);
            assert!(close_rel(*d, signed[i] / cos30, 1e-12), "point {i}: {d} vs d / cos 30");
        }
    }
}

// ------------------------------------------------------------------ AC-3 .. AC-5

/// AC-3: positions [5, 5.05] mm with w = 0.05 mm overlap (`overlapping_bands{0,1}`); no
/// positions is `section_positions_missing`.
#[test]
fn ac3_error_paths() {
    check_ids(&["ac3_overlap", "ac3_missing", "missing_empty_list", "missing_zero_count", "overlap_unsorted"]);
    let root = cases();
    assert_eq!(run(find_case(&root, "ac3_overlap")), Err(SectError::OverlappingBands { a: 0, b: 1 }));
    assert_eq!(run(find_case(&root, "ac3_missing")), Err(SectError::SectionPositionsMissing));
}

/// The gap is exact: a gap that rounds to 2w but is smaller overlaps; identical positions
/// always overlap; bands exactly 2w apart do not, and share their bound.
#[test]
fn overlap_is_exact() {
    check_ids(&["overlap_rounded_gap", "overlap_identical_w0", "touching_bands"]);
}

/// AC-4: a point exactly at |n . p - s| = w is a member; one ulp beyond is not.
#[test]
fn ac4_inclusive_bound() {
    check_ids(&["ac4_inclusive"]);
    let root = cases();
    let secs = run(find_case(&root, "ac4_inclusive")).unwrap();
    assert_eq!(secs[0].members, vec![0, 1, 2]);
}

/// Membership is exact, not on a rounded dot product (points chosen by the oracle where the
/// naive float verdict is wrong both ways).
#[test]
fn membership_is_exact() {
    check_ids(&["exact_not_naive", "near_unit", "exact_domain"]);
}

/// AC-5: with the surface normal parallel to n and min_inplane 0.5, every point is
/// `oblique_section` and the section reports `empty_section`.
#[test]
fn ac5_plane_oblique() {
    check_ids(&["ac5_plane_oblique", "plane_normal3d", "inplane_threshold"]);
    let root = cases();
    let secs = run(find_case(&root, "ac5_plane_oblique")).unwrap();
    assert!(secs[0].has(SectFlag::EmptySection) && secs[0].has(SectFlag::ObliqueSection));
    assert!(secs[0].members.is_empty());
    assert_eq!(secs[0].excluded_oblique, 21);
}

/// REQ-008 and D-045: `no_projection` points are counted per section, unsigned points and
/// (under `inplane`) points without a normal are withheld, never given a value.
#[test]
fn honesty_no_projection_and_withheld() {
    check_ids(&["honesty_normal3d", "honesty_inplane"]);
}

/// The pose maps the points before membership and must be the deviations' pose.
#[test]
fn pose_and_counts() {
    check_ids(&["pose_translation", "pose_rotation", "pose_mismatch", "count_mismatch", "index_mismatch"]);
}

/// REQ-006 and the input checks.
#[test]
fn spec_and_input_errors() {
    check_ids(&[
        "non_unit",
        "nan_point",
        "nan_w",
        "nan_normal",
        "inf_position",
        "nan_signed",
        "nan_surface_normal",
        "negative_w",
        "bad_min_inplane_0",
        "bad_min_inplane_1",
        "bad_min_inplane_2",
        "huge_point",
        "huge_position",
        "stepped",
        "unsorted_chain_order",
    ]);
}

/// Hand-labelled: too many sections is refused before anything is allocated per point.
#[test]
fn too_many_sections() {
    let pose = Rigid::IDENTITY;
    let devs = set_of(Vec::new(), &pose);
    let spec = SectionSpec {
        normal: Z,
        positions: Some(Positions::Stepped { start: 0.0, step: 1.0, count: MAX_SECTIONS as u32 + 1 }),
        half_width_m: 0.1,
        method: Method::Normal3d,
    };
    assert_eq!(build_sections(&[], &pose, &devs, &spec), Err(SectError::TooManySections));
    // No points: every section is empty.
    let spec = SectionSpec { positions: Some(Positions::Stepped { start: 0.0, step: 1.0, count: 3 }), ..spec };
    let secs = build_sections(&[], &pose, &devs, &spec).unwrap();
    assert_eq!(secs.len(), 3);
    assert!(secs.iter().all(|s| s.flags == vec![SectFlag::EmptySection]));
}

/// The documented constants and codes.
#[test]
fn constants_and_codes() {
    assert_eq!(MAX_MAGNITUDE, 2f64.powi(480));
    assert_eq!(MIN_EXACT_MAGNITUDE, 2f64.powi(-480));
    assert_eq!(SectError::OverlappingBands { a: 0, b: 1 }.to_string(), "overlapping_bands{0,1}");
    assert_eq!(SectError::SectionPositionsMissing.code(), "section_positions_missing");
    assert_eq!(SectError::NonUnitDirection.code(), "non_unit_direction");
    assert_eq!(SectError::NanInput { index: Some(3) }.to_string(), "nan_input{point 3}");
    assert_eq!(SectFlag::EmptySection.code(), "empty_section");
    assert_eq!(SectFlag::ObliqueSection.code(), "oblique_section");
    assert_eq!(Withheld::NoSign.code(), "no_sign");
}

// ------------------------------------------------------------------ AC-6 determinism

/// SplitMix64 (test streams only; the oracle runs the same stream).
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        let u = (self.next() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0);
        lo + u * (hi - lo)
    }
}

fn encode(out: &mut Vec<u8>, secs: &[Section]) {
    for s in secs {
        out.extend_from_slice(&s.index.to_le_bytes());
        out.extend_from_slice(&s.position_m.to_bits().to_le_bytes());
        out.extend_from_slice(&(s.members.len() as u32).to_le_bytes());
        for (m, d) in s.members.iter().zip(&s.dev_m) {
            out.extend_from_slice(&m.to_le_bytes());
            out.extend_from_slice(&d.to_bits().to_le_bytes());
        }
        out.extend_from_slice(&(s.withheld.len() as u32).to_le_bytes());
        for (i, r) in &s.withheld {
            out.extend_from_slice(&i.to_le_bytes());
            out.push(match r {
                Withheld::NoSign => 0,
                Withheld::NoNormal => 1,
            });
        }
        out.extend_from_slice(&(s.oblique.len() as u32).to_le_bytes());
        for i in &s.oblique {
            out.extend_from_slice(&i.to_le_bytes());
        }
        out.extend_from_slice(&s.excluded_no_projection.to_le_bytes());
        let mut flags = 0u8;
        for f in &s.flags {
            flags |= match f {
                SectFlag::EmptySection => 1,
                SectFlag::ObliqueSection => 2,
                SectFlag::NoProjection => 4,
                SectFlag::Withheld => 8,
            };
        }
        out.push(flags);
    }
}

/// AC-6: 1 000 000 seeded points (seed 0x5ec7) give bit-identical sections on every CI
/// platform: the digest equals the one the oracle computed from its own exact membership and
/// binary64 replication.
#[test]
fn ac6_bit_identical_sections() {
    let pin = parse(PIN.as_bytes()).unwrap();
    let count = idx(get(&pin, "count")) as usize;
    assert_eq!(count, 1_000_000);
    let mut rng = SplitMix(num(get(&pin, "seed")) as u64);
    assert_eq!(num(get(&pin, "seed")) as u64, 0x5ec7);
    let mut points = Vec::with_capacity(count);
    let mut outcomes = Vec::with_capacity(count);
    for _ in 0..count {
        let p = [rng.uniform(-0.05, 0.05), rng.uniform(-0.05, 0.05), rng.uniform(0.0, 0.1)];
        let kind = rng.next() % 16;
        let d = rng.uniform(-1e-4, 1e-4);
        let v = [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)];
        let ln = ((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2]).sqrt();
        let ns = if ln < 0.25 { Z } else { [v[0] / ln, v[1] / ln, v[2] / ln] };
        points.push(p);
        outcomes.push(match kind {
            0 => DevOutcome::NoProjection,
            1 => projected(p, None, Some(ns)),
            2 => projected(p, Some(d), None),
            _ => projected(p, Some(d), Some(ns)),
        });
    }
    let pose = rigid(get(&pin, "pose"));
    let devs = set_of(outcomes, &pose);
    let positions = Some(Positions::Stepped {
        start: hexf(get(&pin, "start")),
        step: hexf(get(&pin, "step")),
        count: idx(get(&pin, "sections")),
    });
    let spec = SectionSpec {
        normal: hex3(get(&pin, "normal")),
        positions,
        half_width_m: hexf(get(&pin, "w")),
        method: Method::Normal3d,
    };
    let a = build_sections(&points, &pose, &devs, &spec).unwrap();
    let spec = SectionSpec { method: Method::InPlane { min_inplane: hexf(get(&pin, "min_inplane")) }, ..spec };
    let b = build_sections(&points, &pose, &devs, &spec).unwrap();
    let total = |s: &[Section]| s.iter().map(|x| x.members.len()).sum::<usize>();
    assert_eq!(total(&a), idx(get(&pin, "members_normal3d")) as usize);
    assert_eq!(total(&b), idx(get(&pin, "members_inplane")) as usize);
    let mut bytes = Vec::new();
    encode(&mut bytes, &a);
    encode(&mut bytes, &b);
    let got = fid_types::Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
    assert_eq!(got.as_str(), st(get(&pin, "digest")), "AC-6 output digest");
}
