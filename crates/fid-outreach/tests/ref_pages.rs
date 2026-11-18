//! Acceptance criteria of spec node OUTR.DOCS.REF (AC-1 .. AC-8) and edge cases.
//! The expected pages under `fixtures/outr/docs/ref/expected*` are rendered independently by
//! `fixtures/outr/docs/ref/make_fixtures.py` from the layout in the spec and module docs; the
//! wording checks below are hand-labelled from the spec's requirements. Nothing expected here
//! was produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_outreach::ref_pages::{escape, parse_record, valid_release, RefError, MAX_RECORD_BYTES};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const COMMANDS: [&str; 12] = [
    "demo.adopt_doc@1",
    "demo.compute_sum@2",
    "demo.edit_doc@1",
    "demo.edit_draft@1",
    "demo.export_report@1",
    "demo.move_axis@1",
    "demo.probe_power@1",
    "demo.read_item@1",
    "demo.release_record@1",
    "demo.set_limits@1",
    "demo.sign_record@1",
    "demo.sim_step@1",
];

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/outr/docs/ref").join(rel)
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn bin(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(args).output().unwrap()
}

fn show(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-ref-pages-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn render(manifest: &str, records: &str, surfaces: &str, out: &Path) -> Output {
    bin(&[
        "ref-render".into(),
        "--records".into(),
        s(&fix(records)),
        "--manifest".into(),
        s(&fix(manifest)),
        "--surfaces".into(),
        s(&fix(surfaces)),
        "--release".into(),
        "v0.1.0".into(),
        "--out".into(),
        s(out),
    ])
}

fn render_min(out: &Path) -> Output {
    render("min/min-classes.json", "min/records", "min/surfaces.toml", out)
}

fn check(manifest: &str, records: &str, tree: &Path, json: bool) -> Output {
    let mut a = vec![
        "ref-check".to_string(),
        "--records".into(),
        s(&fix(records)),
        "--manifest".into(),
        s(&fix(manifest)),
        "--surfaces".into(),
        s(&fix("min/surfaces.toml")),
        "--ref".into(),
        s(tree),
    ];
    if json {
        a.push("--json".into());
    }
    bin(&a)
}

fn check_min(tree: &Path, json: bool) -> Output {
    check("min/min-classes.json", "min/records", tree, json)
}

/// Every file under `dir`, keyed by its `/`-separated relative path.
fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, d: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut m = BTreeMap::new();
    walk(dir, dir, &mut m);
    m
}

