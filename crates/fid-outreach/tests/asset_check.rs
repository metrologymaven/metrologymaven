//! Acceptance criteria of spec node OUTR.BRAND.ASSET.ATTR (AC-1 .. AC-7) and edge cases.
//! Expected results are hand-labelled from the spec's rules (the EXPECTED table in
//! `fixtures/outr/brand/attr/make_fixtures.py`), not produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_outreach::asset_check::{
    glob_match, is_remote, is_spdx_id, parse_allowlist, parse_attribution, parse_site_origin, scan_css, split_row,
    Origin,
};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ORIGIN: &str = "https://example.org";

fn repo(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

fn fix(rel: &str) -> PathBuf {
    repo("fixtures/outr/brand/attr").join(rel)
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn check(root: &Path, allow: &Path, origin: &str, extra: &[&str]) -> Output {
    let mut a: Vec<String> = vec![
        "asset-check".into(),
        "--root".into(),
        s(root),
        "--allow".into(),
        s(allow),
        "--origin".into(),
        origin.into(),
    ];
    a.extend(extra.iter().map(|x| x.to_string()));
    Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(&a).output().unwrap()
}

fn case(name: &str, extra: &[&str]) -> Output {
    check(&fix(name), &fix("allow.toml"), ORIGIN, extra)
}

fn show(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn lines(o: &Output) -> Vec<String> {
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("asset-check:")).map(str::to_string).collect()
}

fn expect(name: &str, code: i32, want: &[&str]) {
    let o = case(name, &[]);
    assert_eq!(o.status.code(), Some(code), "{name}: {}", show(&o));
    assert_eq!(lines(&o), want, "{name}: {}", show(&o));
}

#[test]
fn ac1_ok_tree_exits_0() {
    let o = case("ok", &["--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("\"ok\":true"), "{out}");
    assert!(out.contains("\"findings\":[]"), "{out}");
    assert!(
        out.contains(
            "\"assets\":[\"fonts/examplo-sans.woff2\",\"img/originals/mark-dark.svg\",\"img/originals/mark.svg\",\"img/photo.png\"]"
        ),
        "{out}"
    );
    assert!(out.contains("\"styles\":[\"css/site.css\",\"index.html\"]"), "{out}");
}

#[test]
fn ac2_missing_row() {
    expect("missing", 1, &["ATTR-MISSING img/x.png"]);
}

#[test]
fn ac3_non_commercial_licence() {
    expect("nc", 1, &["ATTR-LICENCE img/photo.png CC-BY-NC-4.0"]);
}

#[test]
fn ac4_remote_font_at_line_3() {
    expect("remote-font", 1, &["FONT-REMOTE css/fonts.css:3 https://fonts.example.net/a.woff2"]);
    let o = case("remote-font", &["--json"]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("\"code\":\"FONT-REMOTE\""), "{out}");
    assert!(out.contains("\"line\":\"3\""), "{out}");
}

#[test]
fn ac5_orphan_row() {
    expect("orphan", 1, &["ATTR-ORPHAN img/deleted.png"]);
}

#[test]
fn incomplete_rows_and_licence_expression() {
    expect(
        "incomplete",
        1,
        &[
            "ATTR-INCOMPLETE img/logo.svg author must be `project contributors`",
            "ATTR-INCOMPLETE img/photo.png missing source, author",
            "ATTR-LICENCE fonts/stub.otf OFL-1.1 OR GPL-3.0-only",
        ],
    );
}

#[test]
fn html_links_and_inline_style() {
    expect(
        "html-remote",
        1,
        &[
            "FONT-REMOTE index.html:4 https://fonts.example.net/css2?family=Examplo",
            "FONT-REMOTE index.html:5 //cdn.example.net/f.woff2",
            "FONT-REMOTE index.html:7 http://example.org/insecure.css",
        ],
    );
}

#[test]
fn ac6_unparsable_allowlist_exits_2() {
    let o = check(&fix("ok"), &fix("allow-broken.toml"), ORIGIN, &["--json"]);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains("\"code\":\"ATTR-PARSE\""), "{}", show(&o));
}

#[test]
fn unreadable_or_invalid_inputs_exit_2() {
    // No ATTRIBUTION.md at this root.
    let o = check(&fix(""), &fix("allow.toml"), ORIGIN, &[]);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    // Missing allowlist file.
    let o = check(&fix("ok"), &fix("no-such.toml"), ORIGIN, &[]);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    // Origin with a path, and a non-http origin.
    for bad in ["https://example.org/site", "ftp://example.org", "example.org"] {
        let o = check(&fix("ok"), &fix("allow.toml"), bad, &[]);
        assert_eq!(o.status.code(), Some(2), "{bad}: {}", show(&o));
    }
    // Bad usage.
    let o = Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(["asset-check", "--root"]).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn ac7_reports_are_byte_identical() {
    for name in ["ok", "incomplete", "html-remote"] {
        for extra in [&[][..], &["--json"][..]] {
            let a = case(name, extra);
            let b = case(name, extra);
            assert_eq!(a.stdout, b.stdout, "{name}");
            assert!(!a.stdout.is_empty());
        }
    }
}

#[test]
fn exclude_glob_skips_files() {
    let o = case("missing", &["--exclude", "img/*.png"]);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
}

#[test]
fn node_modules_and_git_are_skipped() {
    let d = std::env::temp_dir().join(format!("fid-asset-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("node_modules/pkg")).unwrap();
    std::fs::create_dir_all(d.join(".git/x")).unwrap();
    std::fs::create_dir_all(d.join("img")).unwrap();
    std::fs::write(d.join("node_modules/pkg/logo.png"), b"stub").unwrap();
    std::fs::write(d.join(".git/x/logo.png"), b"stub").unwrap();
    std::fs::write(d.join("img/a.SVG"), b"<svg/>").unwrap();
    std::fs::write(
        d.join("ATTRIBUTION.md"),
        "| Path | Origin | Source | Author | Licence |\n| :-- | --- | --- | --- | --: |\n| img/** | original | | project contributors | CC-BY-4.0 |\n",
    )
    .unwrap();
    let o = check(&d, &fix("allow.toml"), ORIGIN, &[]);
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains("1 asset file(s)"), "{}", show(&o));
}

#[test]
fn globs() {
    assert!(glob_match("img/*.svg", "img/a.svg"));
    assert!(!glob_match("img/*.svg", "img/sub/a.svg"));
    assert!(glob_match("img/**", "img/sub/a.svg"));
    assert!(glob_match("img/**/*.svg", "img/a.svg"));
    assert!(glob_match("img/**/*.svg", "img/x/y/a.svg"));
    assert!(glob_match("**/*.png", "a.png"));
    assert!(glob_match("img/?.png", "img/x.png"));
    assert!(!glob_match("img/?.png", "img/xy.png"));
    assert!(!glob_match("img/a.svg", "img/A.svg"));
    assert!(glob_match("a*b*c", "aXXbYYc"));
    assert!(!glob_match("a*b*c", "aXXbYY"));
}

#[test]
fn spdx_ids() {
    for ok in ["CC0-1.0", "OFL-1.1", "CC-BY-4.0", "GPL-2.0+", "LicenseRef-Example-1"] {
        assert!(is_spdx_id(ok), "{ok}");
    }
    for bad in ["", "MIT OR Apache-2.0", "(MIT)", "-MIT", "CC0 1.0", "MIT+x", "MIT/X"] {
        assert!(!is_spdx_id(bad), "{bad}");
    }
    let allow = parse_allowlist("asset_licences = [\"OFL-1.1\"]", "a").unwrap();
    assert!(allow.allows("ofl-1.1"));
    assert!(!allow.allows("OFL-1.0"));
    assert!(parse_allowlist("licences = []", "a").is_err());
    assert!(parse_allowlist("asset_licences = \"OFL-1.1\"", "a").is_err());
    assert!(parse_allowlist("asset_licences = [\"OFL-1.1 OR MIT\"]", "a").is_err());
    assert!(parse_allowlist("asset_licences = [1]", "a").is_err());
}

#[test]
fn origins() {
    let site = parse_site_origin("https://Example.org/").unwrap();
    for local in ["/a.css", "a.woff2", "../f/a.woff2", "https://example.org/a", "HTTPS://EXAMPLE.ORG:443/a", "data:x"] {
        assert!(!is_remote(local, &site), "{local}");
    }
    for remote in [
        "https://example.net/a",
        "http://example.org/a",
        "https://example.org:8443/a",
        "//example.net/a",
        "https://example.org.evil.example/a",
        "https://user@example.net/a",
        "file:///c/a.woff2",
    ] {
        assert!(is_remote(remote, &site), "{remote}");
    }
    assert!(!is_remote("//example.org/a", &site));
}

#[test]
fn css_scan_scope() {
    let site = parse_site_origin(ORIGIN).unwrap();
    let css = "a { background: url(https://cdn.example.net/bg.png) }\n\
               p::before { content: \"@import url(https://x.example.net/a.css)\" }\n\
               /* @font-face { src: url(https://x.example.net/c.woff) } */\n\
               @font-face{src:local(X),URL( 'https://x.example.net/b.woff' )}\n\
               @IMPORT url(https://x.example.net/d.css) screen;\n";
    let found: Vec<String> = scan_css("s.css", css, 1, &site).iter().map(|f| f.to_string()).collect();
    assert_eq!(
        found,
        ["FONT-REMOTE s.css:4 https://x.example.net/b.woff", "FONT-REMOTE s.css:5 https://x.example.net/d.css"]
    );
}

#[test]
fn table_parsing() {
    let t = "Intro\n\n| a | b |\n|---|---|\n| x | y |\n\n| path | origin | source | author | licence |\n\
             |---|---|---|---|---|\n| `./img/a\\|b.png` | third-party | s | A | CC0-1.0 |\n| img/c.svg | Original | — | project contributors | CC-BY-4.0\nafter\n";
    let rows = parse_attribution(t, "A.md").unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].path, "img/a|b.png");
    assert_eq!(rows[0].line, 9);
    assert_eq!(rows[1].origin, Origin::Original);
    assert_eq!(rows[1].source, "");
    assert_eq!(rows[1].licence, "CC-BY-4.0");

    let head = "| path | origin | source | author | licence |\n|---|---|---|---|---|\n";
    assert!(parse_attribution("no table here\n", "A.md").is_err());
    assert!(parse_attribution("| path | origin | source | author | licence |\n| x |\n", "A.md").is_err());
    assert!(parse_attribution(&format!("{head}| a.png | borrowed | s | A | MIT |\n"), "A.md").is_err());
    assert!(parse_attribution(&format!("{head}| a.png | original | s |\n"), "A.md").is_err());
    assert!(parse_attribution(
        &format!("{head}| ../a.png | original | | project contributors | CC-BY-4.0 |\n"),
        "A.md"
    )
    .is_err());
    assert!(parse_attribution(&format!("{head}| /a.png | original | | project contributors | CC-BY-4.0 |\n"), "A.md")
        .is_err());
    assert_eq!(parse_attribution(head, "A.md").unwrap(), vec![]);
    assert_eq!(split_row("not a row"), None);
}
