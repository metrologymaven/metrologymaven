//! Acceptance criteria of spec node QUAL.VAP.TPL (AC-3, AC-4, AC-5 and the library side of
//! AC-1/AC-2; the `cargo xtask qual vap check-templates` exits are in
//! `crates/fid-xtask/tests/qual_vap_tpl.rs`). Golden renders and template sets are written by
//! the independent stdlib reference `fixtures/qual/vap/tpl/make_cases.py`; every expected
//! finding below is hand-labelled from the spec.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_qual::vap::tpl::odf::{crc32, package, zip_stored, MIMETYPE};
use fid_qual::vap::tpl::render::{blocks, markdown, odt, Block};
use fid_qual::vap::tpl::{
    check_set, check_template, read_set_dir, Catalogue, FindingCode, Selection, Template, TplError, LICENCE_NOTICE,
    REQUIRED_DOCUMENTS,
};
use std::path::{Path, PathBuf};

const FIX: &str = "fixtures/qual/vap/tpl";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn catalogue() -> Catalogue {
    Catalogue::from_path(&root().join("vap/intended-uses.toml")).unwrap()
}

fn real(id: &str) -> Template {
    let bytes = std::fs::read(root().join(format!("vap/templates/{id}.json"))).unwrap();
    Template::from_json(id, &bytes, &catalogue()).unwrap()
}

fn selections() -> Vec<(String, Vec<String>)> {
    let text = std::fs::read(root().join(FIX).join("golden/selections.json")).unwrap();
    match fid_canon::parse(&text).unwrap() {
        fid_canon::CValue::Obj(m) => m
            .into_iter()
            .map(|(k, v)| match v {
                fid_canon::CValue::Arr(a) => (
                    k,
                    a.into_iter()
                        .map(|s| match s {
                            fid_canon::CValue::Str(s) => s,
                            other => panic!("{other:?}"),
                        })
                        .collect(),
                ),
                other => panic!("{other:?}"),
            })
            .collect(),
        other => panic!("{other:?}"),
    }
}

/// A minimal valid template as JSON text, with `extra` spliced into the first field.
fn minimal(id: &str, field: &str) -> String {
    format!(
        r#"{{"format":"vap-template/1","id":"{id}","title":"T","version":"1.0.0","interpretation":"provisional",
"licence":{{"spdx":"CC-BY-4.0","notice":"{LICENCE_NOTICE}"}},"refs":[],
"sections":[{{"id":"s","heading":"H","fields":[{field}]}}]}}"#
    )
}

fn schema_detail(json: &str) -> String {
    let (t, f) = check_template("x", json.as_bytes(), &catalogue());
    assert!(t.is_none(), "expected a schema finding for {json}");
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].code, FindingCode::Schema, "{f:?}");
    assert_eq!(f[0].subject, "x");
    f[0].detail.clone()
}

// ------------------------------------------------------------------ AC-3 and AC-5

/// AC-3: `urs` with `[transcription_reports]` equals its golden Markdown and ODF files.
#[test]
fn ac3_urs_golden() {
    let sel = Selection::new(&catalogue(), &["transcription_reports"]).unwrap();
    let t = real("urs");
    let md = std::fs::read(root().join(FIX).join("golden/urs.md")).unwrap();
    assert_eq!(markdown(&t, &sel).as_bytes(), md.as_slice());
    let odt_golden = std::fs::read(root().join(FIX).join("golden/urs.odt")).unwrap();
    assert_eq!(odt(&t, &sel).unwrap(), odt_golden);
}

/// AC-3: only rows (and fields) whose rule names `transcription_reports` are pre-filled.
/// Expected rows are hand-labelled from vap/templates/urs.json.
#[test]
fn ac3_only_matching_rows_prefilled() {
    let sel = Selection::new(&catalogue(), &["transcription_reports"]).unwrap();
    let mut prefilled_rows = Vec::new();
    let mut prefilled_fields = Vec::new();
    for b in blocks(&real("urs"), &sel) {
        match b {
            Block::Table { label: Some(l), headers, rows } if l == "Requirements" => {
                assert_eq!(headers.last().map(String::as_str), Some("Pre-filled for"));
                for r in rows {
                    let mark = r.last().unwrap();
                    let requirement = &r[2];
                    if mark.is_empty() {
                        assert!(requirement.is_empty(), "{r:?}");
                    } else {
                        assert_eq!(mark, "transcription_reports");
                        assert!(!requirement.is_empty());
                        prefilled_rows.push(r[0].clone());
                    }
                    // Customer columns are always blank.
                    assert!(r[3].is_empty() && r[4].is_empty(), "{r:?}");
                }
            }
            Block::Labelled { label, mark: Some(m), .. } => {
                assert_eq!(m, "transcription_reports");
                prefilled_fields.push(label);
            }
            _ => {}
        }
    }
    assert_eq!(prefilled_rows, ["URS-TR-01", "URS-TR-02", "URS-TR-03", "URS-DI-01"]);
    assert_eq!(prefilled_fields, ["Intended-use statement"]);
}

