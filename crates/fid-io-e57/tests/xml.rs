//! Acceptance criteria of spec node IO.CLOUD.E57.XML (AC-1 .. AC-5) and the requirements
//! behind them. Fixture files and every expected outcome come from the independent stdlib
//! Python reference `fixtures/io/cloud/e57/xml/make_fixtures.py`; none is produced by this
//! crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_e57::xml::{
    parse_slice, parse_slice_with, read_section, scan_tree, write_tree, Bound, CartesianBounds, ColorLimits, E57Node,
    E57Tree, E57Xml, FieldDesc, FieldKind, Key, Namespace, Pose, Precision, Scalar, ScalarRange, ScanDesc, SectionRef,
    SphericalBounds, WriteError, XmlLimits, XmlSectionError, XmlText, XmlWarning, E57_NAMES, E57_NAMESPACE,
    FORMAT_NAME, MAX_DEPTH, MAX_ELEMENTS, MAX_TEXT_LEN, RETAINED_NODE_COST,
};
use fid_io_e57::{LogicalReader, LogicalWriter, PhysError};
use fid_io_reg::{Budget, LimitRequest, Principal};
use std::io::Cursor;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/cloud/e57/xml").join(name)
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap()
}

fn budget() -> Budget {
    Budget::new(Principal::Human, &LimitRequest::default()).unwrap()
}

fn parse(data: &[u8]) -> Result<E57Xml, XmlSectionError> {
    parse_slice(data, budget())
}

fn hx(v: f64) -> String {
    format!("{:016x}", v.to_bits())
}

fn opt_hx(v: Option<f64>) -> String {
    v.map_or("-".to_string(), hx)
}

fn opt_i(v: Option<i64>) -> String {
    v.map_or("-".to_string(), |v| v.to_string())
}

fn scalar(s: &Scalar) -> String {
    match s {
        Scalar::Integer(v) => format!("integer:{v}"),
        Scalar::Float(v) => format!("float:{}", hx(*v)),
        Scalar::ScaledInteger { raw, scale, offset } => format!("scaled:{raw}:{}:{}", opt_hx(*scale), opt_hx(*offset)),
    }
}

fn join(v: &[f64]) -> String {
    v.iter().map(|x| hx(*x)).collect::<Vec<_>>().join(",")
}

/// The summary line format of two_scans.expected.
fn summary(x: &E57Xml) -> String {
    let mut out = Vec::new();
    for (i, s) in x.scans().iter().enumerate() {
        let t = |o: &Option<XmlText>| o.as_ref().map_or("-".to_string(), |t| t.raw().to_string());
        out.push(format!(
            "scan {i} guid={} name={} description={} points={},{} codecs={}",
            s.guid.raw(),
            t(&s.name),
            t(&s.description),
            s.points.offset,
            s.points.records,
            s.codecs
        ));
        match &s.pose {
            Some(p) => out.push(format!("pose {i} q={} t={}", join(&p.q), join(&p.t))),
            None => out.push(format!("pose {i} none")),
        }
        for (j, f) in s.prototype.iter().enumerate() {
            let k = match f.kind {
                FieldKind::Float { precision, minimum, maximum } => format!(
                    "float precision={} min={} max={}",
                    precision.map_or("-", Precision::as_str),
                    opt_hx(minimum),
                    opt_hx(maximum)
                ),
                FieldKind::ScaledInteger { minimum, maximum, scale, offset } => format!(
                    "scaled min={} max={} scale={} offset={}",
                    opt_i(minimum),
                    opt_i(maximum),
                    opt_hx(scale),
                    opt_hx(offset)
                ),
                FieldKind::Integer { minimum, maximum } => {
                    format!("integer min={} max={}", opt_i(minimum), opt_i(maximum))
                }
                FieldKind::String => "string".to_string(),
            };
            out.push(format!("field {i} {j} {} {k}", f.name));
        }
        if let Some(b) = &s.cartesian_bounds {
            out.push(format!("cbounds {i} {}", join(&[b.x[0], b.x[1], b.y[0], b.y[1], b.z[0], b.z[1]])));
        }
        if let Some(b) = &s.spherical_bounds {
            let v = [b.range[0], b.range[1], b.elevation[0], b.elevation[1], b.azimuth[0], b.azimuth[1]];
            out.push(format!("sbounds {i} {}", join(&v)));
        }
        if let Some(r) = &s.intensity_limits {
            out.push(format!("ilimits {i} {} {}", scalar(&r.minimum), scalar(&r.maximum)));
        }
        if let Some(c) = &s.color_limits {
            let v: Vec<String> =
                [c.red, c.green, c.blue].iter().flat_map(|r| [scalar(&r.minimum), scalar(&r.maximum)]).collect();
            out.push(format!("climits {i} {}", v.join(" ")));
        }
    }
    out.push(format!("images2d {}", x.images2d().map_or("none".to_string(), |n| n.to_string())));
    out.push(format!("extensions {}", x.extension_elements()));
    out.push(format!("unknown {}", x.unknown_elements()));
    for w in x.warnings() {
        match w {
            XmlWarning::QuaternionNotUnit { scan, norm } => {
                out.push(format!("warning pose_quaternion_not_unit scan={scan} norm={}", hx(*norm)));
            }
            XmlWarning::NoScans => out.push("warning no_scans".to_string()),
        }
    }
    out.join("\n") + "\n"
}

