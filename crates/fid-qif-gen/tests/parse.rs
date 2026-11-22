//! Integration tests for `IO.QIF.GEN.PARSE` (AC-1..AC-6 and error paths).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_reg::LimitRequest;
use fid_qif_gen::parse::{parse_pinned, parse_pinned_with_limits, ParseError, RefKind};
use fid_qif_gen::pin::{PinError, PinnedSet};
use std::fs;
use std::path::PathBuf;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("io")
        .join("qif")
        .join("gen")
        .join("parse")
}

#[test]
fn ac1_constructs_matches_golden_ir_json() {
    let dir = fixtures().join("constructs");
    let pinned = PinnedSet::verify(&dir).expect("constructs manifest should verify");
    let ir = parse_pinned(&pinned).expect("constructs.xsd should parse");

    let counts = ir.top_level_counts();
    assert_eq!(counts.simple_types, 9);
    assert_eq!(counts.attributes, 1);
    assert_eq!(counts.attribute_groups, 1);
    assert_eq!(counts.groups, 1);
    assert_eq!(counts.complex_types, 6);
    assert_eq!(counts.elements, 4);
    assert_eq!(counts.total, 22);

    let actual = ir.to_canonical_json();
    let expected = fs::read(fixtures().join("constructs.ir.json")).expect("read constructs.ir.json");
    assert_eq!(
        String::from_utf8_lossy(&actual),
        String::from_utf8_lossy(&expected),
        "canonical IR JSON must match golden constructs.ir.json byte-for-byte"
    );

    // Determinism (REQ-IO.QIF.GEN.PARSE-006): two independent runs yield identical IR and bytes.
    let ir2 = parse_pinned(&pinned).expect("second run should parse");
    assert_eq!(ir, ir2);
    assert_eq!(actual, ir2.to_canonical_json());
}

#[test]
fn ac2_redefine_fails_xsd_construct_unsupported_with_file_and_line() {
    let dir = fixtures().join("redefine");
    let pinned = PinnedSet::verify(&dir).expect("redefine manifest should verify");
    let err = parse_pinned(&pinned).expect_err("xs:redefine must fail");

    assert_eq!(err.code(), "xsd_construct_unsupported");
    match err {
        ParseError::UnsupportedConstruct { construct, pos } => {
            assert_eq!(construct, "xs:redefine");
            assert_eq!(pos.file.as_str(), "redefine.xsd");
            assert_eq!(pos.line, 5);
            assert!(pos.col >= 1);
        }
        other => panic!("expected UnsupportedConstruct, got {other:?}"),
    }
}

#[test]
fn ac3_dangling_type_ref_fails_xsd_unresolved_ref() {
    let dir = fixtures().join("dangling");
    let pinned = PinnedSet::verify(&dir).expect("dangling manifest should verify");
    let err = parse_pinned(&pinned).expect_err("dangling type ref must fail");

    assert_eq!(err.code(), "xsd_unresolved_ref");
    match err {
        ParseError::UnresolvedRef { kind, name, pos } => {
            assert_eq!(kind, RefKind::Type);
            assert_eq!(name, "{urn:fid:test:dangling}Missing");
            assert_eq!(pos.file.as_str(), "dangling.xsd");
            assert_eq!(pos.line, 6);
        }
        other => panic!("expected UnresolvedRef, got {other:?}"),
    }
}

#[test]
fn ac4_mutual_include_cycle_processes_each_file_once() {
    let dir = fixtures().join("cycle");
    let pinned = PinnedSet::verify(&dir).expect("cycle manifest should verify");
    let ir = parse_pinned(&pinned).expect("mutual include cycle must succeed");

    let file_names: Vec<&str> = ir.files.iter().map(|f| f.as_str()).collect();
    assert_eq!(file_names, vec!["a.xsd", "b.xsd"]);
    let counts = ir.top_level_counts();
    assert_eq!(counts.complex_types, 2);
    assert_eq!(counts.elements, 1);
    assert_eq!(counts.total, 3);
}

#[test]
fn ac5_doctype_fails_xml_forbidden_construct() {
    let dir = fixtures().join("doctype");
    let pinned = PinnedSet::verify(&dir).expect("doctype manifest should verify");
    let err = parse_pinned(&pinned).expect_err("DOCTYPE must fail");

    assert_eq!(err.code(), "xml_forbidden_construct");
    match err {
        ParseError::ForbiddenXml { detail, pos } => {
            assert_eq!(detail, "doctype");
            assert_eq!(pos.file.as_str(), "doctype.xsd");
            assert_eq!(pos.line, 2);
        }
        other => panic!("expected ForbiddenXml, got {other:?}"),
    }
}