fn copy_tree(from: &Path, to: &Path) {
    for (rel, bytes) in tree(from) {
        let p = to.join(&rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
}

fn expected_tree() -> PathBuf {
    fix("expected/v0.1.0/ref")
}

fn page(out: &Path, surface: &str, cmd: &str) -> String {
    std::fs::read_to_string(out.join("v0.1.0/ref").join(surface).join(format!("{cmd}.html"))).unwrap()
}

#[test]
fn ac1_three_pages_per_command_at_stable_paths() {
    let out = tmp("ac1");
    let o = render_min(&out);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let got = tree(&out);
    let mut want: Vec<String> = Vec::new();
    for surface in ["cli", "mcp", "python"] {
        for c in COMMANDS {
            want.push(format!("v0.1.0/ref/{surface}/{c}.html"));
        }
    }
    want.sort();
    assert_eq!(got.keys().cloned().collect::<Vec<_>>(), want);
}

#[test]
fn ac1_pages_equal_the_independent_rendering() {
    let out = tmp("golden");
    assert_eq!(render_min(&out).status.code(), Some(0));
    let got = tree(&out.join("v0.1.0/ref"));
    let want = tree(&expected_tree());
    assert_eq!(got.len(), 36);
    for (path, bytes) in &want {
        assert_eq!(got.get(path), Some(bytes), "page {path} differs from the Python rendering");
    }
    assert_eq!(got.keys().collect::<Vec<_>>(), want.keys().collect::<Vec<_>>());
}

#[test]
fn ac2_mcp_unavailability_and_reasons() {
    let out = tmp("ac2");
    assert_eq!(render_min(&out).status.code(), Some(0));
    for c in ["demo.move_axis@1", "demo.sign_record@1", "demo.set_limits@1"] {
        let p = page(&out, "mcp", c);
        assert!(p.contains("not available on MCP"), "{c}");
        assert!(p.contains("Reason: never on MCP for this class"), "{c}");
        assert!(!p.contains("<h2>Parameters</h2>"), "{c}: no tool schema when unavailable");
    }
    for c in ["demo.release_record@1", "demo.export_report@1", "demo.probe_power@1"] {
        let p = page(&out, "mcp", c);
        assert!(p.contains("Reason: request-only"), "{c}");
    }
    let p = page(&out, "mcp", "demo.adopt_doc@1");
    assert!(p.contains("Reason: human-only"));
    assert!(p.contains("<dt>Human only</dt><dd>true</dd>"));
    let p = page(&out, "mcp", "demo.read_item@1");
    assert!(p.contains("<p><code>demo_read_item</code></p>"));
    assert!(!p.contains("Reason:"));
    // REQ-004 facts on every page.
    let p = page(&out, "cli", "demo.move_axis@1");
    for fact in [
        "<dt>Command</dt><dd><code>demo.move_axis@1</code></dd>",
        "<dt>Side-effect class</dt><dd>M1</dd>",
        "<dt>Determinism</dt><dd>N</dd>",
        "<dt>Human only</dt><dd>false</dd>",
        "<dt>MCP</dt><dd>none</dd>",
        "<p><code>fid demo move-axis</code></p>",
    ] {
        assert!(p.contains(fact), "missing {fact}");
    }
}

#[test]
fn ac3_python_pages_with_and_without_the_surface() {
    let out = tmp("ac3a");
    assert_eq!(render_min(&out).status.code(), Some(0));
    for c in COMMANDS {
        let p = page(&out, "python", c);
        assert!(p.contains("Python bindings are not part of this release."), "{c}");
        assert!(!p.contains("fiducial."), "{c}: no signature");
        assert!(!p.contains("<h2>Parameters</h2>"), "{c}");
    }
    let out = tmp("ac3b");
    let o = render("min/min-classes.json", "min/records", "min/surfaces-python.toml", &out);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let want = tree(&fix("expected-python"));
    assert_eq!(want.len(), 12);
    for (path, bytes) in &want {
        let got = std::fs::read(out.join("v0.1.0/ref").join(path)).unwrap();
        assert_eq!(&got, bytes, "{path}");
    }
    let p = page(&out, "python", "demo.read_item@1");
    assert!(p.contains("<p><code>fiducial.demo.read_item</code></p>"));
    assert!(!p.contains("not part of this release"));
}

#[test]
fn ac4_clean_check_passes() {
    let out = tmp("ac4");
    assert_eq!(render_min(&out).status.code(), Some(0));
    let o = check_min(&out.join("v0.1.0/ref"), false);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let o = check_min(&expected_tree(), true);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim_end(), r#"{"findings":[],"ok":true}"#);
}

#[test]
fn ac5_one_changed_word_is_drift() {
    let d = tmp("ac5");
    copy_tree(&expected_tree(), &d);
    let p = d.join("cli/demo.edit_doc@1.html");
    let text = std::fs::read_to_string(&p).unwrap().replacen("Synthetic W2", "Synthetic W3", 1);
    std::fs::write(&p, text).unwrap();
    let o = check_min(&d, true);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(
        String::from_utf8_lossy(&o.stdout).trim_end(),
        r#"{"findings":[{"code":"page_drift","path":"cli/demo.edit_doc@1.html"}],"ok":false}"#
    );
    let o = check_min(&d, false);
    assert!(String::from_utf8_lossy(&o.stdout).contains("page_drift cli/demo.edit_doc@1.html"));
}

#[test]
fn ac6_extra_page() {
    let d = tmp("ac6-extra");
    copy_tree(&expected_tree(), &d);
    std::fs::write(d.join("mcp/demo.ghost@1.html"), "<article></article>\n").unwrap();
    let o = check_min(&d, true);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(
        String::from_utf8_lossy(&o.stdout).trim_end(),
        r#"{"findings":[{"code":"page_without_command","path":"mcp/demo.ghost@1.html"}],"ok":false}"#
    );
}

#[test]
fn ac6_missing_record() {
    let o = check("missing-record/min-classes.json", "missing-record/records", &expected_tree(), true);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(
        String::from_utf8_lossy(&o.stdout).trim_end(),
        r#"{"findings":[{"code":"record_missing","path":"demo.orphan@1.json"}],"ok":false}"#
    );
    let out = tmp("ac6-render");
    let o = render("missing-record/min-classes.json", "missing-record/records", "min/surfaces.toml", &out);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains("record_missing demo.orphan@1.json"));
    assert!(!out.exists(), "nothing written");
}

#[test]
fn page_missing_is_reported() {
    let d = tmp("missing-page");
    copy_tree(&expected_tree(), &d);
    std::fs::remove_file(d.join("python/demo.sim_step@1.html")).unwrap();
    let o = check_min(&d, false);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stdout).contains("page_missing python/demo.sim_step@1.html"));
}

