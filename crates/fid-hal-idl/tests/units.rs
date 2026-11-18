//! Acceptance criteria of spec node HAL.IDL.UNITS (AC-1 .. AC-6). Expected values come from
//! `fixtures/hal/idl/units/make_fixtures.py` (an independent Python reference) or are
//! hand-labelled from the leaf's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_hal_idl::units::{canonical_json, parse_json};
use fid_hal_idl::{
    EncodingDescriptor, FrameGraph, FrameId, NumericType, Quantity, RawValue, SiUnit, TimeSource, Timestamp,
    UnitVector3, UnitsError,
};
use fid_project::Uuid;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hal/idl/units")
}

fn fixture(name: &str) -> CValue {
    parse(&std::fs::read(dir().join(name)).unwrap()).unwrap()
}

fn arr(v: CValue) -> Vec<CValue> {
    let CValue::Arr(a) = v else { panic!("array expected") };
    a
}

fn obj(v: CValue) -> BTreeMap<String, CValue> {
    let CValue::Obj(o) = v else { panic!("object expected") };
    o
}

fn s(v: &CValue) -> &str {
    let CValue::Str(s) = v else { panic!("string expected, got {v:?}") };
    s
}

fn hex64(text: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(text.strip_prefix("0x").unwrap(), 16).unwrap())
}

fn canon(v: &CValue) -> Vec<u8> {
    to_canonical_bytes(v).unwrap()
}

/// AC-1: every valid quantity round-trips to identical canonical JSON, every unit is
/// covered, and each bit string decodes to the decimal the Python reference wrote.
#[test]
fn ac1_quantities_round_trip() {
    let mut seen = Vec::new();
    for case in arr(fixture("quantities.json")) {
        let mut c = obj(case);
        let input = c.remove("quantity").unwrap();
        let q = Quantity::from_value(input.clone()).unwrap_or_else(|e| panic!("{input:?}: {e}"));
        assert_eq!(canon(&q.to_value()), canon(&input), "round trip of {input:?}");
        let CValue::Num(decimal) = c["decimal"] else { panic!("decimal") };
        let negative = c["negative"] == CValue::Bool(true);
        assert_eq!(q.value().abs(), decimal);
        assert_eq!(q.value().is_sign_negative(), negative);
        // Through bytes as well: parse_json -> from_value -> canonical_json.
        let bytes = canonical_json(&input).unwrap();
        let again = Quantity::from_value(parse_json(&bytes).unwrap()).unwrap();
        assert_eq!(canonical_json(&again.to_value()).unwrap(), bytes);
        seen.push(q.unit());
    }
    for u in SiUnit::ALL {
        assert!(seen.contains(&u), "no fixture in unit {u}");
    }
}

