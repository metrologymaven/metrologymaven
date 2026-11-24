//! Acceptance criteria of spec node UX.ICONS.GRID (AC-1 .. AC-5), shipped `design/icons/STYLE.md`
//! (including linting its inline example SVGs), XML/entity hardening, SVG path parsing, geometry
//! checks and exit-2 error paths.
//! Expected results are hand-labelled in `fixtures/ux/icons/grid/expected.json` (written by
//! `make_fixtures.py` and cross-checked there by an independent Python oracle).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_design::icons_lint::{self, lint_svg_str, MAX_SVG_BYTES, REPORT_FORMAT};
use std::path::PathBuf;
use std::process::{Command, Output};

const FX: &str = "fixtures/ux/icons/grid";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(rel: &str) -> String {
    repo().join(FX).join(rel).to_string_lossy().into_owned()
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).current_dir(repo()).args(args).output().expect("run fid-design")
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn json_out(o: &Output) -> CValue {
    fid_canon::parse(stdout(o).trim_end().as_bytes()).unwrap()
}

fn expected() -> CValue {
    fid_canon::parse(&std::fs::read(fix("expected.json")).unwrap()).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(k).unwrap_or_else(|| panic!("missing key {k}")),
        _ => panic!("not an object at {k}"),
    }
}

fn strs(v: &CValue) -> Vec<String> {
    match v {
        CValue::Arr(a) => a
            .iter()
            .map(|x| match x {
                CValue::Str(s) => s.clone(),
                other => panic!("not a string: {other:?}"),
            })
            .collect(),
        other => panic!("not an array: {other:?}"),
    }
}

fn finding_keys(report: &CValue) -> Vec<String> {
    let CValue::Arr(a) = get(report, "findings") else { panic!("findings not an array") };
    let mut out = Vec::new();
    for f in a {
        let (CValue::Str(c), CValue::Str(s)) = (get(f, "code"), get(f, "subject")) else { panic!("bad finding") };
        let k = format!("{c} {s}");
        if out.last() != Some(&k) {
            out.push(k);
        }
    }
    out
}

