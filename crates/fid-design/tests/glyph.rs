//! Spec leaf UX.ICONS.CLASS: `fid-design glyph-distinct` on the shipped glyphs (AC-1) and on
//! the hand-labelled fixtures of `fixtures/ux/icons/class/` (AC-2, AC-3), checked against the
//! independent Python rasteriser `fixtures/ux/icons/class/glyph_oracle.py` (its output is
//! `expected.json`), plus the shape-family rules (REQ-003, REQ-004), raster and index errors and
//! usage errors. AC-4 (the human design review) is not testable here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_design::glyph::{check, rasterise, Threshold, BADGES, CLASSES, STATES};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FX: &str = "fixtures/ux/icons/class";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).current_dir(repo()).args(args).output().expect("run fid-design")
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(k).unwrap_or_else(|| panic!("missing key {k}")),
        _ => panic!("not an object at {k}"),
    }
}

fn num(v: &CValue) -> u64 {
    match v {
        CValue::Num(n) => *n as u64,
        other => panic!("not a number: {other:?}"),
    }
}

fn s(v: &CValue) -> String {
    match v {
        CValue::Str(s) => s.clone(),
        other => panic!("not a string: {other:?}"),
    }
}

/// `(findings as "CODE subject", groups as (id, glyphs, closest pair))` of one oracle root.
type Summary = (Vec<String>, Vec<(String, u64, Option<(String, String, u64, u64)>)>);

fn oracle(root: &str) -> Summary {
    let v = fid_canon::parse(&std::fs::read(repo().join(FX).join("expected.json")).unwrap()).unwrap();
    assert_eq!(s(get(&v, "threshold")), "0.20");
    let r = get(get(&v, "roots"), root);
    let CValue::Arr(fs) = get(r, "findings") else { panic!("findings") };
    let findings = fs
        .iter()
        .map(|f| match f {
            CValue::Arr(p) => format!("{} {}", s(&p[0]), s(&p[1])),
            other => panic!("{other:?}"),
        })
        .collect();
    let groups = ["class", "state", "prov"]
        .iter()
        .map(|g| {
            let x = get(get(r, "groups"), g);
            let min = match get(x, "min") {
                CValue::Null => None,
                m => Some((s(get(m, "a")), s(get(m, "b")), num(get(m, "symdiff")), num(get(m, "union")))),
            };
            (g.to_string(), num(get(x, "glyphs")), min)
        })
        .collect();
    (findings, groups)
}

fn tool(root: &Path) -> Summary {
    let r = check(root, &Threshold::default_value()).unwrap();
    let findings = r.findings.iter().map(|f| format!("{} {}", f.code, f.subject)).collect();
    let groups = r
        .groups
        .iter()
        .map(|g| {
            let min = g.min.as_ref().map(|c| (c.a.clone(), c.b.clone(), c.diff.symdiff, c.diff.union));
            (g.group.to_string(), g.glyphs as u64, min)
        })
        .collect();
    (findings, groups)
}

/// The Rust rasteriser and metric agree with the independent Python oracle on every root, pixel
/// count for pixel count.
#[test]
fn agrees_with_python_oracle() {
    for root in ["design/icons", "fixtures/ux/icons/class/twins", "fixtures/ux/icons/class/missing-class"] {
        assert_eq!(tool(&repo().join(root)), oracle(root), "{root}");
    }
}