/// The outcome format of cases.txt.
fn outcome(r: &Result<E57Xml, XmlSectionError>) -> String {
    match r {
        Ok(x) => {
            let mut s = format!("ok scans={}", x.scans().len());
            for w in x.warnings() {
                match w {
                    XmlWarning::QuaternionNotUnit { scan, norm } => {
                        s.push_str(&format!(" warning={}:{scan}:{}", w.code(), hx(*norm)));
                    }
                    XmlWarning::NoScans => s.push_str(&format!(" warning={}", w.code())),
                }
            }
            s
        }
        Err(XmlSectionError::Malformed { reason, .. }) => format!("malformed {reason}"),
        Err(XmlSectionError::Content { reason, scan: Some(i) }) => format!("malformed {reason} scan={i}"),
        Err(XmlSectionError::Content { reason, scan: None }) => format!("malformed {reason}"),
        Err(XmlSectionError::LimitExceeded { bound, .. }) => format!("limit_exceeded {bound}"),
        Err(e) => e.code().to_string(),
    }
}

// ---- AC-1 and REQ-001 ---------------------------------------------------------------------

#[test]
fn ac1_two_scans_in_document_order() {
    let x = parse(&read("two_scans.xml")).unwrap();
    let expected = String::from_utf8(read("two_scans.expected")).unwrap();
    assert_eq!(summary(&x), expected);
    assert_eq!(x.scans().len(), 2);
    // Untrusted strings stay data: the bidi override in the extension element is escaped for display.
    let note = x.tree().root.iter().find(|(k, _)| k.is("data3D")).map(|(_, n)| n).unwrap();
    let E57Node::Vector { children, .. } = note else { panic!("data3D") };
    let E57Node::Structure(ch) = &children[0] else { panic!("scan") };
    let (_, E57Node::String(t)) = ch.iter().find(|(k, _)| k.to_string() == "ext:vendorNote").unwrap() else {
        panic!("vendorNote")
    };
    assert_eq!(t.to_untrusted().as_display().to_string(), "untrusted \\u{202e}text");
    assert!(x.element_count() > 60);
}

// ---- AC-2, AC-3, REQ-002 .. REQ-004: hand-labelled error paths ----------------------------

#[test]
fn cases_match_their_labels() {
    let text = String::from_utf8(read("cases.txt")).unwrap();
    let mut n = 0;
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let (file, want) = line.split_once(' ').unwrap();
        let got = outcome(&parse(&read(file)));
        assert_eq!(got, want, "{file}");
        n += 1;
    }
    assert!(n >= 40, "{n} cases");
}

#[test]
fn ac2_doctype_is_forbidden() {
    let e = parse(&read("doctype.xml")).unwrap_err();
    assert_eq!(e.code(), "malformed");
    assert!(matches!(e, XmlSectionError::Malformed { reason: "xml_forbidden_construct", .. }));
}

/// The root header every section needs: `formatName` and `versionMajor` 1.
const HEAD: &str =
    "<formatName type=\"String\">ASTM E57 3D Imaging Data File</formatName><versionMajor type=\"Integer\">1</versionMajor>";

fn nested(levels: u32) -> Vec<u8> {
    // root (depth 1) + `levels - 1` nested structures
    let mut inner = String::new();
    for _ in 1..levels {
        inner = format!("<s type=\"Structure\">{inner}</s>");
    }
    format!(
        "<?xml version=\"1.0\"?><e57Root type=\"Structure\" xmlns=\"{E57_NAMESPACE}\">{HEAD}<data3D type=\"Vector\"/>{inner}</e57Root>"
    )
    .into_bytes()
}

