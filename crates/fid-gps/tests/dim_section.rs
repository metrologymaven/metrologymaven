//! Spec node GDT.DIM.SECT.SPAN (`fid_gps::dim::section`): 2D elements from section-curve spans.
//!
//! The cases, curves and expected values come from the independent reference
//! `fixtures/gdt/sect/make_span_fixtures.py` (planted geometry, the leaf's inclusive arc-length
//! rule evaluated in Python binary64, hand-labelled errors); the loader refusals below are
//! hand-labelled from the format rules in `dim::section::curve`.
//!
//! - AC-1: the fillet circles (r = 2 mm) and the straight edges of `rect_with_fillet.json`
//!   within 1e-12 m (`cases_match_the_reference`).
//! - AC-2: `circle_two_vertices` is `span_too_short{needed: 3, got: 2}`.
//! - AC-3: `end_beyond_length` is `span_out_of_range{end}`.
//! - AC-4: every element records the fixture's section ID and digest.
//! - AC-5: `output_bits_are_pinned` (a regression pin: the same bytes on every CI platform).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_geo_frame::FrameId;
use fid_gps::dim::section::curve::{content_bytes, FORMAT};
use fid_gps::dim::section::{
    extract_section_element, CurveSpan, ElementKind, Params2d, SectionCurveView, SectionElement, SectionPlane,
    SpanError,
};
use fid_gps::dmt::{AlgId, Operator, OperatorChain, OperatorKind};
use std::collections::BTreeMap;

const TOL: f64 = 1e-12;