fn run_case(key: &str) -> Output {
    let exp = expected();
    let e = get(&exp, key);
    let CValue::Str(root_rel) = get(e, "root") else { panic!("root") };
    let o = run(&["icons-lint", "--root", &fix(root_rel), "--json"]);
    let CValue::Num(code) = get(e, "exit") else { panic!("exit") };
    assert_eq!(f64::from(o.status.code().unwrap()), *code, "{key}: {}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(get(&r, "format"), &CValue::Str(REPORT_FORMAT.into()));
    assert_eq!(get(&r, "ok"), &CValue::Bool(*code == 0.0), "{key}");
    assert_eq!(strs(get(&r, "files")), strs(get(e, "files")), "{key}");
    assert_eq!(finding_keys(&r), strs(get(e, "findings")), "{key}");
    o
}

/// AC-1: `ok/` (two masters of one icon) exits 0 and is byte-identical across runs.
#[test]
fn ac1_ok_fixture_passes() {
    let a = run_case("ok");
    let b = run(&["icons-lint", "--root", &fix("ok"), "--json"]);
    assert_eq!(a.stdout, b.stdout);
    let t = run(&["icons-lint", "--root", &fix("ok")]);
    assert_eq!(t.status.code(), Some(0));
    assert!(stdout(&t).contains("ok: no findings"), "{}", stdout(&t));
}

/// AC-2: `has-text.svg` exits 1 with `ICON-ELEMENT text`.
#[test]
fn ac2_has_text_fails() {
    run_case("has-text");
    let t = run(&["icons-lint", "--root", &fix("has-text.svg")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("ICON-ELEMENT text"), "{}", stdout(&t));
}

/// AC-3: `literal-fill.svg` exits 1 with `ICON-COLOR-LITERAL`.
#[test]
fn ac3_literal_fill_fails() {
    run_case("literal-fill");
    let t = run(&["icons-lint", "--root", &fix("literal-fill.svg")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("ICON-COLOR-LITERAL"), "{}", stdout(&t));
}

/// AC-4: `off-grid.svg` exits 1 with `ICON-OFF-GRID`.
#[test]
fn ac4_off_grid_fails() {
    run_case("off-grid");
    let t = run(&["icons-lint", "--root", &fix("off-grid.svg")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("ICON-OFF-GRID"), "{}", stdout(&t));
}

/// AC-5: `missing-16/` exits 1 with `ICON-MASTER-MISSING 16`.
#[test]
fn ac5_missing_16_fails() {
    run_case("missing-16");
    let t = run(&["icons-lint", "--root", &fix("missing-16")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("ICON-MASTER-MISSING 16"), "{}", stdout(&t));
}

/// Additional labelled error paths: viewBox, forbidden attr, external ref.
#[test]
fn extra_error_fixtures() {
    run_case("bad-viewbox");
    run_case("bad-attr");
    run_case("external-ref");
}

/// Shipped `design/icons/STYLE.md` covers REQ-001..003 and its inline SVG examples pass icons-lint.
#[test]
fn shipped_style_guide_and_examples() {
    let style = std::fs::read_to_string(repo().join("design/icons/STYLE.md")).unwrap();
    for needle in [
        "{{FAMILY_NAME}}",
        "16",
        "24",
        "0.5 px",
        "1.5",
        "keyline",
        "corner radius",
        "optical padding",
        "redrawn",
        "currentColor",
        "data-role",
        "viewBox=\"0 0 16 16\"",
        "viewBox=\"0 0 24 24\"",
    ] {
        assert!(style.to_lowercase().contains(&needle.to_lowercase()), "missing {needle:?} in design/icons/STYLE.md");
    }

    // Extract every fenced ```svg block from STYLE.md and verify each passes icons-lint.
    let mut blocks = Vec::new();
    let mut in_svg = false;
    let mut cur = String::new();
    for line in style.lines() {
        if !in_svg && line.trim() == "```svg" {
            in_svg = true;
            cur.clear();
        } else if in_svg && line.trim() == "```" {
            in_svg = false;
            blocks.push(cur.clone());
        } else if in_svg {
            cur.push_str(line);
            cur.push('\n');
        }
    }
    assert!(blocks.len() >= 2, "STYLE.md must contain original 16 and 24 px SVG examples");
    for (i, svg) in blocks.iter().enumerate() {
        let mut f = Vec::new();
        lint_svg_str(&format!("style-example-{i}.svg"), svg, None, &mut f);
        assert!(f.is_empty(), "STYLE.md example {i} failed icons-lint: {f:?}");
    }

    // The 16 px example follows the guide's own 16 px rules: its ticks are 2 px long, and every
    // straight horizontal or vertical 1 px stroke runs on a `.5` centreline.
    let sixteen = blocks.iter().find(|b| b.contains("viewBox=\"0 0 16 16\"")).expect("16 px example");
    let half = |v: f64| (v - v.floor() - 0.5).abs() < 1e-9;
    let mut ticks = 0;
    for line in sixteen.lines().filter(|l| l.contains("<path")) {
        let d = line.split("d=\"").nth(1).and_then(|r| r.split('"').next()).expect("path d");
        for seg in d.split('M').filter(|x| !x.is_empty()) {
            let (head, axis, end) = if let Some((h, e)) = seg.split_once('V') {
                (h, 'V', e)
            } else if let Some((h, e)) = seg.split_once('H') {
                (h, 'H', e)
            } else {
                panic!("16 px example path segment {seg:?} is not a straight H/V stroke");
            };
            let nums: Vec<f64> = head.split_whitespace().map(|n| n.parse().unwrap()).collect();
            let (x, y, e): (f64, f64, f64) = (nums[0], nums[1], end.trim().parse().unwrap());
            let (centre, from) = if axis == 'V' { (x, y) } else { (y, x) };
            assert!(half(centre), "16 px stroke {seg:?} is not on a .5 centreline");
            assert!(((e - from).abs() - 2.0).abs() < 1e-9, "16 px tick {seg:?} is not 2 px long");
            ticks += 1;
        }
    }
    assert_eq!(ticks, 4, "16 px example has four cardinal ticks");
    for c in sixteen.lines().filter(|l| l.contains("<circle")) {
        for key in ["cx=\"", "cy=\""] {
            let v: f64 = c.split(key).nth(1).and_then(|r| r.split('"').next()).unwrap().parse().unwrap();
            assert!(half(v), "16 px circle centre {v} is not on a .5 centreline: {c}");
        }
    }

    // Shipped design/icons/ holds no SVG yet (STYLE.md is not an icon): an empty root is not a
    // pass but an `ICON-NO-SVG` finding with exit 2, since nothing was checked.
    let o = run(&["icons-lint", "--root", "design/icons", "--json"]);
    assert_eq!(o.status.code(), Some(2), "{}", stdout(&o));
    let report = json_out(&o);
    assert_eq!(finding_keys(&report), ["ICON-NO-SVG ."]);
    assert_eq!(get(&report, "ok"), &CValue::Bool(false));
}

/// A root with no SVG at all is a finding with exit 2, in JSON and text output alike; one SVG
/// (even a failing one) is checked normally.
#[test]
fn root_without_svgs_is_a_finding_with_exit_2() {
    let dir = std::env::temp_dir().join(format!("fid-design-icons-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("svg")).unwrap();
    std::fs::write(dir.join("README.md"), "no icons yet\n").unwrap();
    let root = dir.to_string_lossy().into_owned();

    let o = run(&["icons-lint", "--root", &root, "--json"]);
    assert_eq!(o.status.code(), Some(2), "{}", stdout(&o));
    assert_eq!(finding_keys(&json_out(&o)), ["ICON-NO-SVG ."]);
    let o = run(&["icons-lint", "--root", &root]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stdout(&o).contains("ICON-NO-SVG"), "{}", stdout(&o));

    // Upper-case extensions are SVGs too, and name their masters.
    std::fs::write(
        dir.join("svg").join("dot.16.SVG"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="currentColor"><rect x="7" y="7" width="2" height="2"/></svg>"#,
    )
    .unwrap();
    let o = run(&["icons-lint", "--root", &root, "--json"]);
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    assert_eq!(finding_keys(&json_out(&o)), ["ICON-MASTER-MISSING 24"]);

    let _ = std::fs::remove_dir_all(&dir);
}

/// Forbidden elements, DTD/entity expansion, external refs, transforms, stroke widths and paths.
#[test]
fn svg_profile_and_hardening_checks() {
    let lint = |svg: &str, master: Option<u32>| -> Vec<&'static str> {
        let mut f = Vec::new();
        lint_svg_str("t.svg", svg, master, &mut f);
        f.sort();
        f.dedup();
        f.into_iter().map(|x| x.code).collect()
    };

    // Valid SVG using all allowed shape elements and relative path commands on 0.5 grid.
    let all_ok = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <g data-role="sem.icon.primary">
            <rect x="2" y="2" width="20" height="20" rx="1.5"/>
            <circle cx="12" cy="12" r="4.5"/>
            <line x1="2.5" y1="12" x2="6.5" y2="12"/>
            <polyline points="4,4 8.5,4 8.5,8.5"/>
            <polygon points="15.5,15.5 19.5,15.5 17.5,19.5"/>
            <path d="M4 16l2.5 2.5h3v-2a1.5 1.5 0 011.5-1.5c.1.2.3.4 2 0Z"/>
        </g>
    </svg>"#;
    assert_eq!(lint(all_ok, Some(24)), Vec::<&str>::new());

    // Forbidden elements: image, script, style, filter, linearGradient, use, foreignObject.
    for tag in ["image", "script", "style", "filter", "linearGradient", "radialGradient", "use", "foreignObject"] {
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><{tag}/></svg>"#);
        let codes = lint(&svg, Some(16));
        assert!(codes.contains(&icons_lint::ELEMENT), "{tag}: {codes:?}");
    }

    // `<svg>` only as the root: a nested `<svg>` (its own viewport) is ICON-ELEMENT svg.
    let nested = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="currentColor"><svg viewBox="0 0 16 16"><rect x="2" y="2" width="12" height="12"/></svg></svg>"#;
    assert_eq!(lint(nested, Some(16)), [icons_lint::ELEMENT]);
    let mut f = Vec::new();
    lint_svg_str("t.svg", nested, Some(16), &mut f);
    assert!(f.iter().any(|x| x.subject == "svg" && x.detail.contains("nested")), "{f:?}");
    let deep =
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="currentColor"><g><g><svg/></g></g></svg>"#;
    assert_eq!(lint(deep, Some(16)), [icons_lint::ELEMENT]);

    // DTD and custom entity expansion refused with ICON-EXTERNAL-REF.
    let dtd = r#"<!DOCTYPE svg [<!ENTITY x "a">]><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"/>"#;
    assert_eq!(lint(dtd, Some(16)), [icons_lint::EXTERNAL_REF]);

    let ent = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g data-role="&evil;"/></svg>"#;
    assert_eq!(lint(ent, Some(16)), [icons_lint::EXTERNAL_REF]);

    let pi = r#"<?xml-stylesheet type="text/css" href="x.css"?><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"/>"#;
    assert_eq!(lint(pi, Some(16)), [icons_lint::EXTERNAL_REF]);

    // External URL inside fill or xlink:href.
    let url_fill = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect x="2" y="2" width="12" height="12" fill="url(#g)"/></svg>"#;
    assert_eq!(lint(url_fill, Some(16)), [icons_lint::EXTERNAL_REF]);

    // Transform, class, onclick rejected with ICON-ATTR.
    let tf = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g transform="translate(1,1)"/></svg>"#;
    assert_eq!(lint(tf, Some(16)), [icons_lint::ATTR]);

    // Wrong stroke-width for master size rejected with ICON-ATTR.
    let bad_sw = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1"><circle cx="12" cy="12" r="6"/></svg>"#;
    assert_eq!(lint(bad_sw, Some(24)), [icons_lint::ATTR]);

    // Relative path drift off the 0.5 grid caught.
    let rel_off = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1"><path d="M2 2l2.2 0"/></svg>"#;
    assert_eq!(lint(rel_off, Some(16)), [icons_lint::OFF_GRID]);

    // Wrong master viewBox for .16.svg file.
    let mismatch_vb = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"/>"#;
    assert_eq!(lint(mismatch_vb, Some(16)), [icons_lint::VIEWBOX]);
}

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-icons-lint-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Missing 24 master, oversized SVG (> 256 KiB), bad usage and unreadable inputs.
#[test]
fn missing_24_oversized_and_exit_2() {
    // Missing 24 master reports ICON-MASTER-MISSING 24.
    let d = temp("missing-24");
    std::fs::create_dir_all(d.join("svg")).unwrap();
    std::fs::copy(repo().join(FX).join("ok/svg/target.16.svg"), d.join("svg/target.16.svg")).unwrap();
    let r = icons_lint::check(&d).unwrap();
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].code, icons_lint::MASTER_MISSING);
    assert_eq!(r.findings[0].subject, "24");

    // SVG larger than 256 KiB exits 2 (bounded read).
    let big = temp("oversized").join("huge.16.svg");
    let pad = " ".repeat((MAX_SVG_BYTES as usize) + 64);
    std::fs::write(&big, format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><!--{pad}--></svg>"#))
        .unwrap();
    let o = run(&["icons-lint", "--root", big.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));

    // Bad usage and missing root exit 2.
    for args in [
        &["icons-lint"][..],
        &["icons-lint", "--root", "design/icons", "--all"],
        &["icons-lint", "--root", "design/icons", "--from", "x"],
        &["icons-lint", "--root", "no/such/path"],
    ] {
        assert_eq!(run(args).status.code(), Some(2), "{args:?}");
    }
}