#[test]
fn ac3_depth_65_is_refused_and_64_passes() {
    assert_eq!(MAX_DEPTH, 64);
    let e = parse(&nested(65)).unwrap_err();
    assert!(matches!(e, XmlSectionError::LimitExceeded { bound: Bound::Depth, value: 64, .. }), "{e:?}");
    assert!(parse(&nested(64)).is_ok());
    let low = XmlLimits::default().with_max_depth(10);
    assert!(parse_slice_with(&nested(10), budget(), low).is_ok());
    let e = parse_slice_with(&nested(11), budget(), low).unwrap_err();
    assert!(matches!(e, XmlSectionError::LimitExceeded { bound: Bound::Depth, value: 10, .. }), "{e:?}");
    // Limits are only lowered.
    assert_eq!(XmlLimits::default().with_max_depth(1000).max_depth(), 64);
}

fn flat(elements: u64) -> Vec<u8> {
    // root + the two header elements + data3D + `elements - 4` integers
    let mut s = format!("<e57Root type=\"Structure\" xmlns=\"{E57_NAMESPACE}\">{HEAD}<data3D type=\"Vector\">");
    for _ in 4..elements {
        s.push_str("<vectorChild type=\"Integer\"/>");
    }
    s.push_str("</data3D></e57Root>");
    s.into_bytes()
}

#[test]
fn req003_element_count_bound() {
    assert_eq!(MAX_ELEMENTS, 1_000_000);
    assert_eq!(XmlLimits::default().with_max_elements(u64::MAX).max_elements(), MAX_ELEMENTS);
    let low = XmlLimits::default().with_max_elements(100);
    // data3D holds integers, which is not a scan: the element bound fires before that check.
    let e = parse_slice_with(&flat(101), budget(), low).unwrap_err();
    assert!(matches!(e, XmlSectionError::LimitExceeded { bound: Bound::Elements, value: 100, .. }), "{e:?}");
    let e = parse_slice_with(&flat(100), budget(), low).unwrap_err();
    assert!(matches!(e, XmlSectionError::Content { reason: "scan_not_structure", scan: Some(0) }), "{e:?}");
}

fn string_doc(len: usize) -> Vec<u8> {
    let mut s = format!(
        "<e57Root type=\"Structure\" xmlns=\"{E57_NAMESPACE}\">{HEAD}<data3D type=\"Vector\"/><t type=\"String\">"
    );
    s.push_str(&"a".repeat(len));
    s.push_str("</t></e57Root>");
    s.into_bytes()
}

#[test]
fn req003_text_length_bound() {
    assert_eq!(MAX_TEXT_LEN, 1 << 20);
    assert!(parse(&string_doc(MAX_TEXT_LEN)).is_ok());
    let e = parse(&string_doc(MAX_TEXT_LEN + 1)).unwrap_err();
    assert!(matches!(e, XmlSectionError::LimitExceeded { bound: Bound::TextLength, .. }), "{e:?}");
    // A value split by a comment counts as one string.
    let low = XmlLimits::default().with_max_text_len(32);
    let doc = format!(
        "<e57Root type=\"Structure\" xmlns=\"{E57_NAMESPACE}\">{HEAD}<data3D type=\"Vector\"/><t type=\"String\">{}<!-- -->{}</t></e57Root>",
        "a".repeat(20),
        "b".repeat(20)
    );
    let e = parse_slice_with(doc.as_bytes(), budget(), low).unwrap_err();
    assert!(matches!(e, XmlSectionError::LimitExceeded { bound: Bound::TextLength, value: 32, .. }), "{e:?}");
}

#[test]
fn budgets_are_charged() {
    assert!(RETAINED_NODE_COST >= std::mem::size_of::<(Key, E57Node)>() as u64);
    let data = read("two_scans.xml");
    let small = Budget::new(Principal::Script, &LimitRequest::default().alloc(4096)).unwrap();
    let e = parse_slice(&data, small).unwrap_err();
    assert!(matches!(e, XmlSectionError::LimitExceeded { bound: Bound::Alloc, .. }), "{e:?}");
    let few = Budget::new(Principal::Script, &LimitRequest::default().bytes(100)).unwrap();
    let e = parse_slice(&data, few).unwrap_err();
    assert!(matches!(e, XmlSectionError::LimitExceeded { bound: Bound::Bytes, value: 100, .. }), "{e:?}");
}