/// Every golden render of make_cases.py matches, for both formats.
#[test]
fn all_goldens_match() {
    let sels = selections();
    assert_eq!(sels.len(), 4);
    for (id, sel) in sels {
        let s = Selection::new(&catalogue(), &sel).unwrap();
        let t = real(&id);
        let md = std::fs::read(root().join(FIX).join(format!("golden/{id}.md"))).unwrap();
        assert_eq!(markdown(&t, &s), String::from_utf8(md).unwrap(), "{id}.md");
        let o = std::fs::read(root().join(FIX).join(format!("golden/{id}.odt"))).unwrap();
        assert_eq!(odt(&t, &s).unwrap(), o, "{id}.odt");
    }
}

/// AC-5 on the exact AC-3 pair: `urs` with `[transcription_reports]`, rendered twice in both
/// formats from freshly loaded inputs, is byte-identical.
#[test]
fn ac5_urs_transcription_twice() {
    let render = || {
        let c = catalogue();
        let sel = Selection::new(&c, &["transcription_reports"]).unwrap();
        let t = real("urs");
        (markdown(&t, &sel).into_bytes(), odt(&t, &sel).unwrap())
    };
    let first = render();
    let second = render();
    assert_eq!(first.0, second.0, "urs.md");
    assert_eq!(first.1, second.1, "urs.odt");
}

/// AC-5: rendering twice gives byte-identical files, and so does a reordered selection.
#[test]
fn ac5_deterministic() {
    let c = catalogue();
    let t = real("risk");
    let a = Selection::new(&c, &["scan_inspection", "prismatic_gdt"]).unwrap();
    let b = Selection::new(&c, &["prismatic_gdt", "scan_inspection"]).unwrap();
    assert_eq!(markdown(&t, &a), markdown(&t, &a));
    assert_eq!(markdown(&t, &a), markdown(&t, &b));
    assert_eq!(odt(&t, &a).unwrap(), odt(&t, &a).unwrap());
    assert_eq!(odt(&t, &a).unwrap(), odt(&t, &b).unwrap());
}

/// REQ-003: Markdown and ODF carry the same blocks in the same order (spot check: every
/// heading of the Markdown appears as a `text:h` in the same order in content.xml).
#[test]
fn md_and_odt_same_order() {
    let sel = Selection::new(&catalogue(), &["scan_inspection"]).unwrap();
    let t = real("pq-guidance");
    let md = markdown(&t, &sel);
    let xml = fid_qual::vap::tpl::render::odt_content_xml(&t, &sel);
    let md_heads: Vec<&str> =
        md.lines().filter_map(|l| l.strip_prefix("## ").or_else(|| l.strip_prefix("# "))).collect();
    let xml_heads: Vec<&str> = xml
        .lines()
        .filter_map(|l| l.strip_prefix("<text:h text:outline-level=\"").map(|r| &r[3..r.len() - "</text:h>".len()]))
        .collect();
    assert_eq!(md_heads, xml_heads);
    assert_eq!(md_heads.len(), 6);
}

/// Markdown escapes template text; ODF escapes XML specials.
#[test]
fn escaping() {
    let json = minimal("x", r#"{"id":"f","label":"a*b_c|d<e>&#[]`\\","kind":"text"}"#);
    let t = Template::from_json("x", json.as_bytes(), &catalogue()).unwrap();
    let sel = Selection::default();
    let md = markdown(&t, &sel);
    assert!(md.contains(r"**a\*b\_c\|d\<e\>\&\#\[\]\`\\:** (to be completed)"), "{md}");
    let xml = fid_qual::vap::tpl::render::odt_content_xml(&t, &sel);
    assert!(xml.contains("<text:p>a*b_c|d&lt;e&gt;&amp;#[]`\\: (to be completed)</text:p>"), "{xml}");
}

// ------------------------------------------------------------------ the ODF container

/// The standard CRC-32 check value (hand-labelled: CRC-32/ISO-HDLC of "123456789").
#[test]
fn crc32_check_value() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b""), 0);
}