/// AC-1: the shipped glyphs pass and the report gives the minimum pairwise difference per group.
#[test]
fn ac1_shipped_glyphs_pass() {
    let o = run(&["glyph-distinct", "--root", "design/icons", "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), String::from_utf8_lossy(&o.stderr));
    let v = fid_canon::parse(stdout(&o).trim_end().as_bytes()).unwrap();
    assert_eq!(s(get(&v, "format")), "fid-design-glyph-distinct@1");
    assert_eq!(get(&v, "ok"), &CValue::Bool(true));
    assert_eq!(s(get(&v, "threshold")), "0.20");
    let CValue::Arr(groups) = get(&v, "groups") else { panic!("groups") };
    let (_, want) = oracle("design/icons");
    assert_eq!(groups.len(), 3);
    for (g, (id, glyphs, min)) in groups.iter().zip(want) {
        assert_eq!(s(get(g, "group")), id);
        // U64 values are encoded as decimal strings in canonical JSON.
        assert_eq!(s(get(g, "glyphs")), glyphs.to_string());
        let (a, b, sd, un) = min.unwrap();
        let m = get(g, "min");
        assert_eq!((s(get(m, "a")), s(get(m, "b"))), (a, b));
        assert_eq!((s(get(m, "symdiff")), s(get(m, "union"))), (sd.to_string(), un.to_string()));
        // Every group clears the 0.20 threshold.
        assert!(sd * 5 >= un, "{id}: {sd}/{un}");
    }
    let text = stdout(&run(&["glyph-distinct", "--root", "design/icons"]));
    assert!(text.contains("class: 11 glyph(s), minimum difference 0.284090 (class.w3 class.m1)"), "{text}");
    assert!(text.contains("ok: no findings"), "{text}");
}

/// AC-2: two state glyphs differing by one pixel.
#[test]
fn ac2_twins() {
    let o = run(&["glyph-distinct", "--root", &format!("{FX}/twins")]);
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    assert!(stdout(&o).contains("GLYPH-SIMILAR state.indeterminate state.not-decidable"), "{}", stdout(&o));
    let (findings, _) = tool(&repo().join(FX).join("twins"));
    assert_eq!(findings, ["GLYPH-SIMILAR state.indeterminate state.not-decidable"]);
    let r = check(&repo().join(FX).join("twins"), &Threshold::default_value()).unwrap();
    let state = r.groups.iter().find(|g| g.group == "state").unwrap().min.clone().unwrap();
    assert_eq!((state.diff.symdiff, state.diff.dx, state.diff.dy), (1, 0, 0));
}

/// AC-3: no `class.i1`.
#[test]
fn ac3_missing_class() {
    let o = run(&["glyph-distinct", "--root", &format!("{FX}/missing-class"), "--json"]);
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    assert!(stdout(&o).contains("\"code\":\"GLYPH-MISSING\""), "{}", stdout(&o));
    let text = stdout(&run(&["glyph-distinct", "--root", &format!("{FX}/missing-class")]));
    assert!(text.contains("GLYPH-MISSING class.i1"), "{text}");
    let (findings, _) = tool(&repo().join(FX).join("missing-class"));
    assert_eq!(findings, ["GLYPH-MISSING class.i1"]);
}

/// REQ-001: every glyph and the stripe pattern ship as 16 and 24 px masters with an index entry
/// whose `a11y_id` is `icon.<name>`, labelless, with English text of at most 40 characters.
#[test]
fn shipped_set_is_complete() {
    let idx = fid_canon::parse(&std::fs::read(repo().join("design/icons/index.json")).unwrap()).unwrap();
    let CValue::Arr(icons) = get(&idx, "icons") else { panic!("icons") };
    let mut names: Vec<String> =
        CLASSES.iter().chain(STATES.iter()).chain(BADGES.iter()).map(|n| n.to_string()).collect();
    names.push("pattern.oot-stripe".to_string());
    for n in &names {
        let e = icons.iter().find(|e| s(get(e, "name")) == *n).unwrap_or_else(|| panic!("no index entry {n}"));
        assert_eq!(s(get(e, "a11y_id")), format!("icon.{n}"));
        assert_eq!(get(e, "labelless"), &CValue::Bool(true));
        let en = s(get(e, "a11y_en"));
        assert!(!en.trim().is_empty() && en.chars().count() <= 40, "{n}: {en:?}");
        for size in [16, 24] {
            assert!(repo().join(format!("design/icons/svg/{n}.{size}.svg")).is_file(), "{n}.{size}.svg");
        }
    }
    assert_eq!(icons.len(), names.len());
}

fn temp_copy(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fid-design-glyph-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("svg")).unwrap();
    std::fs::copy(repo().join("design/icons/index.json"), dir.join("index.json")).unwrap();
    for e in std::fs::read_dir(repo().join("design/icons/svg")).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if name.ends_with(".16.svg") {
            std::fs::copy(&p, dir.join("svg").join(name)).unwrap();
        }
    }
    dir
}

fn write(dir: &Path, name: &str, body: &str) {
    let text = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\" fill=\"none\" stroke=\"currentColor\" \
         stroke-width=\"1\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{body}</svg>"
    );
    std::fs::write(dir.join("svg").join(format!("{name}.16.svg")), text).unwrap();
}

fn keys(dir: &Path) -> Vec<String> {
    check(dir, &Threshold::default_value())
        .unwrap()
        .findings
        .iter()
        .map(|f| format!("{} {}", f.code, f.subject))
        .collect()
}

const OCTAGON: &str = "<polygon points=\"5,1.5 10,1.5 13.5,5 13.5,10 10,13.5 5,13.5 1.5,10 1.5,5\"/>";