#[test]
fn req004_quaternion_off_unit_is_passed_on_with_warning() {
    let x = parse(&read("two_scans.xml")).unwrap();
    let p = x.scans()[1].pose.unwrap();
    assert_eq!(p.q, [1.0, 0.0, 0.0, 0.002]);
    assert_eq!(x.warnings(), &[XmlWarning::QuaternionNotUnit { scan: 1, norm: p.quaternion_norm() }]);
    assert_eq!(x.warnings()[0].code(), "pose_quaternion_not_unit");
}

// ---- read_section over the physical layer -------------------------------------------------

fn e57_file(xml: &[u8], page_size: u64) -> Vec<u8> {
    let mut w = LogicalWriter::new(Cursor::new(Vec::new()), page_size).unwrap();
    w.append(&[7u8; 100]).unwrap();
    let at = w.physical_position().unwrap();
    w.append(xml).unwrap();
    let (cur, _) = w.finish(at, xml.len() as u64).unwrap();
    cur.into_inner()
}

#[test]
fn read_section_streams_the_verified_xml_section() {
    let xml = read("two_scans.xml");
    let file = e57_file(&xml, 256);
    let mut r = LogicalReader::open(Cursor::new(file.clone())).unwrap();
    let got = read_section(&mut r, budget()).unwrap();
    assert_eq!(got, parse(&xml).unwrap());
    // A damaged page inside the section is the physical layer's error.
    let mut bad = file;
    let at = bad.len() - 300;
    bad[at] ^= 0x40;
    let mut r = LogicalReader::open(Cursor::new(bad)).unwrap();
    let e = read_section(&mut r, budget()).unwrap_err();
    assert!(matches!(e, XmlSectionError::Phys(PhysError::ChecksumMismatch { .. })), "{e:?}");
    assert_eq!(e.code(), "checksum_mismatch");
    // A BOM at the start of the section is refused.
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(&xml);
    let mut r = LogicalReader::open(Cursor::new(e57_file(&bom, 256))).unwrap();
    let e = read_section(&mut r, budget()).unwrap_err();
    assert!(matches!(e, XmlSectionError::Malformed { reason: "xml_bom", .. }), "{e:?}");
}

// ---- AC-4 and REQ-005: the writer -----------------------------------------------------------

fn bits(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn opt_bits(s: &str) -> Option<f64> {
    (s != "-").then(|| bits(s))
}

fn opt_int(s: &str) -> Option<i64> {
    (s != "-").then(|| s.parse().unwrap())
}

fn key(s: &str) -> Key {
    match s.split_once(':') {
        Some((p, l)) => Key::prefixed(p, l),
        None => Key::new(s),
    }
}

fn scalars(t: &[&str]) -> Vec<Scalar> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        match t[i] {
            "integer" => {
                out.push(Scalar::Integer(t[i + 1].parse().unwrap()));
                i += 2;
            }
            "float" => {
                out.push(Scalar::Float(bits(t[i + 1])));
                i += 2;
            }
            "scaled" => {
                out.push(Scalar::ScaledInteger {
                    raw: t[i + 1].parse().unwrap(),
                    scale: opt_bits(t[i + 2]),
                    offset: opt_bits(t[i + 3]),
                });
                i += 4;
            }
            other => panic!("scalar kind {other}"),
        }
    }
    out
}

fn range(v: &[Scalar]) -> ScalarRange {
    ScalarRange { minimum: v[0], maximum: v[1] }
}