/// `mimetype` is the first member, stored, with no extra field, and the timestamps are the
/// DOS epoch (hand-labelled from the ODF packaging rules).
#[test]
fn odt_layout() {
    let z = package("<x/>").unwrap();
    assert_eq!(&z[0..4], &[0x50, 0x4b, 0x03, 0x04]);
    assert_eq!(u16::from_le_bytes([z[8], z[9]]), 0, "method stored");
    assert_eq!(u16::from_le_bytes([z[10], z[11]]), 0, "time");
    assert_eq!(u16::from_le_bytes([z[12], z[13]]), 0x0021, "date 1980-01-01");
    assert_eq!(u16::from_le_bytes([z[26], z[27]]), 8, "name length");
    assert_eq!(u16::from_le_bytes([z[28], z[29]]), 0, "extra length");
    assert_eq!(&z[30..38], b"mimetype");
    assert_eq!(&z[38..38 + MIMETYPE.len()], MIMETYPE.as_bytes());
    let eocd = &z[z.len() - 22..];
    assert_eq!(&eocd[0..4], &[0x50, 0x4b, 0x05, 0x06]);
    assert_eq!(u16::from_le_bytes([eocd[10], eocd[11]]), 3, "three members");
}

/// Member names that could escape an extraction folder, or that are not printable ASCII, are
/// refused; plain relative names are accepted.
#[test]
fn zip_member_names() {
    assert!(zip_stored(&[("META-INF/manifest.xml", b"x")]).is_ok());
    for bad in ["", "../x", "a/../b", "/abs", "a\\b", "a//b", "./a", "a/", "caf\u{e9}", "a b", "a\nb"] {
        match zip_stored(&[(bad, b"x")]) {
            Err(e @ TplError::Limit(_)) => assert!(e.to_string().contains("unsafe ZIP member name"), "{e}"),
            other => panic!("{bad:?}: {other:?}"),
        }
    }
}

// ------------------------------------------------------------------ real templates

/// The templates this leaf owns pass every check.
#[test]
fn real_templates_valid() {
    let c = catalogue();
    for id in ["urs", "risk", "pq-guidance", "periodic-review"] {
        let bytes = std::fs::read(root().join(format!("vap/templates/{id}.json"))).unwrap();
        let (t, f) = check_template(id, &bytes, &c);
        assert!(f.is_empty(), "{id}: {f:?}");
        assert_eq!(t.unwrap().id(), id);
    }
}

/// The repository template directory holds the four templates of this leaf and the IQ and OQ
/// protocols of QUAL.VAP.PROTO; the other three required documents belong to QUAL.VAP.CHECK
/// and SUPQ and are reported missing until those leaves land.
#[test]
fn repository_set_reports_sibling_documents_missing() {
    let files = read_set_dir(&root().join("vap/templates")).unwrap();
    let report = check_set(&files, &catalogue());
    let missing: Vec<String> = report.findings.iter().map(|f| f.to_string()).collect();
    assert_eq!(
        missing,
        ["TPL-MISSING checklist-17025", "TPL-MISSING checklist-part11-annex11", "TPL-MISSING supplier-questionnaire"]
    );
    assert_eq!(report.valid, ["iq-protocol", "oq-protocol", "periodic-review", "pq-guidance", "risk", "urs"]);
}

// ------------------------------------------------------------------ AC-1, AC-2, AC-4 (library)

#[test]
fn ac1_set_ok() {
    let files = read_set_dir(&root().join(FIX).join("set-ok")).unwrap();
    let report = check_set(&files, &catalogue());
    assert!(report.is_ok(), "{:?}", report.findings);
    assert_eq!(report.valid.len(), REQUIRED_DOCUMENTS.len());
}

#[test]
fn ac2_set_no_supq() {
    let files = read_set_dir(&root().join(FIX).join("set-no-supq")).unwrap();
    let report = check_set(&files, &catalogue());
    let got: Vec<String> = report.findings.iter().map(|f| f.to_string()).collect();
    assert_eq!(got, ["TPL-MISSING supplier-questionnaire"]);
}

#[test]
fn ac4_set_no_licence() {
    let files = read_set_dir(&root().join(FIX).join("set-no-licence")).unwrap();
    let report = check_set(&files, &catalogue());
    let got: Vec<String> = report.findings.iter().map(|f| f.to_string()).collect();
    assert_eq!(got, ["TPL-LICENCE risk: no licence notice"]);
}