#[test]
fn ac6_multi_file_qif3_bundle_matches_independent_counts() {
    let dir = fixtures().join("qif3_bundle");
    let pinned = PinnedSet::verify(&dir).expect("qif3_bundle manifest should verify");
    let ir = parse_pinned(&pinned).expect("qif3_bundle must parse");

    let counts_json = fs::read_to_string(fixtures().join("qif3-counts.json")).expect("read qif3-counts.json");
    let actual = ir.top_level_counts();

    let expected_sub = format!(
        r#""counts":{{"attribute_groups":{},"attributes":{},"complex_types":{},"elements":{},"groups":{},"simple_types":{},"total":{}}}"#,
        actual.attribute_groups,
        actual.attributes,
        actual.complex_types,
        actual.elements,
        actual.groups,
        actual.simple_types,
        actual.total,
    );
    assert!(counts_json.contains(&expected_sub), "expected `{expected_sub}` in qif3-counts.json:\n{counts_json}");
    assert_eq!(
        ir.files.iter().map(|f| f.as_str()).collect::<Vec<_>>(),
        vec![
            "QIFDocument.xsd",
            "QIFLibrary/Characteristics.xsd",
            "QIFLibrary/Features.xsd",
            "QIFLibrary/Primitives.xsd",
            "QIFLibrary/Units.xsd",
        ]
    );
}

#[test]
fn unpinned_include_is_refused() {
    let dir = fixtures().join("unpinned_include");
    let pinned = PinnedSet::verify(&dir).expect("unpinned_include manifest should verify");
    let err = parse_pinned(&pinned).expect_err("unpinned include must fail");

    assert_eq!(err.code(), "xsd_unpinned_reference");
    assert!(matches!(err, ParseError::Pin(PinError::UnpinnedReference { .. })));
}

#[test]
fn tightened_byte_limit_fails_limit_exceeded() {
    let dir = fixtures().join("constructs");
    let pinned = PinnedSet::verify(&dir).expect("constructs manifest should verify");
    let tiny = LimitRequest::default().bytes(64);
    let err = parse_pinned_with_limits(&pinned, &tiny).expect_err("64-byte budget must fail");
    assert_eq!(err.code(), "limit_exceeded");
}