/// Parses writer_scans.txt (written by the Python reference).
fn writer_params() -> (XmlText, Vec<Namespace>, Vec<ScanDesc>) {
    let text = String::from_utf8(read("writer_scans.txt")).unwrap();
    let mut guid = None;
    let mut ns = Vec::new();
    let mut scans = Vec::new();
    let mut cur: Option<ScanDesc> = None;
    for line in text.lines() {
        let (k, rest) = line.split_once(' ').unwrap_or((line, ""));
        let t: Vec<&str> = rest.split(' ').collect();
        let s = cur.as_mut();
        match k {
            "file_guid" => guid = Some(XmlText::new(rest)),
            "ns" => ns.push(Namespace { prefix: t[0].to_string(), uri: XmlText::new(t[1]) }),
            "scan" => {
                cur = Some(ScanDesc {
                    guid: XmlText::new(""),
                    name: None,
                    description: None,
                    pose: None,
                    points: SectionRef { offset: 0, records: 0 },
                    prototype: Vec::new(),
                    codecs: 0,
                    cartesian_bounds: None,
                    spherical_bounds: None,
                    intensity_limits: None,
                    color_limits: None,
                })
            }
            "guid" => s.unwrap().guid = XmlText::new(rest),
            "name" => s.unwrap().name = Some(XmlText::new(rest)),
            "description" => s.unwrap().description = Some(XmlText::new(rest)),
            "pose" => {
                let v: Vec<f64> = t.iter().map(|x| bits(x)).collect();
                s.unwrap().pose = Some(Pose { q: [v[0], v[1], v[2], v[3]], t: [v[4], v[5], v[6]] });
            }
            "points" => {
                s.unwrap().points = SectionRef { offset: t[0].parse().unwrap(), records: t[1].parse().unwrap() }
            }
            "field" => {
                let kind = match t[1] {
                    "float" => FieldKind::Float {
                        precision: match t[2] {
                            "single" => Some(Precision::Single),
                            "double" => Some(Precision::Double),
                            _ => None,
                        },
                        minimum: opt_bits(t[3]),
                        maximum: opt_bits(t[4]),
                    },
                    "scaled" => FieldKind::ScaledInteger {
                        minimum: opt_int(t[2]),
                        maximum: opt_int(t[3]),
                        scale: opt_bits(t[4]),
                        offset: opt_bits(t[5]),
                    },
                    "integer" => FieldKind::Integer { minimum: opt_int(t[2]), maximum: opt_int(t[3]) },
                    "string" => FieldKind::String,
                    other => panic!("field kind {other}"),
                };
                s.unwrap().prototype.push(FieldDesc { name: key(t[0]), kind });
            }
            "cbounds" => {
                let v: Vec<f64> = t.iter().map(|x| bits(x)).collect();
                s.unwrap().cartesian_bounds =
                    Some(CartesianBounds { x: [v[0], v[1]], y: [v[2], v[3]], z: [v[4], v[5]] });
            }
            "sbounds" => {
                let v: Vec<f64> = t.iter().map(|x| bits(x)).collect();
                s.unwrap().spherical_bounds =
                    Some(SphericalBounds { range: [v[0], v[1]], elevation: [v[2], v[3]], azimuth: [v[4], v[5]] });
            }
            "ilimits" => s.unwrap().intensity_limits = Some(range(&scalars(&t))),
            "climits" => {
                let v = scalars(&t);
                s.unwrap().color_limits =
                    Some(ColorLimits { red: range(&v[0..2]), green: range(&v[2..4]), blue: range(&v[4..6]) });
            }
            "end" => scans.push(cur.take().unwrap()),
            other => panic!("line {other}"),
        }
    }
    (guid.unwrap(), ns, scans)
}

#[test]
fn ac4_writer_three_scans_round_trip_and_bytes() {
    let (guid, ns, scans) = writer_params();
    assert_eq!(scans.len(), 3);
    let tree = scan_tree(&guid, &ns, &scans).unwrap();
    let bytes = write_tree(&tree).unwrap();
    assert_eq!(String::from_utf8(bytes.clone()).unwrap(), String::from_utf8(read("writer_three_scans.xml")).unwrap());
    // write -> parse -> equal tree and equal scans
    let back = parse(&bytes).unwrap();
    assert_eq!(back.tree(), &tree);
    assert_eq!(back.scans(), &scans[..]);
    assert!(back.warnings().is_empty());
    // two writes are byte-identical, and the parsed tree writes the same bytes
    assert_eq!(write_tree(&tree).unwrap(), bytes);
    assert_eq!(write_tree(back.tree()).unwrap(), bytes);
    // -0.0 and the subnormal survive bit for bit
    let b = back.scans()[0].cartesian_bounds.unwrap();
    assert_eq!(b.y[0].to_bits(), (-0.0f64).to_bits());
    assert_eq!(back.scans()[2].pose.unwrap().t[0].to_bits(), 1);
}

#[test]
fn parsed_fixture_trees_write_back_equal() {
    for name in ["two_scans.xml", "writer_three_scans.xml", "vector_heterogeneous_allowed.xml", "depth64.xml"] {
        let x = parse(&read(name)).unwrap();
        let bytes = write_tree(x.tree()).unwrap();
        let again = parse(&bytes).unwrap();
        assert_eq!(again.tree(), x.tree(), "{name}");
        assert_eq!(again.scans(), x.scans(), "{name}");
        assert_eq!(write_tree(again.tree()).unwrap(), bytes, "{name}");
    }
}

