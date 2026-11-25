//! Acceptance criteria of spec node GDT.PROF.ZDEF.UNIF (AC-1 .. AC-6) on `prof::zdef`.
//! Inputs and expected values come from the independent stdlib-Python oracle
//! `fixtures/gdt/prof/zdef/make_zdef_fixtures.py` (exact rational arithmetic, one explicit
//! rounding per IEEE-754 operation): `constant.json` (AC-1, AC-3, AC-4, AC-5), `property.json`
//! (AC-2) and `rows.json` (synthetic DMT rows derived from the synthetic table). Comparing
//! bit patterns on every CI platform is AC-6. Invented values only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{digest_bytes, CValue};
use fid_geo_cad::{Label, TopologyId};
use fid_geo_dist::{SignStatus, SignUndefined};
use fid_gps::dmt::{DmtRow, DmtTable, OperatorChain};
use fid_gps::prof::dev::{DevOutcome, Deviation, Projected};
use fid_gps::prof::zdef::{
    apply, zone_intervals, ConstantZone, NonUniformParams, Side, ZdefError, ZoneDefKind, ZoneDefinition, ZoneIntervals,
    ZoneParam,
};
use fid_gps::resolve::get_dmt_row;
use fid_gps::zone::shape::ZoneShape;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/prof/zdef")
}

fn load(name: &str) -> CValue {
    fid_canon::parse(&std::fs::read(dir().join(name)).unwrap()).unwrap()
}

fn table() -> DmtTable {
    DmtTable::load(&std::fs::read(dir().join("rows.json")).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("object"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("array"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("string"),
    }
}

fn bits(v: &CValue) -> u64 {
    u64::from_str_radix(st(v).strip_prefix("0x").unwrap(), 16).unwrap()
}

fn f(v: &CValue) -> f64 {
    f64::from_bits(bits(v))
}

fn int(v: &CValue) -> u64 {
    match v {
        CValue::Num(n) => *n as u64,
        _ => panic!("number"),
    }
}

fn hx(x: f64) -> String {
    format!("0x{:016x}", x.to_bits())
}

fn definition(d: &BTreeMap<String, CValue>) -> ZoneDefinition {
    let t = d.get("t").map_or(f64::NAN, f);
    match st(&d["kind"]) {
        "bilateral" => ZoneDefinition::Bilateral { t },
        "unilateral" => ZoneDefinition::Unilateral { t, side: Side::parse(st(&d["side"])).unwrap() },
        "unequal" => ZoneDefinition::Unequal { t, share: f(&d["share"]) },
        "non_uniform" => {
            ZoneDefinition::NonUniform(NonUniformParams { widths_m: arr(&d["widths"]).iter().map(f).collect() })
        }
        k => panic!("kind {k}"),
    }
}

fn row<'t>(t: &'t DmtTable, key: &str) -> &'t DmtRow {
    get_dmt_row(t, key).unwrap()
}

fn chain<'r>(r: &'r DmtRow, variant: &str) -> &'r OperatorChain {
    r.variant(variant).unwrap()
}

fn constant(z: &ZoneIntervals) -> &ConstantZone {
    match z {
        ZoneIntervals::Constant(c) => c,
        ZoneIntervals::PerPoint(_) => panic!("constant"),
    }
}