#[test]
fn unsupported_attribute_and_xs_any_are_refused() {
    let tmp = std::env::temp_dir().join(format!("fid_qif_gen_parse_unsup_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("create temp dir");

    let xsd = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" targetNamespace=\"urn:t\">\n",
        "  <xs:complexType name=\"Wild\">\n",
        "    <xs:sequence>\n",
        "      <xs:any/>\n",
        "    </xs:sequence>\n",
        "  </xs:complexType>\n",
        "</xs:schema>\n",
    );
    write_pinned_single(&tmp, "wild.xsd", xsd);
    let pinned = PinnedSet::verify(&tmp).expect("verify temp pin");
    let err = parse_pinned(&pinned).expect_err("xs:any must fail");
    assert_eq!(err.code(), "xsd_construct_unsupported");
    match err {
        ParseError::UnsupportedConstruct { construct, pos } => {
            assert_eq!(construct, "xs:any");
            assert_eq!(pos.file.as_str(), "wild.xsd");
            assert_eq!(pos.line, 5);
        }
        other => panic!("expected UnsupportedConstruct, got {other:?}"),
    }

    let bad_attr_xsd = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" targetNamespace=\"urn:t\">\n",
        "  <xs:complexType name=\"Bad\" block=\"restriction\"/>\n",
        "</xs:schema>\n",
    );
    write_pinned_single(&tmp, "wild.xsd", bad_attr_xsd);
    let pinned = PinnedSet::verify(&tmp).expect("verify temp pin 2");
    let err = parse_pinned(&pinned).expect_err("unknown attr must fail");
    assert_eq!(err.code(), "xsd_construct_unsupported");
    match err {
        ParseError::UnsupportedConstruct { construct, pos } => {
            assert_eq!(construct, "xs:complexType/@block");
            assert_eq!(pos.line, 3);
        }
        other => panic!("expected UnsupportedConstruct, got {other:?}"),
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn unresolved_group_attrgroup_keyref_and_substitution_group_fail_in_source_order() {
    let tmp = std::env::temp_dir().join(format!("fid_qif_gen_parse_refs_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("create temp dir");

    let cases: &[(&str, RefKind, &str)] = &[
        (
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:t=\"urn:t\" targetNamespace=\"urn:t\">\n",
                "  <xs:complexType name=\"C\"><xs:group ref=\"t:NoGroup\"/></xs:complexType>\n",
                "</xs:schema>\n",
            ),
            RefKind::Group,
            "{urn:t}NoGroup",
        ),
        (
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:t=\"urn:t\" targetNamespace=\"urn:t\">\n",
                "  <xs:complexType name=\"C\"><xs:attributeGroup ref=\"t:NoAG\"/></xs:complexType>\n",
                "</xs:schema>\n",
            ),
            RefKind::AttributeGroup,
            "{urn:t}NoAG",
        ),
        (
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:t=\"urn:t\" targetNamespace=\"urn:t\">\n",
                "  <xs:element name=\"Sub\" type=\"xs:string\" substitutionGroup=\"t:NoHead\"/>\n",
                "</xs:schema>\n",
            ),
            RefKind::Element,
            "{urn:t}NoHead",
        ),
        (
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:t=\"urn:t\" targetNamespace=\"urn:t\">\n",
                "  <xs:element name=\"Root\" type=\"xs:string\">\n",
                "    <xs:keyref name=\"K\" refer=\"t:NoKey\">\n",
                "      <xs:selector xpath=\"t:X\"/>\n",
                "      <xs:field xpath=\"@id\"/>\n",
                "    </xs:keyref>\n",
                "  </xs:element>\n",
                "</xs:schema>\n",
            ),
            RefKind::IdentityKey,
            "{urn:t}NoKey",
        ),
    ];

    for (xsd, expected_kind, expected_name) in cases {
        write_pinned_single(&tmp, "case.xsd", xsd);
        let pinned = PinnedSet::verify(&tmp).expect("verify temp pin");
        let err = parse_pinned(&pinned).expect_err("unresolved ref must fail");
        assert_eq!(err.code(), "xsd_unresolved_ref");
        match err {
            ParseError::UnresolvedRef { kind, name, .. } => {
                assert_eq!(kind, *expected_kind);
                assert_eq!(name, *expected_name);
            }
            other => panic!("expected UnresolvedRef, got {other:?}"),
        }
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn first_unresolved_ref_in_source_order_is_reported() {
    let tmp = std::env::temp_dir().join(format!("fid_qif_gen_parse_order_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("create temp dir");

    let xsd = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:t=\"urn:t\" targetNamespace=\"urn:t\">\n",
        "  <xs:element name=\"First\" type=\"t:MissingOne\"/>\n",
        "  <xs:element name=\"Second\" type=\"t:MissingTwo\"/>\n",
        "</xs:schema>\n",
    );
    write_pinned_single(&tmp, "order.xsd", xsd);
    let pinned = PinnedSet::verify(&tmp).expect("verify temp pin");
    let err = parse_pinned(&pinned).expect_err("must fail on first missing ref");
    assert_eq!(err.code(), "xsd_unresolved_ref");
    match err {
        ParseError::UnresolvedRef { kind, name, pos } => {
            assert_eq!(kind, RefKind::Type);
            assert_eq!(name, "{urn:t}MissingOne");
            assert_eq!(pos.line, 3);
        }
        other => panic!("expected UnresolvedRef, got {other:?}"),
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn malformed_and_xinclude_fail_with_stable_codes() {
    let tmp = std::env::temp_dir().join(format!("fid_qif_gen_parse_mal_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("create temp dir");

    let bad_occurs = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" targetNamespace=\"urn:t\">\n",
        "  <xs:complexType name=\"C\">\n",
        "    <xs:sequence>\n",
        "      <xs:element name=\"A\" type=\"xs:string\" minOccurs=\"3\" maxOccurs=\"2\"/>\n",
        "    </xs:sequence>\n",
        "  </xs:complexType>\n",
        "</xs:schema>\n",
    );
    write_pinned_single(&tmp, "bad.xsd", bad_occurs);
    let pinned = PinnedSet::verify(&tmp).expect("verify bad.xsd");
    let err = parse_pinned(&pinned).expect_err("minOccurs > maxOccurs must fail");
    assert_eq!(err.code(), "malformed");
    assert_eq!(err.pos().map(|p| p.line), Some(5));

    let xinclude = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:xi=\"http://www.w3.org/2001/XInclude\">\n",
        "  <xi:include href=\"other.xsd\"/>\n",
        "</xs:schema>\n",
    );
    write_pinned_single(&tmp, "bad.xsd", xinclude);
    let pinned = PinnedSet::verify(&tmp).expect("verify xinclude");
    let err = parse_pinned(&pinned).expect_err("XInclude must fail");
    assert_eq!(err.code(), "xml_forbidden_construct");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
#[ignore = "AC-6 upstream DMSC QIF 3.0 XSDs not pinned yet (licence unverified); see xsd/qif3/README.md"]
fn ac6_real_qif3_set_parses() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("xsd").join("qif3");
    let pinned = PinnedSet::verify(&dir).expect("real QIF 3.0 pinned set must verify");
    let ir = parse_pinned(&pinned).expect("real QIF 3.0 pinned set must parse");
    assert!(ir.top_level_counts().total > 0);
}

fn write_pinned_single(dir: &std::path::Path, filename: &str, xsd: &str) {
    let lic = b"Synthetic test licence.\n";
    let xsd_bytes = xsd.as_bytes();
    fs::write(dir.join("LICENSE.txt"), lic).expect("write licence");
    fs::write(dir.join(filename), xsd_bytes).expect("write xsd");
    let manifest = format!(
        "manifest_version = 1\nlicence_path = \"LICENSE.txt\"\nlicence_sha256 = \"{}\"\n\n[source]\nurl = \"https://example.org/test.git\"\nrev = \"2222222222222222222222222222222222222222\"\n\n[[file]]\npath = \"{filename}\"\nbytes = {}\nsha256 = \"{}\"\nrole = \"xsd\"\n",
        sha256_hex(lic),
        xsd_bytes.len(),
        sha256_hex(xsd_bytes),
    );
    fs::write(dir.join("qif-xsd.manifest.toml"), manifest).expect("write manifest");
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::from("sha256:");
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}