fn tree_with(node: E57Node) -> E57Tree {
    E57Tree {
        namespaces: Vec::new(),
        root: vec![
            (Key::new("formatName"), E57Node::String(XmlText::new(FORMAT_NAME))),
            (Key::new("versionMajor"), E57Node::Integer { value: Some(1), minimum: None, maximum: None }),
            (Key::new("data3D"), E57Node::Vector { allow_heterogeneous: None, children: Vec::new() }),
            (Key::new("x"), node),
        ],
    }
}

fn invalid(t: &E57Tree) -> &'static str {
    match write_tree(t) {
        Err(WriteError::Invalid { reason, .. }) => reason,
        other => panic!("{other:?}"),
    }
}

#[test]
fn writer_refuses_what_the_reader_refuses() {
    let int = |v, lo, hi| E57Node::Integer { value: v, minimum: lo, maximum: hi };
    assert_eq!(invalid(&tree_with(int(Some(11), Some(0), Some(10)))), "value_out_of_range");
    assert_eq!(invalid(&tree_with(int(None, Some(2), Some(1)))), "minimum_above_maximum");
    let f = |v| E57Node::Float { value: Some(v), precision: None, minimum: None, maximum: None };
    assert_eq!(invalid(&tree_with(f(f64::NAN))), "float_not_finite");
    assert_eq!(invalid(&tree_with(f(f64::INFINITY))), "float_not_finite");
    let single = E57Node::Float { value: Some(1e39), precision: Some(Precision::Single), minimum: None, maximum: None };
    assert_eq!(invalid(&tree_with(single)), "float_out_of_range");
    let scaled = E57Node::ScaledInteger { raw: Some(1), minimum: None, maximum: None, scale: Some(0.0), offset: None };
    assert_eq!(invalid(&tree_with(scaled)), "scale_zero");
    assert_eq!(invalid(&tree_with(E57Node::String(XmlText::new("a\u{85}b")))), "control_character");
    assert_eq!(invalid(&tree_with(E57Node::String(XmlText::new("a\u{0}b")))), "control_character");
    let het = E57Node::Vector { allow_heterogeneous: Some(false), children: vec![int(None, None, None), f(1.0)] };
    assert_eq!(invalid(&tree_with(het)), "vector_heterogeneous");
    let codecs = E57Node::CompressedVector {
        file_offset: 0,
        record_count: 0,
        prototype: Box::new(E57Node::Structure(Vec::new())),
        codecs: Some(Box::new(E57Node::Structure(Vec::new()))),
    };
    assert_eq!(invalid(&tree_with(codecs)), "codecs_not_vector");
    let mut dup = tree_with(int(Some(1), None, None));
    dup.root.push((Key::new("x"), int(Some(2), None, None)));
    assert_eq!(invalid(&dup), "duplicate_key");
    let mut bad_name = tree_with(int(Some(1), None, None));
    bad_name.root.push((Key::new("1x"), int(Some(2), None, None)));
    assert_eq!(invalid(&bad_name), "element_name");
    let mut undeclared = tree_with(int(Some(1), None, None));
    undeclared.root.push((Key::prefixed("q", "x"), int(Some(2), None, None)));
    assert_eq!(invalid(&undeclared), "element_namespace");
    for (prefix, uri, reason) in [
        ("xml", "urn:a", "namespace_prefix"),
        ("a:b", "urn:a", "namespace_prefix"),
        ("a", "", "namespace_uri"),
        ("a", E57_NAMESPACE, "namespace_uri"),
        ("a", "urn:\u{9f}", "control_character"),
    ] {
        let mut t = tree_with(int(Some(1), None, None));
        t.namespaces.push(Namespace { prefix: prefix.to_string(), uri: XmlText::new(uri) });
        assert_eq!(invalid(&t), reason, "{prefix} {uri}");
    }
    let mut twice = tree_with(int(Some(1), None, None));
    twice.namespaces.push(Namespace { prefix: "a".to_string(), uri: XmlText::new("urn:a") });
    twice.namespaces.push(Namespace { prefix: "b".to_string(), uri: XmlText::new("urn:a") });
    assert_eq!(invalid(&twice), "namespace_uri_duplicate");
    // depth: the root (1) and 63 nested structures (2 .. 64) are written and read back; one
    // more level is 65.
    let mut deep = E57Node::Structure(Vec::new());
    for _ in 0..62 {
        deep = E57Node::Structure(vec![(Key::new("s"), deep)]);
    }
    let ok = tree_with(deep.clone());
    assert_eq!(parse(&write_tree(&ok).unwrap()).unwrap().tree(), &ok);
    let deeper = E57Node::Structure(vec![(Key::new("s"), deep)]);
    assert_eq!(
        write_tree(&tree_with(deeper)).unwrap_err(),
        WriteError::LimitExceeded { bound: Bound::Depth, value: 64 }
    );
}