/// AC-1, AC-3, AC-4, AC-5 and AC-6: every case of `constant.json`, bit for bit.
#[test]
fn constant_cases_match_the_oracle() {
    let t = table();
    let doc = load("constant.json");
    let cases = arr(&obj(&doc)["cases"]);
    assert!(cases.len() >= 30);
    for c in cases {
        let c = obj(c);
        let name = st(&c["name"]);
        let def = definition(obj(&c["def"]));
        let r = row(&t, st(&c["row"]));
        let ch = chain(r, st(&c["variant"]));
        let exp = obj(&c["expect"]);
        let got = apply(&def, ch, r.limitations());
        if let Some(e) = exp.get("error") {
            let e = obj(e);
            let err = got.expect_err(name);
            assert_eq!(err.code(), st(&e["code"]), "{name}");
            match err {
                ZdefError::BadZoneParameter { name: p } => assert_eq!(p.name(), st(&e["name"]), "{name}"),
                ZdefError::Unsupported { kind } => assert_eq!(kind.code(), st(&e["kind"]), "{name}"),
                ZdefError::UnknownZoneLimitation { .. } => panic!("{name}: unexpected {err}"),
            }
            // No interval on any error path.
            assert!(zone_intervals(&def, ch, r.limitations()).is_err(), "{name}");
            continue;
        }
        let a = got.unwrap_or_else(|e| panic!("{name}: {e}"));
        let z = constant(&a.intervals);
        assert_eq!(hx(z.interval.lower_m), st(&exp["lower"]), "{name} lower");
        assert_eq!(hx(z.interval.upper_m), st(&exp["upper"]), "{name} upper");
        assert_eq!(hx(z.width_m), st(&exp["width"]), "{name} width");
        assert_eq!(hx(z.shift_m), st(&exp["shift"]), "{name} shift");
        // REQ-004: the shift is (lower + upper) * 0.5 bit for bit.
        assert_eq!(z.shift_m.to_bits(), ((z.interval.lower_m + z.interval.upper_m) * 0.5).to_bits(), "{name}");
        // REQ-007: the record fragment, compared as canonical JSON.
        let want = fid_canon::to_canonical_bytes(&exp["record"]).unwrap();
        let have = fid_canon::to_canonical_bytes(&a.record.to_cvalue()).unwrap();
        assert_eq!(String::from_utf8(have).unwrap(), String::from_utf8(want).unwrap(), "{name} record");
        assert_eq!(zone_intervals(&def, ch, r.limitations()).unwrap(), a.intervals, "{name}");
        // Membership at each bound and its neighbours, against the oracle's exact interval.
        let probes = arr(&exp["probes"]);
        assert_eq!(probes.len(), 6, "{name}");
        for pr in probes {
            let pr = obj(pr);
            let want = matches!(pr["inside"], CValue::Bool(true));
            assert_eq!(z.contains(f(&pr["d"])), Some(want), "{name} probe {}", st(&pr["d"]));
        }
    }
}

/// AC-1 in words: the five intervals of t = 0.4 mm, share 0.1 mm, as the nearest binary64.
#[test]
fn ac1_decimal_intervals() {
    let t = table();
    let surf = row(&t, "dmt:asme/profile_of_a_surface/none@1");
    let (tt, s) = (0.4e-3, 0.1e-3);
    let get = |def: ZoneDefinition, variant: &str| {
        let a = apply(&def, chain(surf, variant), surf.limitations()).unwrap();
        *constant(&a.intervals)
    };
    let z = get(ZoneDefinition::Bilateral { t: tt }, "share_outward");
    assert_eq!((z.interval.lower_m, z.interval.upper_m), (-0.2e-3, 0.2e-3));
    let z = get(ZoneDefinition::Unilateral { t: tt, side: Side::Outward }, "share_outward");
    assert_eq!((z.interval.lower_m, z.interval.upper_m), (0.0, 0.4e-3));
    let z = get(ZoneDefinition::Unilateral { t: tt, side: Side::Inward }, "share_outward");
    assert_eq!((z.interval.lower_m, z.interval.upper_m), (-0.4e-3, 0.0));
    let z = get(ZoneDefinition::Unequal { t: tt, share: s }, "share_outward");
    assert_eq!((z.interval.lower_m, z.interval.upper_m), (-(0.4e-3 - 0.1e-3), 0.1e-3));
    let z = get(ZoneDefinition::Unequal { t: tt, share: s }, "share_inward");
    assert_eq!((z.interval.lower_m, z.interval.upper_m), (-0.1e-3, 0.4e-3 - 0.1e-3));
    // The offset shape of GDT.ZONE.ZTYPE takes the shift.
    assert_eq!(z.offset_shape(), ZoneShape::Offset { disposition_shift: z.shift_m });
}