/// REQ-003, hand-labelled: an A0 icon without the family octagon (only its sliders) no longer
/// shares the outer shape of W3, M1 and S1.
#[test]
fn family_member_without_shared_shape() {
    let dir = temp_copy("fam-out");
    write(&dir, "class.a0", "<path d=\"M4.5 4V11M7.5 4V11M10.5 4V11\"/>");
    let k = keys(&dir);
    for want in ["GLYPH-FAMILY class.w3 class.a0", "GLYPH-FAMILY class.m1 class.a0", "GLYPH-FAMILY class.s1 class.a0"] {
        assert!(k.contains(&want.to_string()), "{k:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// REQ-003, hand-labelled: an R0 icon drawn inside the family octagon has the same silhouette as
/// every family member.
#[test]
fn other_class_using_family_shape() {
    let dir = temp_copy("fam-in");
    write(
        &dir,
        "class.r0",
        &format!("{OCTAGON}<circle cx=\"7.5\" cy=\"7.5\" r=\"1.5\" fill=\"currentColor\" stroke=\"none\"/>"),
    );
    let k = keys(&dir);
    for m in ["class.w3", "class.m1", "class.s1", "class.a0"] {
        assert!(k.contains(&format!("GLYPH-FAMILY class.r0 {m}")), "{k:?}");
    }
    let o = run(&["glyph-distinct", "--root", &dir.to_string_lossy()]);
    assert_eq!(o.status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

/// REQ-004, hand-labelled: a stale glyph inside the conforms circle shares its outer shape.
#[test]
fn stale_sharing_conforms_shape() {
    let dir = temp_copy("stale");
    write(
        &dir,
        "state.stale",
        "<circle cx=\"7.5\" cy=\"7.5\" r=\"6\"/><polygon points=\"5.5,4.5 9.5,4.5 7.5,7.5 9.5,10.5 5.5,10.5 7.5,7.5\"/>",
    );
    assert!(keys(&dir).contains(&"GLYPH-FAMILY state.stale state.conforms".to_string()), "{:?}", keys(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Glyphs outside the profile or covering nothing are `GLYPH-RASTER` and are not compared; an
/// index entry without the right `a11y_id` is `GLYPH-MISSING`.
#[test]
fn raster_and_index_errors() {
    let dir = temp_copy("raster");
    std::fs::write(
        dir.join("svg/class.r1.16.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\"><text>R1</text></svg>",
    )
    .unwrap();
    write(&dir, "state.stale", "");
    let idx = std::fs::read_to_string(dir.join("index.json")).unwrap();
    std::fs::write(dir.join("index.json"), idx.replace("\"icon.prov.entered\"", "\"icon.entered\"")).unwrap();
    let r = check(&dir, &Threshold::default_value()).unwrap();
    let k: Vec<String> = r.findings.iter().map(|f| format!("{} {}", f.code, f.subject)).collect();
    assert_eq!(k, ["GLYPH-MISSING prov.entered", "GLYPH-RASTER class.r1", "GLYPH-RASTER state.stale"]);
    assert_eq!(r.groups.iter().map(|g| g.glyphs).collect::<Vec<_>>(), [10, 5, 5]);

    // A malformed index cannot be checked (exit 2), nor can a missing root.
    std::fs::write(dir.join("index.json"), "{\"icons\": [{\"name\": \"x\"}, {\"name\": \"x\"}]}").unwrap();
    assert_eq!(run(&["glyph-distinct", "--root", &dir.to_string_lossy()]).status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(run(&["glyph-distinct", "--root", "no/such/icons"]).status.code(), Some(2));
}

/// Without an index every glyph lacks its entry.
#[test]
fn no_index_means_missing_entries() {
    let dir = temp_copy("noindex");
    std::fs::remove_file(dir.join("index.json")).unwrap();
    let k = keys(&dir);
    assert_eq!(k.len(), CLASSES.len() + STATES.len() + BADGES.len());
    assert!(k.iter().all(|x| x.starts_with("GLYPH-MISSING ")), "{k:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--threshold`: a stricter value fails the shipped set (its closest class pair differs by
/// 25/88, its closest state pair by 14/54); bad values and misplaced flags are usage errors.
#[test]
fn threshold_and_usage() {
    let o = run(&["glyph-distinct", "--root", "design/icons", "--threshold", "0.9"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("GLYPH-SIMILAR class.w3 class.m1"), "{}", stdout(&o));
    // The closest state pair differs by 14/54 (0.259...): it passes 0.25 and fails 0.26.
    let o = run(&["glyph-distinct", "--root", "design/icons", "--threshold", "0.25", "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    let o = run(&["glyph-distinct", "--root", "design/icons", "--threshold", "0.26"]);
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    assert!(stdout(&o).contains("GLYPH-SIMILAR state.conforms state.not-decidable"), "{}", stdout(&o));
    assert!(!stdout(&o).contains("GLYPH-SIMILAR class."), "{}", stdout(&o));
    for bad in [
        &["glyph-distinct", "--root", "design/icons", "--threshold", "0"][..],
        &["glyph-distinct", "--root", "design/icons", "--threshold", "1.5"],
        &["glyph-distinct", "--root", "design/icons", "--threshold"],
        &["glyph-distinct"],
        &["glyph-distinct", "--root", "design/icons", "--all"],
        &["glyph-distinct", "--root", "design/icons", "--file", "x"],
        &["icons-lint", "--root", "design/icons", "--threshold", "0.2"],
        &["pkg-check", "--root", "design", "--threshold", "0.2"],
    ] {
        assert_eq!(run(bad).status.code(), Some(2), "{bad:?}");
    }
}

/// D0: the same input gives the same bytes.
#[test]
fn deterministic_report() {
    let a = run(&["glyph-distinct", "--root", &format!("{FX}/twins"), "--json"]);
    let b = run(&["glyph-distinct", "--root", &format!("{FX}/twins"), "--json"]);
    assert_eq!(a.stdout, b.stdout);
    // The 24 px masters are not compared, but the 16 px rasteriser refuses them.
    let text = std::fs::read_to_string(repo().join("design/icons/svg/class.r0.24.svg")).unwrap();
    assert!(rasterise(&text).is_err());
}