#[test]
fn builder_refuses_scans_it_cannot_write() {
    let (guid, ns, scans) = writer_params();
    let mut s = scans[0].clone();
    s.codecs = 1;
    assert_eq!(scan_tree(&guid, &ns, &[s]).unwrap_err(), WriteError::Scan { index: 0, reason: "codecs_not_writable" });
    let mut s = scans[0].clone();
    s.cartesian_bounds = Some(CartesianBounds { x: [1.0, 0.0], y: [0.0, 1.0], z: [0.0, 1.0] });
    assert_eq!(scan_tree(&guid, &ns, &[s]).unwrap_err(), WriteError::Scan { index: 0, reason: "bounds_inverted" });
    let mut s = scans[0].clone();
    s.prototype.clear();
    assert_eq!(scan_tree(&guid, &ns, &[s]).unwrap_err(), WriteError::Scan { index: 0, reason: "prototype_empty" });
    let mut s = scans[0].clone();
    s.pose = Some(Pose { q: [f64::NAN, 0.0, 0.0, 0.0], t: [0.0; 3] });
    assert_eq!(
        scan_tree(&guid, &ns, &[s]).unwrap_err(),
        WriteError::Scan { index: 0, reason: "scan_not_representable" }
    );
    // A prefixed prototype field needs its namespace.
    let t = scan_tree(&guid, &[], &scans[..1]).unwrap();
    assert!(matches!(write_tree(&t), Err(WriteError::Invalid { reason: "element_namespace", .. })));
}

// ---- AC-5: in-tree mutation fuzzing (the cargo-fuzz target is crates/fid-io-e57/fuzz) --------

struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const SNIPPETS: &[&[u8]] = &[
    b"<!DOCTYPE x>",
    b"&e;",
    b"&#x85;",
    b"<a type=\"Integer\">",
    b"</a>",
    b"type=\"Float\"",
    b"<vectorChild type=\"Structure\">",
    b"1e400",
    b"-",
    b"\"",
    b"<![CDATA[",
    b"xmlns:q=\"urn:q\"",
];

fn mutate(rng: &mut SplitMix, seed: &[u8]) -> Vec<u8> {
    let mut d = seed.to_vec();
    for _ in 0..1 + rng.below(4) {
        if d.is_empty() {
            break;
        }
        let at = rng.below(d.len());
        match rng.below(5) {
            0 => d[at] ^= 1 << rng.below(8),
            1 => {
                d.remove(at);
            }
            2 => d.truncate(at),
            3 => {
                let s = SNIPPETS[rng.below(SNIPPETS.len())];
                d.splice(at..at, s.iter().copied());
            }
            _ => {
                let len = rng.below(64).min(d.len() - at);
                let chunk: Vec<u8> = d[at..at + len].to_vec();
                let to = rng.below(d.len());
                d.splice(to..to, chunk);
            }
        }
    }
    d
}

#[test]
fn ac5_mutated_sections_never_panic_and_accepted_ones_round_trip() {
    let seeds = [read("two_scans.xml"), read("writer_three_scans.xml"), read("quaternion_unnormalised.xml")];
    let mut rng = SplitMix(0x05EE_DE57);
    let (mut ok, mut err) = (0u32, 0u32);
    for i in 0..3000 {
        let input = mutate(&mut rng, &seeds[i % seeds.len()]);
        match parse(&input) {
            Ok(x) => {
                ok += 1;
                let bytes = match write_tree(x.tree()) {
                    Ok(b) => b,
                    // Escaping may pass the size bound: a typed refusal, never bad bytes.
                    Err(WriteError::LimitExceeded { .. }) => continue,
                    Err(e) => panic!("case {i}: {e}"),
                };
                let back = parse(&bytes).unwrap_or_else(|e| panic!("case {i}: {e}"));
                assert_eq!(back.tree(), x.tree(), "case {i}");
                assert_eq!(back.scans(), x.scans(), "case {i}");
            }
            Err(e) => {
                err += 1;
                assert!(!e.code().is_empty());
                assert!(!e.to_string().is_empty());
            }
        }
    }
    assert!(ok > 0 && err > 0, "ok {ok} err {err}");
}

// ---- review follow-ups: minimal escaping, f32 rounding, root header, unknown names -----------