#[test]
fn ac7_stale_records_write_nothing() {
    let out = tmp("ac7");
    let o = render("min/min-classes.json", "stale/records", "min/surfaces.toml", &out);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(stdout.contains("manifest_digest_mismatch demo.read_item@1.json"), "{stdout}");
    assert!(!out.exists(), "nothing written");
    let o = check("min/min-classes.json", "stale/records", &expected_tree(), true);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains(r#""code":"manifest_digest_mismatch""#));
}

#[test]
fn ac8_reruns_are_byte_identical() {
    let a = tmp("ac8a");
    let b = tmp("ac8b");
    assert_eq!(render_min(&a).status.code(), Some(0));
    assert_eq!(render_min(&b).status.code(), Some(0));
    assert_eq!(tree(&a), tree(&b));
}

#[test]
fn bad_usage_and_release_tags() {
    assert!(valid_release("v0.1.0"));
    assert!(valid_release("2026.11-rc1"));
    assert!(valid_release("console"));
    assert!(valid_release("com10"));
    for bad in ["", ".", "..", "../x", "a/b", "a\\b", "v 1", "v1.", "con", "NUL", "aux.1", "Com1.0", "lpt9", "prn"] {
        assert!(!valid_release(bad), "{bad:?}");
    }
    let o = bin(&["ref-render".into(), "--release".into(), "v1".into()]);
    assert_eq!(o.status.code(), Some(2));
    let out = tmp("badrel");
    let o = bin(&[
        "ref-render".into(),
        "--records".into(),
        s(&fix("min/records")),
        "--manifest".into(),
        s(&fix("min/min-classes.json")),
        "--surfaces".into(),
        s(&fix("min/surfaces.toml")),
        "--release".into(),
        "..".into(),
        "--out".into(),
        s(&out),
    ]);
    assert_eq!(o.status.code(), Some(2));
    assert!(!out.exists());
}

fn record_text() -> String {
    std::fs::read_to_string(fix("min/records/demo.read_item@1.json")).unwrap()
}

#[test]
fn record_parse_errors() {
    let ok = record_text();
    assert!(parse_record(ok.as_bytes(), "demo.read_item@1.json").is_ok());
    // File name must match the record's command major.
    assert!(matches!(parse_record(ok.as_bytes(), "demo.read_item@2.json"), Err(RefError::RecordParse { .. })));
    // Unknown reason code.
    let bad = ok.replace(r#""available":true,"name":"demo_read_item""#, r#""available":false,"reason":"nope""#);
    assert_ne!(bad, ok);
    assert!(matches!(parse_record(bad.as_bytes(), "demo.read_item@1.json"), Err(RefError::RecordParse { .. })));
    // Unknown side-effect class.
    let bad = ok.replace(r#""side_effect":"R0""#, r#""side_effect":"Z9""#);
    assert!(matches!(parse_record(bad.as_bytes(), "demo.read_item@1.json"), Err(RefError::RecordParse { .. })));
    // Major beyond 5 digits, as for CommandId.
    let bad = ok.replace(r#""major":1"#, r#""major":100000"#);
    assert_ne!(bad, ok);
    assert!(matches!(parse_record(bad.as_bytes(), "demo.read_item@100000.json"), Err(RefError::RecordParse { .. })));
    // Over the 1 MB bound.
    let big = format!("{}{}", " ".repeat(MAX_RECORD_BYTES), ok);
    assert!(matches!(parse_record(big.as_bytes(), "demo.read_item@1.json"), Err(RefError::RecordParse { .. })));
}

#[test]
fn help_text_is_escaped_verbatim() {
    assert_eq!(escape(r#"a < b & c > d "q" 's'"#), "a &lt; b &amp; c &gt; d &quot;q&quot; &#39;s&#39;");
    assert_eq!(escape("a\n\nb\r\n"), "a&#10;&#10;b&#13;&#10;");
    let out = tmp("esc");
    assert_eq!(render_min(&out).status.code(), Some(0));
    let p = page(&out, "cli", "demo.read_item@1");
    assert!(p.contains("Compare a &lt; b &amp; c &gt; d, &quot;quoted&quot; and &#39;single&#39;.</pre>"));
    // The help has blank lines; the page must not, so a CommonMark processor cannot end the HTML
    // block inside <pre> (and each value stays on one line).
    assert!(p.contains("read item.&#10;&#10;Usage notes"));
    for (path, bytes) in tree(&out) {
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("\n\n") && !text.contains('\r'), "{path} has a blank line or CR");
        let pre = text.lines().find(|l| l.starts_with("<pre class=\"help\">")).unwrap();
        assert!(pre.ends_with("</pre>"), "{path}: help spans lines");
    }
}