/// AC-4 variants: an altered notice, a wrong SPDX ID and a licence of the wrong type all fail
/// naming the template; a schema problem elsewhere is reported alongside, not instead.
#[test]
fn ac4_licence_variants() {
    let c = catalogue();
    let field = r#"{"id":"f","label":"L","kind":"text"}"#;
    let good = minimal("x", field);
    let altered = good.replace("provided you give attribution.", "no attribution needed.");
    let (_, f) = check_template("x", altered.as_bytes(), &c);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].to_string(), "TPL-LICENCE x: licence.notice differs from the project licence notice");
    let spdx = good.replace(r#""spdx":"CC-BY-4.0""#, r#""spdx":"CC0-1.0""#);
    let (_, f) = check_template("x", spdx.as_bytes(), &c);
    assert_eq!(f[0].to_string(), "TPL-LICENCE x: licence.spdx must be \"CC-BY-4.0\"");
    let both = good.replace(r#""spdx":"CC-BY-4.0""#, r#""spdx":"MIT""#).replace(r#""title":"T""#, r#""title":"""#);
    let (t, f) = check_template("x", both.as_bytes(), &c);
    assert!(t.is_none());
    let codes: Vec<FindingCode> = f.iter().map(|x| x.code).collect();
    assert_eq!(codes, [FindingCode::Schema, FindingCode::Licence]);
    match Template::from_json("x", both.as_bytes(), &c) {
        Err(TplError::Template(first)) => assert_eq!(first.code, FindingCode::Schema),
        other => panic!("{other:?}"),
    }
}

// ------------------------------------------------------------------ TPL-SCHEMA refusals

#[test]
fn schema_refusals() {
    let text = r#"{"id":"f","label":"L","kind":"text"}"#;
    let cases: Vec<(String, &str)> = vec![
        (minimal("x", r#"{"id":"f","label":"L","kind":"text","colour":"red"}"#), "has unknown key \"colour\""),
        (minimal("x", r#"{"id":"f","id":"g","label":"L","kind":"text"}"#), "not valid JSON (duplicate_key"),
        (minimal("y", text), "id \"y\" does not match the file name"),
        (minimal("x", r#"{"id":"F","label":"L","kind":"text"}"#), "is not a valid ID"),
        (minimal("x", r#"{"id":"f-","label":"L","kind":"text"}"#), "is not a valid ID"),
        (minimal("x", r#"{"id":"f","label":"L","kind":"memo"}"#), "is not text, choice, table or signature"),
        (
            minimal("x", r#"{"id":"f","label":"L","kind":"text","prefill":["laser_tracking"],"value":"v"}"#),
            "not in the intended-use catalogue",
        ),
        (
            minimal("x", r#"{"id":"f","label":"L","kind":"text","prefill":["scan_inspection"]}"#),
            "both prefill and value",
        ),
        (minimal("x", r#"{"id":"f","label":"L","kind":"text","value":"v"}"#), "both prefill and value"),
        (minimal("x", r#"{"id":"f","label":"L","kind":"text","prefill":[],"value":"v"}"#), "must name 1 to 16"),
        (
            minimal(
                "x",
                r#"{"id":"f","label":"L","kind":"text","prefill":["scan_inspection","scan_inspection"],"value":"v"}"#,
            ),
            "twice",
        ),
        (
            minimal("x", r#"{"id":"f","label":"L","kind":"signature","prefill":["scan_inspection"],"value":"v"}"#),
            "unknown key \"prefill\"",
        ),
        (minimal("x", r#"{"id":"f","label":"L","kind":"choice","options":["a"]}"#), "at least 2 options"),
        (minimal("x", r#"{"id":"f","label":"L","kind":"choice","options":["a","a"]}"#), "lists \"a\" twice"),
        (
            minimal(
                "x",
                r#"{"id":"f","label":"L","kind":"choice","options":["a","b"],"prefill":["scan_inspection"],"value":"c"}"#,
            ),
            "is not one of its options",
        ),
        (minimal("x", "{\"id\":\"f\",\"label\":\"L\\u0007\",\"kind\":\"text\"}"), "control or bidirectional"),
        (minimal("x", "{\"id\":\"f\",\"label\":\"L\\u202e\",\"kind\":\"text\"}"), "control or bidirectional"),
        (minimal("x", r#"{"id":"f","label":"a  b","kind":"text"}"#), "two spaces in a row"),
        (minimal("x", r#"{"id":"f","label":" L","kind":"text"}"#), "leading or trailing white space"),
        (minimal("x", r#"{"id":"f","label":"","kind":"text"}"#), "is empty"),
        (minimal("x", r#"{"id":"f","label":7,"kind":"text"}"#), "must be a string"),
        (minimal("x", &format!("{text},{text}")), "field ID \"f\" appears twice"),
        (minimal("x", text).replace(r#""interpretation":"provisional""#, r#""interpretation":"reviewed""#), "D-045"),
        (minimal("x", text).replace("vap-template/1", "vap-template/2"), "format must be"),
        (minimal("x", text).replace(r#""version":"1.0.0""#, r#""version":"1.0""#), "MAJOR.MINOR.PATCH"),
        (minimal("x", text).replace(r#""version":"1.0.0""#, r#""version":"01.0.0""#), "MAJOR.MINOR.PATCH"),
        (
            minimal("x", text).replace(r#""refs":[]"#, r#""refs":[{"designator":"ISO 1","edition":"2020"}]"#),
            "refs[0].clause is missing",
        ),
        (
            minimal("x", text).replace(
                r#""refs":[]"#,
                &format!(r#""refs":[{{"designator":"{}","edition":"1","clause":"1"}}]"#, "q".repeat(65)),
            ),
            "longer than 64",
        ),
        (minimal("x", text).replace(r#""refs":[],"#, ""), "refs is missing"),
        (
            minimal("x", text).replace(
                r#"[{"id":"s","heading":"H","fields":["#,
                r#"[{"id":"s","heading":"H","fields":[]},{"id":"s","heading":"H","fields":["#,
            ),
            "section ID \"s\" appears twice",
        ),
    ];
    for (json, want) in cases {
        let got = schema_detail(&json);
        assert!(got.contains(want), "want {want:?} in {got:?} for {json}");
    }
}

#[test]
fn schema_refusals_tables() {
    let table = |cols: &str, rows: &str| {
        minimal("x", &format!(r#"{{"id":"t","label":"T","kind":"table","columns":[{cols}],"rows":[{rows}]}}"#))
    };
    let cols = r#"{"id":"a","heading":"A","fill":"static"},{"id":"p","heading":"P","fill":"prefill"},{"id":"c","heading":"C","fill":"customer"}"#;
    let ok = table(
        cols,
        r#"{"id":"r1","prefill":["scan_inspection"],"cells":{"a":"1","p":"v"}},{"id":"r2","cells":{"a":"2"}}"#,
    );
    assert!(Template::from_json("x", ok.as_bytes(), &catalogue()).is_ok());
    let cases = [
        (table(cols, r#"{"id":"r1","cells":{"a":"1","c":"filled"}}"#), "fills customer column \"c\""),
        (table(cols, r#"{"id":"r1","cells":{"a":"1","p":"v"}}"#), "but the row has no prefill rule"),
        (table(cols, r#"{"id":"r1","prefill":["scan_inspection"],"cells":{"a":"1"}}"#), "lacks column \"p\""),
        (table(cols, r#"{"id":"r1","cells":{}}"#), "lacks column \"a\""),
        (table(cols, r#"{"id":"r1","cells":{"a":"1","z":"1"}}"#), "unknown column \"z\""),
        (table(cols, r#"{"id":"r1","cells":{"a":"1"}},{"id":"r1","cells":{"a":"2"}}"#), "row ID \"r1\" twice"),
        (
            table(r#"{"id":"a","heading":"A","fill":"static"},{"id":"a","heading":"B","fill":"static"}"#, ""),
            "column ID \"a\" twice",
        ),
        (table(r#"{"id":"a","heading":"A","fill":"maybe"}"#, ""), "is not static, prefill or customer"),
        (table("", ""), "columns is empty"),
        (
            table(
                r#"{"id":"a","heading":"A","fill":"static"}"#,
                r#"{"id":"r1","prefill":["scan_inspection"],"cells":{"a":"1"}}"#,
            ),
            "no pre-fill column",
        ),
    ];
    for (json, want) in cases {
        let got = schema_detail(&json);
        assert!(got.contains(want), "want {want:?} in {got:?}");
    }
}

/// Input limits: oversized and over-deep templates are refused before they are walked.
#[test]
fn schema_limits() {
    let big = format!("{{\"pad\":\"{}\"}}", "a".repeat(fid_qual::vap::tpl::MAX_TEMPLATE_BYTES));
    assert!(schema_detail(&big).contains("larger than"));
    let deep = format!("{}{}", "[".repeat(20), "]".repeat(20));
    assert!(schema_detail(&deep).contains("not valid JSON (input_limit"));
    assert!(schema_detail("[]").contains("template must be an object"));
}

// ------------------------------------------------------------------ catalogue and selection

#[test]
fn catalogue_refusals() {
    let head = "format = \"vap-intended-uses/1\"\n";
    let entry = |id: &str| format!("[[intended_use]]\nid = \"{id}\"\ntitle = \"T\"\nsummary = \"S\"\n");
    assert_eq!(Catalogue::from_toml(&format!("{head}{}", entry("a_b"))).unwrap().uses().len(), 1);
    let cases = [
        (format!("{head}{}{}", entry("a"), entry("a")), "listed twice"),
        (format!("{head}{}", entry("A")), "not valid"),
        (format!("format = \"x\"\n{}", entry("a")), "format must be"),
        (format!("{head}extra = 1\n{}", entry("a")), "unknown key \"extra\""),
        (format!("{head}{}colour = \"red\"\n", entry("a")), "unknown key \"colour\""),
        (format!("{head}{}id = \"b\"\n", entry("a")), ""),
        (head.to_string(), "array of tables"),
    ];
    for (text, want) in cases {
        match Catalogue::from_toml(&text) {
            Err(e @ TplError::Catalogue(_)) => {
                assert_eq!(e.code(), "TPL-CATALOGUE");
                assert!(e.to_string().contains(want), "want {want:?} in {e}");
            }
            other => panic!("{text}: {other:?}"),
        }
    }
}

#[test]
fn selection_refusals() {
    let c = catalogue();
    assert_eq!(
        Selection::new(&c, &["scan_inspection", "prismatic_gdt"]).unwrap().ids(),
        ["prismatic_gdt", "scan_inspection"]
    );
    for (ids, want) in
        [(vec!["laser"], "not in the catalogue"), (vec!["scan_inspection", "scan_inspection"], "selected twice")]
    {
        match Selection::new(&c, &ids) {
            Err(e @ TplError::Selection { .. }) => assert!(e.to_string().contains(want), "{e}"),
            other => panic!("{other:?}"),
        }
    }
}

/// A rule that names a use not selected leaves the field unfilled; the catalogue lists the
/// three launch-1 intended uses (hand-labelled from the parent node).
#[test]
fn catalogue_contents() {
    let c = catalogue();
    let ids: Vec<&str> = c.uses().iter().map(|u| u.id.as_str()).collect();
    assert_eq!(ids, ["prismatic_gdt", "scan_inspection", "transcription_reports"]);
    let md = markdown(&real("urs"), &Selection::default());
    assert!(!md.contains("pre-filled for:"), "{md}");
    assert!(md.contains("Pre-filled for intended uses: none"));
}

/// A symbolic link given as the template folder or as the catalogue is refused, even when its
/// target is valid.
#[cfg(unix)]
#[test]
fn symlinked_dir_and_catalogue_refused() {
    let tmp = std::env::temp_dir().join(format!("fid-qual-vap-tpl-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let dir_link = tmp.join("set");
    let cat_link = tmp.join("intended-uses.toml");
    std::os::unix::fs::symlink(root().join(FIX).join("set-ok"), &dir_link).unwrap();
    std::os::unix::fs::symlink(root().join("vap/intended-uses.toml"), &cat_link).unwrap();
    let dir_err = read_set_dir(&dir_link).unwrap_err();
    assert!(dir_err.contains("symbolic link"), "{dir_err}");
    match Catalogue::from_path(&cat_link) {
        Err(e @ TplError::Catalogue(_)) => assert!(e.to_string().contains("symbolic link"), "{e}"),
        other => panic!("{other:?}"),
    }
    // The targets themselves are fine.
    assert!(read_set_dir(&root().join(FIX).join("set-ok")).is_ok());
    assert!(Catalogue::from_path(&root().join("vap/intended-uses.toml")).is_ok());
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A file given as the template folder, or a folder given as the catalogue, is refused.
#[test]
fn wrong_kind_of_path_refused() {
    let file = root().join("vap/intended-uses.toml");
    assert!(read_set_dir(&file).unwrap_err().contains("not a directory"));
    match Catalogue::from_path(&root().join("vap/templates")) {
        Err(e @ TplError::Catalogue(_)) => assert!(e.to_string().contains("not a regular file"), "{e}"),
        other => panic!("{other:?}"),
    }
}