/// AC-3 and AC-4 in words.
#[test]
fn ac3_ac4_error_paths() {
    let t = table();
    let surf = row(&t, "dmt:asme/profile_of_a_surface/none@1");
    let line = row(&t, "dmt:asme/profile_of_a_line/none@1");
    let ch = chain(surf, "share_outward");
    let e = zone_intervals(&ZoneDefinition::Unequal { t: 0.4e-3, share: 0.5e-3 }, ch, surf.limitations());
    assert_eq!(e, Err(ZdefError::BadZoneParameter { name: ZoneParam::Share }));
    assert_eq!(e.unwrap_err().to_string(), "bad_zone_parameter{share}");
    let e = zone_intervals(&ZoneDefinition::Bilateral { t: f64::NAN }, ch, surf.limitations());
    assert_eq!(e, Err(ZdefError::BadZoneParameter { name: ZoneParam::T }));
    let ch = chain(line, "share_outward");
    let e = zone_intervals(&ZoneDefinition::Unequal { t: 0.4e-3, share: 0.1e-3 }, ch, line.limitations());
    assert_eq!(e, Err(ZdefError::Unsupported { kind: ZoneDefKind::Unequal }));
    assert_eq!(e.unwrap_err().to_string(), "UNSUPPORTED{unequal}");
    // The limitation is read from whatever list the caller passes (a resolved row's).
    let lim = vec!["zone_def_unsupported:bilateral".to_owned()];
    let e = zone_intervals(&ZoneDefinition::Bilateral { t: 0.4e-3 }, chain(surf, "share_outward"), &lim);
    assert_eq!(e, Err(ZdefError::Unsupported { kind: ZoneDefKind::Bilateral }));
    // An unknown or misspelt kind, or no kind, is refused (fail closed), for every
    // definition kind; other limitation codes are ignored.
    let uni = ZoneDefinition::Unilateral { t: 0.4e-3, side: Side::Inward };
    let ch = chain(surf, "share_outward");
    for (lim, index) in [
        (vec!["zone_def_unsupported:unilateralx"], 0),
        (vec!["pattern_size_limited", "zone_def_unsupported"], 1),
        (vec!["zone_def_unsupported:bilateral", "zone_def_unsupported:unequl"], 1),
    ] {
        let lim: Vec<String> = lim.into_iter().map(str::to_owned).collect();
        let e = zone_intervals(&uni, ch, &lim);
        assert_eq!(e, Err(ZdefError::UnknownZoneLimitation { index }), "{lim:?}");
        assert_eq!(e.unwrap_err().code(), "unknown_zone_limitation");
    }
    let lim = vec!["pattern_size_limited".to_owned(), "zone_def_unsupported:unequal".to_owned()];
    assert!(zone_intervals(&uni, ch, &lim).is_ok());
}

/// AC-5: non-uniform is UNSUPPORTED in this build, with no interval.
#[test]
fn ac5_non_uniform_unsupported() {
    let t = table();
    let surf = row(&t, "dmt:asme/profile_of_a_surface/none@1");
    let def = ZoneDefinition::NonUniform(NonUniformParams { widths_m: vec![0.4e-3, 0.8e-3] });
    let e = zone_intervals(&def, chain(surf, "share_outward"), surf.limitations());
    assert_eq!(e, Err(ZdefError::Unsupported { kind: ZoneDefKind::NonUniform }));
    assert_eq!(e.unwrap_err().to_string(), "UNSUPPORTED{non_uniform}");
}