/// AC-2: `mm` fails with `unit_unsupported`, NaN with `quantity_not_finite`; further shape
/// errors carry their own codes.
#[test]
fn ac2_invalid_quantities() {
    let cases = arr(fixture("quantities_invalid.json"));
    let mut names = Vec::new();
    for case in cases {
        let mut c = obj(case);
        let name = s(&c["name"]).to_string();
        let expect = s(&c["expect"]).to_string();
        let err = Quantity::from_value(c.remove("input").unwrap()).unwrap_err();
        assert_eq!(err.code(), expect, "{name}: {err}");
        names.push(name);
    }
    assert!(names.iter().any(|n| n == "unit_mm") && names.iter().any(|n| n == "value_nan"));
    // The leaf's two error paths, spelled out.
    let mm = parse_json(br#"{"unit":"mm","value":"0x3ff0000000000000"}"#).unwrap();
    assert!(matches!(Quantity::from_value(mm), Err(UnitsError::UnitUnsupported(u)) if u == "mm"));
    let nan = parse_json(br#"{"unit":"m","value":"0x7ff8000000000000"}"#).unwrap();
    assert_eq!(Quantity::from_value(nan), Err(UnitsError::QuantityNotFinite));
    assert_eq!(Quantity::new(f64::NAN, SiUnit::Metre), Err(UnitsError::QuantityNotFinite));
}

fn descriptors() -> (Vec<(String, CValue, String)>, Vec<CValue>) {
    let mut f = obj(fixture("encodings.json"));
    let ds = arr(f.remove("descriptors").unwrap())
        .into_iter()
        .map(|d| {
            let mut d = obj(d);
            (s(&d["name"]).to_string(), d.remove("descriptor").unwrap(), s(&d["expect"]).to_string())
        })
        .collect();
    (ds, arr(f.remove("decodes").unwrap()))
}

/// AC-3: valid descriptors pass (and round-trip); `scale_zero` and `unit_px` fail with
/// `encoding_invalid`, as do the other labelled invalid descriptors.
#[test]
fn ac3_encodings() {
    let (ds, _) = descriptors();
    let mut checked = Vec::new();
    for (name, d, expect) in ds {
        match EncodingDescriptor::from_value(d.clone()) {
            Ok(e) => {
                assert_eq!(expect, "ok", "{name} passed");
                assert_eq!(canon(&e.to_value()), canon(&d), "{name} round trip");
            }
            Err(err) => assert_eq!(err.code(), expect, "{name}: {err}"),
        }
        checked.push(name);
    }
    for must in ["scale_zero", "unit_px"] {
        assert!(checked.iter().any(|n| n == must), "fixture {must} missing");
    }
}

fn raw(numeric: NumericType, text: &str) -> RawValue {
    match numeric {
        NumericType::I16 => RawValue::I16(text.parse().unwrap()),
        NumericType::I32 => RawValue::I32(text.parse().unwrap()),
        NumericType::I64 => RawValue::I64(text.parse().unwrap()),
        NumericType::U16 => RawValue::U16(text.parse().unwrap()),
        NumericType::U32 => RawValue::U32(text.parse().unwrap()),
        NumericType::F32 => {
            RawValue::F32(f32::from_bits(u32::from_str_radix(text.strip_prefix("0x").unwrap(), 16).unwrap()))
        }
        NumericType::F64 => RawValue::F64(hex64(text)),
    }
}

/// REQ-002: decoded = offset + scale × raw, bit-identical to the Python float64 reference.
#[test]
fn req002_decode_matches_python_reference() {
    let (ds, decodes) = descriptors();
    let by_name: BTreeMap<String, EncodingDescriptor> = ds
        .into_iter()
        .filter(|(_, _, e)| e == "ok")
        .map(|(n, d, _)| (n, EncodingDescriptor::from_value(d).unwrap()))
        .collect();
    assert!(decodes.len() >= 18);
    for case in decodes {
        let c = obj(case);
        let d = by_name[s(&c["descriptor"])];
        let r = raw(d.numeric(), s(&c["raw"]));
        let expect = s(&c["expect"]);
        let got = d.decode(r);
        if expect.starts_with("0x") {
            let q = got.unwrap_or_else(|e| panic!("{c:?}: {e}"));
            assert_eq!(q.value().to_bits(), hex64(expect).to_bits(), "{c:?}");
            assert_eq!(q.unit().as_str(), s(&c["unit"]));
        } else {
            assert_eq!(got.map_err(|e| e.code()), Err(expect), "{c:?}");
        }
    }
}

fn graph(name: &str) -> FrameGraph {
    let mut f = obj(fixture(name));
    FrameGraph::from_value(f.remove("graph").unwrap()).unwrap()
}

/// AC-4: sensor → mount → sensor fails with `frame_graph_invalid`; the arm chain passes.
#[test]
fn ac4_frame_graphs() {
    let cycle = graph("frames_cycle.json");
    assert_eq!(cycle.validate().map_err(|e| e.code()), Err("frame_graph_invalid"));
    let arm = graph("frames_arm.json");
    assert_eq!(arm.validate(), Ok(()));
    assert_eq!(arm.frames.len(), 5);
    assert_eq!(arm.edges.len(), 4);
    // Round trip of the wire form.
    let input = obj(fixture("frames_arm.json")).remove("graph").unwrap();
    assert_eq!(canon(&arm.to_value()), canon(&input));
}

/// REQ-005: further invalid graphs (two parents, repeated edge, self-edge, longer cycle,
/// undeclared frame, duplicate frame) all fail with `frame_graph_invalid`.
#[test]
fn req005_invalid_graphs() {
    let cases = arr(fixture("frames_invalid.json"));
    assert_eq!(cases.len(), 6);
    for case in cases {
        let mut c = obj(case);
        let name = s(&c["name"]).to_string();
        let g = FrameGraph::from_value(c.remove("graph").unwrap()).unwrap();
        assert_eq!(g.validate().map_err(|e| e.code()), Err("frame_graph_invalid"), "{name}");
    }
}

/// Determinism: shuffling frames and edges never changes the outcome or the message.
#[test]
fn frame_validation_is_order_independent() {
    let mut graphs = vec![graph("frames_arm.json"), graph("frames_cycle.json")];
    for case in arr(fixture("frames_invalid.json")) {
        graphs.push(FrameGraph::from_value(obj(case).remove("graph").unwrap()).unwrap());
    }
    for g in graphs {
        let want = g.validate();
        let n_f = g.frames.len();
        let n_e = g.edges.len();
        for k in 0..6 {
            let mut h = g.clone();
            h.frames.reverse();
            h.edges.reverse();
            if n_f > 0 {
                h.frames.rotate_left(k % n_f);
            }
            if n_e > 0 {
                h.edges.rotate_left(k % n_e);
            }
            assert_eq!(h.validate(), want);
        }
    }
}

/// FrameId: UUIDv5 under the session namespace, identical to Python's `uuid.uuid5`.
#[test]
fn frame_ids_match_python_uuid5() {
    let mut f = obj(fixture("frames_arm.json"));
    let ns = Uuid::parse(s(&f["session_namespace"])).unwrap();
    let names = obj(f.remove("names").unwrap());
    assert_eq!(names.len(), 9);
    for (name, id) in names {
        assert_eq!(FrameId::derive(&ns, &name).to_string(), s(&id), "{name}");
        assert_eq!(FrameId::parse(s(&id)).unwrap(), FrameId::derive(&ns, &name));
    }
    // Upper case, v4 and nil IDs are not frame IDs.
    let up = FrameId::derive(&ns, "sensor").to_string().to_uppercase();
    for bad in [up.as_str(), "6f1c2a3b-4d5e-4f60-8a71-92b3c4d5e6f7", "00000000-0000-0000-0000-000000000000"] {
        assert_eq!(FrameId::parse(bad).map_err(|e| e.code()), Err("bad_value"), "{bad}");
    }
}

/// AC-5: shuffled time sources sort to HW_TRIGGER < PTP < DEVICE_CLOCK < NTP < HOST_RECEIVE.
#[test]
fn ac5_time_source_order() {
    let cases = arr(fixture("time_sources.json"));
    assert!(cases.len() >= 6);
    for case in cases {
        let c = obj(case);
        let read = |k: &str| -> Vec<TimeSource> {
            arr(c[k].clone()).into_iter().map(|v| TimeSource::from_value(v).unwrap()).collect()
        };
        let mut got = read("shuffled");
        got.sort();
        assert_eq!(got, read("sorted"));
    }
    let names: Vec<&str> = TimeSource::ALL.iter().map(|t| t.as_str()).collect();
    assert_eq!(names, ["HW_TRIGGER", "PTP", "DEVICE_CLOCK", "NTP", "HOST_RECEIVE"]);
    assert!(TimeSource::HwTrigger < TimeSource::Ptp && TimeSource::Ntp < TimeSource::HostReceive);
    assert_eq!(TimeSource::parse("GPS").map_err(|e| e.code()), Err("bad_value"));
}

/// AC-6: a direction off unit length by more than 1e-9 fails with `direction_not_unit`.
/// The leaf's example `(1, 0, 1e-6)` has |norm − 1| ≈ 5e-13, inside REQ-006's tolerance, so
/// it passes; `(1, 0, 1e-4)` (≈ 5e-9) carries the error path (stated in the PR).
#[test]
fn ac6_directions() {
    for case in arr(fixture("directions.json")) {
        let mut c = obj(case);
        let name = s(&c["name"]).to_string();
        let expect = s(&c["expect"]).to_string();
        let input = c.remove("vector").unwrap();
        match UnitVector3::from_value(input.clone()) {
            Ok(v) => {
                assert_eq!(expect, "ok", "{name} passed");
                assert_eq!(canon(&v.to_value()), canon(&input));
            }
            Err(e) => assert_eq!(e.code(), expect, "{name}"),
        }
    }
    assert_eq!(UnitVector3::new(1.0, 0.0, 1e-4), Err(UnitsError::DirectionNotUnit));
    assert!(UnitVector3::new(1.0, 0.0, 1e-6).is_ok());
}

#[test]
fn timestamps_round_trip() {
    let a = parse_json(br#"{"t_dev":"-5","t_host":"1700000000123456789"}"#).unwrap();
    let t = Timestamp::from_value(a.clone()).unwrap();
    assert_eq!(t, Timestamp { t_dev: Some(-5), t_host: 1_700_000_000_123_456_789 });
    assert_eq!(canon(&t.to_value()), canon(&a));
    let b = parse_json(br#"{"t_host":"0"}"#).unwrap();
    assert_eq!(Timestamp::from_value(b).unwrap().t_dev, None);
    let bads: [&[u8]; 4] = [br#"{"t_host":0}"#, br#"{"t_host":"007"}"#, br#"{"t_dev":"1"}"#, br#"{"t_host":"1","t":"2"}"#];
    for bad in bads {
        assert!(Timestamp::from_value(parse_json(bad).unwrap()).is_err());
    }
}