fn dir() -> String {
    format!("{}/../../fixtures/gdt/sect", env!("CARGO_MANIFEST_DIR"))
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/{name}", dir())).unwrap()
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

fn hx(v: &CValue) -> f64 {
    f64::from_bits(u64::from_str_radix(st(v), 16).unwrap())
}

fn v2(v: &CValue) -> [f64; 2] {
    let a = arr(v);
    [hx(&a[0]), hx(&a[1])]
}

fn chain(steps: &[CValue]) -> OperatorChain {
    let ops = steps
        .iter()
        .map(|s| {
            let s = arr(s);
            Operator::new(OperatorKind::parse(st(&s[0])).unwrap(), AlgId::parse(st(&s[1])).unwrap(), BTreeMap::new())
                .unwrap()
        })
        .collect();
    OperatorChain::new("default", ops).unwrap()
}

fn kind(s: &str) -> ElementKind {
    match s {
        "line2d" => ElementKind::Line2d,
        "circle2d" => ElementKind::Circle2d,
        "point2d" => ElementKind::Point2d,
        _ => panic!("kind {s}"),
    }
}

fn reference() -> CValue {
    fid_canon::parse(&read("cases.json")).unwrap()
}

fn curve(name: &str) -> SectionCurveView {
    SectionCurveView::from_json(&read(name)).unwrap()
}

fn run_case(c: &BTreeMap<String, CValue>) -> Result<SectionElement, SpanError> {
    let view = curve(st(&c["curve"]));
    let sp = obj(&c["span"]);
    let span = CurveSpan {
        section_id: st(&sp["section_id"]).to_owned(),
        start: hx(&sp["start"]),
        end: hx(&sp["end"]),
        wrap: matches!(sp["wrap"], CValue::Bool(true)),
    };
    extract_section_element(&view, &span, kind(st(&c["kind"])), &chain(arr(&c["chain"])))
}

fn check_truth(id: &str, el: &SectionElement, truth: &BTreeMap<String, CValue>) {
    match *el.params() {
        Params2d::Circle2d { c, r } => {
            let ct = v2(&truth["c"]);
            assert!((c[0] - ct[0]).abs() <= TOL && (c[1] - ct[1]).abs() <= TOL, "{id}: centre {c:?} vs {ct:?}");
            assert!((r - hx(&truth["r"])).abs() <= TOL, "{id}: radius {r}");
        }
        Params2d::Line2d { p, d } => {
            let (pt, dt) = (v2(&truth["p"]), v2(&truth["d"]));
            let off = (p[0] - pt[0]) * dt[1] - (p[1] - pt[1]) * dt[0];
            assert!(off.abs() <= TOL, "{id}: line off the edge by {off}");
            assert!((d[0] * dt[1] - d[1] * dt[0]).abs() <= TOL, "{id}: direction {d:?}");
            assert!((d[0] * d[0] + d[1] * d[1] - 1.0).abs() <= 1e-15, "{id}: unit direction");
        }
        Params2d::Point2d { c } => {
            let ct = v2(&truth["c"]);
            assert!((c[0] - ct[0]).abs() <= TOL && (c[1] - ct[1]).abs() <= TOL, "{id}: point {c:?} vs {ct:?}");
        }
    }
}

/// AC-1 to AC-4 and the other cases of the reference.
#[test]
fn cases_match_the_reference() {
    let doc = reference();
    let curves = obj(&obj(&doc)["curves"]);
    let cases = arr(&obj(&doc)["cases"]);
    assert!(cases.len() >= 20);
    for c in cases {
        let c = obj(c);
        let id = st(&c["id"]);
        let expect = obj(&c["expect"]);
        let got = run_case(c);
        if let Some(e) = expect.get("error") {
            assert_eq!(got.map(|_| ()).unwrap_err().to_string(), st(e), "{id}");
            continue;
        }
        let el = got.unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(el.first_vertex().to_string(), number(&expect["first"]), "{id}: first vertex");
        assert_eq!(el.vertex_count().to_string(), number(&expect["count"]), "{id}: count");
        check_truth(id, &el, obj(&expect["truth"]));
        // AC-4: provenance from the fixture.
        let meta = obj(&curves[st(&c["curve"])]);
        assert_eq!(el.section_id(), st(&meta["section_id"]), "{id}");
        assert_eq!(el.section_digest().as_str(), st(&meta["digest"]), "{id}");
        assert_eq!(el.fit_alg(), st(&arr(&arr(&c["chain"])[0])[1]), "{id}");
        assert_eq!(el.kind().as_str(), st(&c["kind"]), "{id}");
        assert_eq!(el.span().start.to_bits(), hx(&obj(&c["span"])["start"]).to_bits(), "{id}");
    }
}

fn number(v: &CValue) -> String {
    match v {
        CValue::Num(x) => format!("{x}"),
        _ => panic!("number"),
    }
}

/// The arc lengths follow the pinned rule bit for bit (the reference evaluates it in binary64),
/// and the loaded digest is the reference's.
#[test]
fn length_and_digest_match_the_reference() {
    let doc = reference();
    for (name, meta) in obj(&obj(&doc)["curves"]) {
        let meta = obj(meta);
        let view = curve(name);
        assert_eq!(view.length().to_bits(), hx(&meta["length"]).to_bits(), "{name}");
        assert_eq!(view.digest().as_str(), st(&meta["digest"]), "{name}");
        assert_eq!(view.vertices().len().to_string(), number(&meta["vertices"]), "{name}");
        let arc = view.arc_lengths();
        assert_eq!(arc[0].to_bits(), 0);
        assert!(arc.windows(2).all(|w| w[0] < w[1]));
    }
}

/// The canonical document round-trips, and the constructor gives the same digest.
#[test]
fn canonical_round_trip() {
    for name in ["rect_with_fillet.json", "open_arc.json"] {
        let bytes = read(name);
        let view = curve(name);
        let canon = view.to_canonical_json().unwrap();
        assert_eq!(canon, bytes.strip_suffix(b"\n").unwrap(), "{name}");
        let again = SectionCurveView::new(
            view.section_id(),
            view.frame(),
            *view.plane(),
            view.vertices().to_vec(),
            view.closed(),
        )
        .unwrap();
        assert_eq!(again, view);
        let content = content_bytes(&view).unwrap();
        assert_eq!(view.digest().to_bytes(), fid_canon::digest_bytes(&content).0);
    }
}

fn rect_text() -> String {
    String::from_utf8(read("rect_with_fillet.json")).unwrap()
}

fn load(text: &str) -> String {
    SectionCurveView::from_json(text.as_bytes()).unwrap_err().to_string()
}

/// Loader refusals, hand-labelled from the format rules.
#[test]
fn loader_refusals() {
    let t = rect_text();
    assert_eq!(load(&format!("\u{feff}{t}")), "bad_section{bom}");
    assert_eq!(load(&t.replacen(FORMAT, "fid.scan.sect.curve/2", 1)), "unsupported_section_schema{version: 2}");
    assert_eq!(load(&t.replacen(FORMAT, "fid.scan.sect.def/1", 1)), "bad_section{format}");
    // The digest is checked last, against the recomputed content digest.
    let i = t.find("sha256:").unwrap() + 7;
    let flipped = if &t[i..=i] == "0" { "1" } else { "0" };
    let tampered = format!("{}{flipped}{}", &t[..i], &t[i + 1..]);
    assert_eq!(load(&tampered), "section_digest_mismatch");
    // A changed vertex keeps the stated digest, which then no longer matches.
    let moved =
        t.replacen("[\"3f847ae147ae147b\",\"0000000000000000\"]", "[\"3f847ae147ae147c\",\"0000000000000000\"]", 1);
    assert_ne!(moved, t);
    assert_eq!(load(&moved), "section_digest_mismatch");
    assert_eq!(load(&t.replacen("{\"closed\"", "{\"extra\":true,\"closed\"", 1)), "bad_section{unknown_field}");
    assert_eq!(load(&t.replacen("\"closed\":true,", "", 1)), "bad_section{missing_field}");
    assert_eq!(load(&t.replacen("\"closed\":true", "\"closed\":\"true\"", 1)), "bad_section{wrong_type}");
    assert_eq!(
        load(&t.replacen("{\"closed\":true", "{\"closed\":true,\"closed\":true", 1)),
        "bad_section{duplicate_key}"
    );
    assert_eq!(
        load(&t.replacen("5f2c0e8a9b1d4c3e8f7a6b5c4d3e2f10", "5F2C0E8A9B1D4C3E8F7A6B5C4D3E2F10", 1)),
        "bad_section{frame}"
    );
    assert_eq!(load(&t.replacen("\"sect:rect-fillet-1\"", "\"Sect\"", 1)), "bad_section{section_id}");
    // Negative zero has a second text for 0; refused (the first vertex's v).
    assert_eq!(
        load(&t.replacen(
            "[\"3f847ae147ae147b\",\"0000000000000000\"]",
            "[\"3f847ae147ae147b\",\"8000000000000000\"]",
            1
        )),
        "bad_section{negative_zero, index: 0}"
    );
    assert_eq!(
        load(&t.replacen(
            "[\"3f847ae147ae147b\",\"0000000000000000\"]",
            "[\"7ff0000000000000\",\"0000000000000000\"]",
            1
        )),
        "bad_section{coordinate, index: 0}"
    );
    // Upper-case hex and decimal numbers are not coordinate texts.
    assert_eq!(
        load(&t.replacen("[\"3f847ae147ae147b\",", "[\"3F847AE147AE147B\",", 1)),
        "bad_section{coordinate, index: 0}"
    );
    assert_eq!(load(&t.replacen("[\"3f847ae147ae147b\",", "[0.01,", 1)), "bad_section{wrong_type, index: 0}");
    // A repeated vertex is a zero-length segment (refused before the digest).
    let first = "[\"3f847ae147ae147b\",\"0000000000000000\"]";
    assert_eq!(load(&t.replacen(first, &format!("{first},{first}"), 1)), "bad_section{zero_length_segment, index: 1}");
    // Plane axes that are not orthonormal.
    assert_eq!(load(&t.replacen("\"3fe3333333333333\"", "\"3fe4000000000000\"", 1)), "bad_section{plane_axes}");
    assert_eq!(load("[]"), "bad_section{wrong_type}");
    assert_eq!(load("{\"format\":"), "bad_section{syntax}");
}

fn plane() -> SectionPlane {
    SectionPlane::new([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap()
}

/// Constructor refusals and normalisation.
#[test]
fn constructor_rules() {
    let f = FrameId::WORLD;
    let ok = |v: Vec<[f64; 2]>, closed| SectionCurveView::new("s1", f, plane(), v, closed);
    let e = |r: Result<SectionCurveView, SpanError>| r.unwrap_err().to_string();
    assert_eq!(e(ok(vec![[0.0, 0.0]], false)), "bad_section{vertex_count}");
    assert_eq!(e(ok(vec![[0.0, 0.0], [1.0, 0.0]], true)), "bad_section{vertex_count}");
    assert_eq!(e(ok(vec![[0.0, 0.0], [f64::NAN, 0.0]], false)), "bad_section{coordinate, index: 1}");
    assert_eq!(e(ok(vec![[0.0, 0.0], [2.0e4, 0.0]], false)), "bad_section{coordinate, index: 1}");
    assert_eq!(e(ok(vec![[0.0, 0.0], [1e-310, 0.0]], false)), "bad_section{coordinate, index: 1}");
    assert_eq!(e(ok(vec![[1.0, 0.0], [1.0, 0.0]], false)), "bad_section{zero_length_segment, index: 1}");
    // A segment below the rounding resolution of the running sum.
    assert_eq!(
        e(ok(vec![[0.0, 0.0], [1.0e3, 0.0], [1.0e3, 1.0e-14]], false)),
        "bad_section{arc_length_not_increasing, index: 2}"
    );
    // The closing segment of a closed curve.
    assert_eq!(
        e(ok(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]], true)),
        "bad_section{zero_length_segment, index: 0}"
    );
    assert_eq!(
        e(SectionCurveView::new("", f, plane(), vec![[0.0, 0.0], [1.0, 0.0]], false)),
        "bad_section{section_id}"
    );
    assert_eq!(
        SectionPlane::new([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 1e-6]).unwrap_err().to_string(),
        "bad_section{plane_axes}"
    );
    assert_eq!(
        SectionPlane::new([0.0, f64::INFINITY, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap_err().to_string(),
        "bad_section{plane_value}"
    );
    // -0.0 is stored as +0.0, so both give one curve and one digest.
    let a = ok(vec![[-0.0, 0.0], [1.0, -0.0]], false).unwrap();
    let b = ok(vec![[0.0, 0.0], [1.0, 0.0]], false).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.vertices()[0][0].to_bits(), 0);
    let p = SectionPlane::new([-0.0; 3], [1.0, -0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
    assert_eq!(p, plane());
    assert_eq!(p.normal(), [0.0, 0.0, 1.0]);
}

fn line_chain(alg: &str) -> OperatorChain {
    let op = Operator::new(OperatorKind::Association, AlgId::parse(alg).unwrap(), BTreeMap::new()).unwrap();
    OperatorChain::new("default", vec![op]).unwrap()
}

/// Span values: NaN and -0.0 are refused, an empty wrap selection is too short.
#[test]
fn span_value_rules() {
    let view = curve("rect_with_fillet.json");
    let ch = line_chain("fit.line2.ls@1.0.0");
    let span =
        |start: f64, end: f64, wrap: bool| CurveSpan { section_id: "sect:rect-fillet-1".to_owned(), start, end, wrap };
    let e = |s: CurveSpan| extract_section_element(&view, &s, ElementKind::Line2d, &ch).unwrap_err().to_string();
    assert_eq!(e(span(f64::NAN, 0.001, false)), "bad_span{not_finite}");
    assert_eq!(e(span(0.0, f64::INFINITY, false)), "bad_span{not_finite}");
    assert_eq!(e(span(-0.0, 0.001, false)), "bad_span{negative_zero}");
    // Between two vertices: nothing selected.
    let arc = view.arc_lengths();
    let (a, b) = (arc[5], arc[6]);
    let m1 = a + (b - a) / 4.0;
    let m2 = a + (b - a) / 2.0;
    assert_eq!(e(span(m1, m2, false)), "span_too_short{needed: 2, got: 0}");
    // A wrapping span past the last vertex that ends before vertex 0's successor.
    let last = arc[arc.len() - 1];
    let w = (last + view.length()) / 2.0;
    assert!(last < w && w < view.length());
    assert_eq!(e(span(w, 0.0, true)), "span_too_short{needed: 2, got: 1}");
    // A wrapping span from the length to 0 selects only vertex 0; the whole curve without
    // wrap selects every vertex once.
    let el = extract_section_element(
        &view,
        &span(view.length(), 0.0, true),
        ElementKind::Point2d,
        &line_chain("fit.point.ls@1.0.0"),
    )
    .unwrap();
    assert_eq!((el.first_vertex(), el.vertex_count()), (0, 1));
    let el = extract_section_element(
        &view,
        &span(0.0, view.length(), false),
        ElementKind::Point2d,
        &line_chain("fit.point.ls@1.0.0"),
    )
    .unwrap();
    assert_eq!((el.first_vertex(), el.vertex_count()), (0, view.vertices().len()));
    // An association step with parameters (a constraint) is refused, never run unconstrained.
    let mut params = BTreeMap::new();
    params.insert("radius".to_owned(), fid_gps::dmt::Param::F64Bits(0.002f64.to_bits()));
    let op = Operator::new(OperatorKind::Association, AlgId::parse("fit.circle2.ls@1.0.0").unwrap(), params).unwrap();
    let constrained = OperatorChain::new("default", vec![op]).unwrap();
    let (a, b) = (arc[160], arc[223]);
    assert_eq!(
        extract_section_element(&view, &span(a, b, false), ElementKind::Circle2d, &constrained)
            .unwrap_err()
            .to_string(),
        "association_params_unsupported{fit.circle2.ls@1.0.0}"
    );
    // A chain naming a non-fit algorithm for the kind.
    let bad = line_chain("syn.assoc.least_squares@0.1.0");
    assert_eq!(
        extract_section_element(&view, &span(0.0, 0.001, false), ElementKind::Line2d, &bad).unwrap_err().to_string(),
        "fit_kind_mismatch{kind: line2d, alg: syn.assoc.least_squares@0.1.0}"
    );
}

fn push_f64(out: &mut Vec<u8>, x: f64) {
    out.extend_from_slice(&x.to_bits().to_le_bytes());
}

/// Regression pin of AC-5: SHA-256 of every output field of every successful reference case.
/// Generated by this crate (not an oracle); CI checks it on Linux, Windows and macOS.
const AC5_PIN: &str = "sha256:d4203a9672dcbb9d9e576755a65af4209f5f21448b261f216d687041be4f9b9a";

#[test]
fn output_bits_are_pinned() {
    let doc = reference();
    let mut out = Vec::new();
    for c in arr(&obj(&doc)["cases"]) {
        let c = obj(c);
        let Ok(el) = run_case(c) else { continue };
        out.extend_from_slice(st(&c["id"]).as_bytes());
        match *el.params() {
            Params2d::Line2d { p, d } => [p[0], p[1], d[0], d[1]].iter().for_each(|&x| push_f64(&mut out, x)),
            Params2d::Circle2d { c, r } => [c[0], c[1], r].iter().for_each(|&x| push_f64(&mut out, x)),
            Params2d::Point2d { c } => [c[0], c[1]].iter().for_each(|&x| push_f64(&mut out, x)),
        }
        out.extend_from_slice(el.fit_digest().as_str().as_bytes());
        out.extend_from_slice(el.section_digest().as_str().as_bytes());
        out.extend_from_slice(&(el.first_vertex() as u64).to_le_bytes());
        out.extend_from_slice(&(el.vertex_count() as u64).to_le_bytes());
    }
    let got = fid_types::Digest::from_bytes(fid_canon::digest_bytes(&out).0);
    assert_eq!(got.as_str(), AC5_PIN, "AC-5 output pin");
}