/// Membership of deviations, including NaN (no membership).
#[test]
fn contains_is_the_closed_interval() {
    let t = table();
    let surf = row(&t, "dmt:asme/profile_of_a_surface/none@1");
    let a = apply(&ZoneDefinition::Unilateral { t: 0.4e-3, side: Side::Outward }, chain(surf, "share_outward"), &[])
        .unwrap();
    let z = constant(&a.intervals);
    assert_eq!(z.contains(0.0), Some(true));
    assert_eq!(z.contains(-0.0), Some(true));
    assert_eq!(z.contains(0.4e-3), Some(true));
    assert_eq!(z.contains(-f64::from_bits(1)), Some(false));
    assert_eq!(z.contains(0.4e-3_f64.next_up()), Some(false));
    assert_eq!(z.contains(f64::NAN), None);
}

/// GDT.PROF.DEV deviations: a signed one has membership; one without a sign (the edge of the
/// toleranced set, D-045) or without a projection has none, never `Some(true)`.
#[test]
fn deviation_membership_needs_a_sign() {
    let t = table();
    let surf = row(&t, "dmt:asme/profile_of_a_surface/none@1");
    let a = apply(&ZoneDefinition::Bilateral { t: 0.4e-3 }, chain(surf, "share_outward"), &[]).unwrap();
    let z = constant(&a.intervals);
    let proj = |signed_dev: Option<f64>, sign: SignStatus| Deviation {
        index: 0,
        outcome: DevOutcome::Projected(Projected {
            face_id: TopologyId::Imported(Label::new("f1").unwrap()),
            foot_point: [0.0, 0.0, 0.0],
            normal: None,
            distance: 0.1e-3,
            signed_dev,
            sign,
            approx_error: 0.0,
            on_boundary: true,
        }),
    };
    assert_eq!(z.contains_deviation(&proj(Some(0.1e-3), SignStatus::Outside)), Some(true));
    assert_eq!(z.contains_deviation(&proj(Some(-0.3e-3), SignStatus::Inside)), Some(false));
    assert_eq!(z.contains_deviation(&proj(None, SignStatus::Undefined(SignUndefined::Boundary))), None);
    assert_eq!(z.contains_deviation(&Deviation { index: 1, outcome: DevOutcome::NoProjection }), None);
}

/// 2^-53, exactly.
const TWO_M53: f64 = 1.0 / 9_007_199_254_740_992.0;

/// The AC-2 case stream; see the oracle's docstring (both must implement it identically).
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

fn step(mut x: f64, k: i64) -> f64 {
    for _ in 0..k.unsigned_abs() {
        x = if k > 0 { x.next_up() } else { x.next_down() };
    }
    x
}