#[test]
fn minimal_escaping_keeps_large_strings_writable() {
    // About 300 KB of `>`: written as is (only `]]>` needs `&gt;`), so it stays under the bound.
    let gt = ">".repeat(300_000);
    let t = tree_with(E57Node::String(XmlText::new(&gt)));
    let bytes = write_tree(&t).unwrap();
    assert!(bytes.len() < 310_000, "{}", bytes.len());
    assert_eq!(parse(&bytes).unwrap().tree(), &t);
    // `]]>` is escaped, `]>` and `>]]` are not.
    let s = tree_with(E57Node::String(XmlText::new("a]]>b ]> >]] ]]]>")));
    let text = String::from_utf8(write_tree(&s).unwrap()).unwrap();
    assert!(text.contains(">a]]&gt;b ]> >]] ]]]&gt;</x>"), "{text}");
    assert_eq!(parse(text.as_bytes()).unwrap().tree(), &s);
}

#[test]
fn accepted_tree_past_the_size_bound_is_a_typed_refusal() {
    // 300,000 `&` in CDATA are 300 KB to read but 1.5 MB escaped: the writer refuses.
    let doc = format!(
        "<e57Root type=\"Structure\" xmlns=\"{E57_NAMESPACE}\">{HEAD}<data3D type=\"Vector\"/><t type=\"String\"><![CDATA[{}]]></t></e57Root>",
        "&".repeat(300_000)
    );
    let x = parse(doc.as_bytes()).unwrap();
    assert_eq!(
        write_tree(x.tree()).unwrap_err(),
        WriteError::LimitExceeded { bound: Bound::TextLength, value: MAX_TEXT_LEN as u64 }
    );
}

#[test]
fn namespace_names_pick_the_quote_they_do_not_contain() {
    let mut t = tree_with(E57Node::Integer { value: Some(1), minimum: None, maximum: None });
    t.namespaces.push(Namespace { prefix: "a".to_string(), uri: XmlText::new("urn:a\"b") });
    t.namespaces.push(Namespace { prefix: "b".to_string(), uri: XmlText::new("urn:c'd") });
    t.namespaces.push(Namespace { prefix: "c".to_string(), uri: XmlText::new("urn:e\"f'g&h") });
    let text = String::from_utf8(write_tree(&t).unwrap()).unwrap();
    assert!(text.contains(" xmlns:a='urn:a\"b'"), "{text}");
    assert!(text.contains(" xmlns:b=\"urn:c'd\""), "{text}");
    assert!(text.contains(" xmlns:c=\"urn:e&quot;f'g&amp;h\""), "{text}");
    assert_eq!(parse(text.as_bytes()).unwrap().tree(), &t);
}

#[test]
fn single_precision_is_rounds_to_a_finite_f32() {
    let single =
        |v| E57Node::Float { value: Some(v), precision: Some(Precision::Single), minimum: None, maximum: None };
    // 3.4028235e38 is above f32::MAX but rounds to it.
    let t = tree_with(single(3.4028235e38));
    assert_eq!(parse(&write_tree(&t).unwrap()).unwrap().tree(), &t);
    assert_eq!(invalid(&tree_with(single(3.4028236e38))), "float_out_of_range");
    assert_eq!(invalid(&tree_with(single(-3.4028236e38))), "float_out_of_range");
}

#[test]
fn root_header_is_required_by_reader_and_writer() {
    let mut t = tree_with(E57Node::Integer { value: Some(1), minimum: None, maximum: None });
    t.root.remove(0);
    assert_eq!(write_tree(&t).unwrap_err(), WriteError::Content { reason: "format_name_missing", scan: None });
    let mut t = tree_with(E57Node::Integer { value: Some(1), minimum: None, maximum: None });
    t.root[1].1 = E57Node::Integer { value: Some(2), minimum: None, maximum: None };
    assert_eq!(write_tree(&t).unwrap_err(), WriteError::Content { reason: "version_major_unsupported", scan: None });
    assert_eq!(write_tree(&t).unwrap_err().code(), "invalid_tree");
}

#[test]
fn unknown_e57_names_are_counted() {
    assert!(E57_NAMES.windows(2).all(|w| w[0] < w[1]), "sorted and unique");
    let x = parse(&read("two_scans.xml")).unwrap();
    assert_eq!(x.unknown_elements(), 1);
    // writer_three_scans.xml has one non-E57 prototype field name (`label`).
    assert_eq!(parse(&read("writer_three_scans.xml")).unwrap().unknown_elements(), 1);
}