/// AC-2: 10 000 seeded triples; `lower <= d <= upper` agrees with `|d - shift| <= width/2`
/// wherever both sides are exact, and every disagreement is listed by the oracle and lies
/// within 1 ulp of t of a bound. The digest pins every interval (AC-6).
#[test]
fn ac2_seeded_property() {
    let t = table();
    let surf = row(&t, "dmt:asme/profile_of_a_surface/none@1");
    let out = chain(surf, "share_outward");
    let inw = chain(surf, "share_inward");
    let doc = load("property.json");
    let doc = obj(&doc);
    assert_eq!(st(&doc["seed"]), "0x2def");
    let count = int(&doc["count"]);
    assert_eq!(count, 10_000);
    let mut rng = SplitMix(0x2def);
    let mut lines = String::new();
    let mut disagree: Vec<(u64, u64)> = Vec::new();
    let mut corrections = 0u64;
    for i in 0..count {
        let tt = (1000 + rng.next() % 9_999_001) as f64 * 1e-9;
        let kc = rng.next() % 5;
        let mut share = (tt * (rng.next() % 1001) as f64) / 1000.0;
        if share > tt {
            share = tt;
        }
        let r4 = rng.next();
        let (def, ch) = match kc {
            0 => (ZoneDefinition::Bilateral { t: tt }, out),
            1 => (ZoneDefinition::Unilateral { t: tt, side: Side::Outward }, out),
            2 => (ZoneDefinition::Unilateral { t: tt, side: Side::Inward }, out),
            3 => (ZoneDefinition::Unequal { t: tt, share }, out),
            _ => (ZoneDefinition::Unequal { t: tt, share }, inw),
        };
        let a = apply(&def, ch, surf.limitations()).unwrap();
        let z = *constant(&a.intervals);
        let (lo, hi) = (z.interval.lower_m, z.interval.upper_m);
        let d = if r4 % 4 < 2 {
            let u = (r4 >> 11) as f64 * TWO_M53;
            (u * 2.0 - 1.0) * tt
        } else {
            let r5 = rng.next();
            let bound = if r5 & 1 == 0 { lo } else { hi };
            step(bound, (r5 >> 1) as i64 % 7 - 3)
        };
        // A: the rounded interval; X: `contains`, the exact zone (checked by the digest).
        let in_a = lo <= d && d <= hi;
        let in_x = z.contains(d).unwrap();
        if in_x != in_a {
            corrections += 1;
        }
        let diff = d - z.shift_m;
        let hw = z.width_m * 0.5;
        let in_b = diff.abs() <= hw;
        // Exactness of the four operations by the error-free two-sum / halving checks.
        let exact = two_diff_exact(hi, lo, z.width_m)
            && two_sum_exact(lo, hi, (z.shift_m * 2.0, z.shift_m))
            && two_diff_exact(d, z.shift_m, diff)
            && hw * 2.0 == z.width_m;
        if exact {
            assert_eq!(in_a, in_b, "case {i}: exact case disagrees");
        } else if in_a != in_b {
            let ulp_t = tt.next_up() - tt;
            let (dl, dh) = ((d - lo).abs(), (d - hi).abs());
            let near = if dl.total_cmp(&dh).is_le() { dl } else { dh };
            assert!(near <= ulp_t, "case {i}: disagreement {near:e} beyond 1 ulp of t {ulp_t:e}");
            disagree.push((i, d.to_bits()));
        }
        lines.push_str(&format!(
            "{i} {kc} {} {} {} {} {} {} {} {} {} {} {}\n",
            hx(tt),
            hx(share),
            hx(d),
            hx(lo),
            hx(hi),
            hx(z.width_m),
            hx(z.shift_m),
            u8::from(in_a),
            u8::from(in_b),
            u8::from(exact),
            u8::from(in_x)
        ));
    }
    let digest = digest_bytes(lines.as_bytes());
    assert_eq!(
        digest.to_string(),
        format!("sha256:{}", st(&doc["sha256"])),
        "case stream or intervals differ from the oracle"
    );
    let listed: Vec<(u64, u64)> = arr(&doc["disagreements"])
        .iter()
        .map(|e| {
            let e = obj(e);
            (int(&e["index"]), bits(&e["d"]))
        })
        .collect();
    assert_eq!(disagree, listed);
    assert!(!listed.is_empty());
    // A versus the exact zone: the rounded interval differs only at a rounded bound.
    assert_eq!(corrections, int(&doc["exact_zone_corrections"]));
    assert!(corrections > 0);
}

/// `a - b == r` exactly (Knuth two-sum error term is zero).
fn two_diff_exact(a: f64, b: f64, r: f64) -> bool {
    let nb = -b;
    let bv = r - a;
    let av = r - bv;
    let err = (a - av) + (nb - bv);
    err == 0.0 && r.is_finite()
}

/// `(a + b) / 2 == shift` exactly: `a + b == twice` exactly and `twice / 2 == shift` exactly.
fn two_sum_exact(a: f64, b: f64, (twice, shift): (f64, f64)) -> bool {
    let s = a + b;
    let bv = s - a;
    let av = s - bv;
    let err = (a - av) + (b - bv);
    err == 0.0 && s == twice && twice * 0.5 == shift
}
