//! Acceptance criteria of spec node ECO.REL.PROC.NOTES (AC-1 .. AC-4). The fixtures under
//! `fixtures/rel/notes/` are fictional and hand written; `basic/expected.md` and every expected
//! line below are hand-labelled from the spec's rules, with the SHA-256 values computed by
//! coreutils `sha256sum` over the fixture reports (never by running this crate).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::rel::notes::{
    escape_title, group_of, parse_export, parse_gates, parse_threat_summary, read_reports, render, run, NotesError,
    GROUPS, MAX_TITLE_CHARS, TRUNCATED,
};
use fid_xtask::rel::version::Channel;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/rel/notes").join(name)
}

/// A fresh, empty directory under the target directory (never inside the fixtures).
fn tmp_dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("rel-notes").join(name);
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// Runs `fid-xtask rel notes <args>` in `cwd` (what `cargo xtask rel notes ...` runs);
/// returns (exit code, stdout).
fn cli(cwd: &Path, args: &[&str]) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fid-xtask")).current_dir(cwd).args(["rel", "notes"]).args(args).output().unwrap();
    (o.status.code().unwrap(), String::from_utf8(o.stdout).unwrap())
}

fn notes_for(case: &str, prs: &str, name: &str) -> String {
    let out = tmp_dir(name);
    let out_s = out.to_str().unwrap();
    let args = ["--tag", "v1.4.0", "--channel", "stable", "--prs", prs, "--reports", "reports/", "--out", out_s];
    let (code, _) = cli(&fixture(case), &args);
    assert_eq!(code, 0);
    std::fs::read_to_string(out.join("release-notes.md")).unwrap()
}

/// AC-1: the basic fixture (12 PRs, v1.3.0 to v1.4.0 on stable) gives `expected.md` byte for byte.
#[test]
fn ac1_basic_matches_expected() {
    let out = tmp_dir("ac1");
    let out_s = out.to_str().unwrap();
    let args = ["--tag", "v1.4.0", "--channel", "stable", "--prs", "prs.json", "--reports", "reports/", "--out", out_s];
    let (code, stdout) = cli(&fixture("basic"), &args);
    assert_eq!(code, 0, "{stdout}");
    assert!(stdout.starts_with("notes: ok "), "{stdout}");
    let got = std::fs::read(out.join("release-notes.md")).unwrap();
    let want = std::fs::read(fixture("basic/expected.md")).unwrap();
    assert_eq!(String::from_utf8_lossy(&got), String::from_utf8_lossy(&want));
    assert_eq!(got, want);
    // No temporary file is left behind.
    let names: Vec<_> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, vec![std::ffi::OsString::from("release-notes.md")]);
}

/// AC-1, REQ-001 and D0: every PR appears once, and the output does not depend on the export's
/// order or on the run.
#[test]
fn ac1_deterministic_and_order_independent() {
    let bytes = std::fs::read(fixture("basic/prs.json")).unwrap();
    let export = parse_export(&bytes, "v1.4.0", Channel::Stable).unwrap();
    let reports = read_reports(&fixture("basic/reports"), "v1.4.0", Channel::Stable).unwrap();
    let a = render(&export, &reports);
    let mut reversed = export.clone();
    reversed.prs.reverse();
    assert_eq!(render(&reversed, &reports), a);
    for pr in &export.prs {
        assert_eq!(a.matches(&format!("- #{} ", pr.number)).count(), 1, "PR #{}", pr.number);
    }
    assert_eq!(notes_for("basic", "prs.json", "det-1"), notes_for("basic", "prs.json", "det-2"));
}

/// AC-2: without `status.json` the tool exits 1 naming it, and writes nothing.
#[test]
fn ac2_missing_status() {
    let out = tmp_dir("ac2");
    let out_s = out.to_str().unwrap();
    let args = ["--tag", "v1.4.0", "--channel", "stable", "--prs", "../basic/prs.json", "--reports", "reports/", "--out", out_s];
    let (code, stdout) = cli(&fixture("missing-status"), &args);
    assert_eq!((code, stdout.as_str()), (1, "NOTES-MISSING status.json\n"));
    assert!(!out.exists());
}

/// REQ-002: every missing report is named at once, in a fixed order.
#[test]
fn missing_reports_all_named() {
    let empty = tmp_dir("empty-reports");
    std::fs::create_dir_all(&empty).unwrap();
    let errs = read_reports(&empty, "v1.4.0", Channel::Stable).unwrap_err();
    let lines: Vec<String> = errs.iter().map(ToString::to_string).collect();
    assert_eq!(
        lines,
        [
            "NOTES-MISSING parity.json",
            "NOTES-MISSING parity.md",
            "NOTES-MISSING status.json",
            "NOTES-MISSING status.md",
            "NOTES-MISSING release-numbers.json",
            "NOTES-MISSING release-gates.json",
            "NOTES-MISSING evidence/statement.json",
            "NOTES-MISSING threat/register-summary.json",
        ]
    );
    assert!(errs.iter().all(|e| e.exit_code() == 1));
}

/// AC-3: hostile titles are escaped, flattened to one line and cut; no link, image, HTML or
/// heading can form.
#[test]
fn ac3_hostile_titles() {
    let text = notes_for("hostile-title", "prs.json", "ac3");
    let cut = format!("- #304 {}{TRUNCATED}", "abcdefghij".repeat(20));
    let want = [
        r"- #301 Fix \[link\]\(javascript\:x\) and \*bold\*",
        r"- #302 \<script\>alert\(1\)\<\/script\> \!\[img\]\(x\.png\)",
        r"- #303 Line one \#\# Injected heading  \| a \| b \|",
        cut.as_str(),
        "- #305 Swap  evil  text",
        "- #306 (no title)",
    ];
    let section = format!("### Fixes\n\n{}\n\n", want.join("\n"));
    assert!(text.contains(&section), "{text}");
    let changes = text.split("## Attachments").next().unwrap();
    assert!(!changes.contains("](") && !changes.contains("<script>") && !changes.contains("*bold*"));
    assert!(!text.contains("THIS-TAIL-IS-CUT"));
    assert!(!text.contains('\u{202E}') && !text.contains('\r'));
    assert!(!text.lines().any(|l| l.starts_with("## Injected")));
}

/// AC-4: a Tier 2 build failure in `release-gates.json` gets its own section, naming it.
#[test]
fn ac4_tier2_failure_listed() {
    let text = notes_for("tier2-fail", "../basic/prs.json", "ac4");
    let section = "## Tier 2 build failures\n\n- Tier 2 build failed\\: fid linux\\-aarch64 \\(exit 101\\)\n\n";
    assert!(text.contains(section), "{text}");
    assert!(text.contains(
        "| [release-gates.json](release-gates.json) | \
         `sha256:1f728ba16150b4670e8ee60de679d52b7e8e5f45caaa4c118000ed1a050d6b03` |\n"
    ));
}

/// REQ-004: the header line, then the version, channel and commit.
#[test]
fn header_lines() {
    let text = notes_for("basic", "prs.json", "header");
    assert!(text.starts_with("Generated by fid-xtask rel notes. Do not edit.\n\n# Release v1.4.0\n\n- Version: 1.4.0\n"));
    assert!(text.contains("- Channel: stable\n- Commit: 0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c\n"));
}

#[test]
fn escape_and_truncate_rules() {
    let exact: String = "é".repeat(MAX_TITLE_CHARS);
    assert_eq!(escape_title(&exact), exact);
    let long = format!("{exact}x");
    assert_eq!(escape_title(&long), format!("{exact}{TRUNCATED}"));
    assert_eq!(escape_title("a_b`c~d"), r"a\_b\`c\~d");
    assert_eq!(escape_title("\u{2028}\t x \u{FEFF}"), "x");
    assert_eq!(escape_title(""), "(no title)");
}

#[test]
fn grouping_rules() {
    let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(group_of(&l(&[])), GROUPS.len());
    assert_eq!(group_of(&l(&["ci", "wontfix"])), GROUPS.len());
    assert_eq!(group_of(&l(&["DOCS", "Feature"])), 0);
    assert_eq!(group_of(&l(&["internal", "breaking"])), 3);
    assert_eq!(GROUPS.map(|g| g.0), ["feature", "fix", "security", "breaking", "docs", "internal"]);
}

fn export_json(previous: &str, prs: &str) -> String {
    format!(
        "{{\"tag\":\"v1.4.0\",\"channel\":\"stable\",\"previous_tag\":{previous},\
         \"commit\":\"0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c\",\"prs\":[{prs}]}}"
    )
}

fn pr(n: &str) -> String {
    format!(
        "{{\"number\":{n},\"title\":\"t\",\"labels\":[],\"merged_at\":\"2026-10-01T00:00:00Z\",\
         \"merge_commit\":\"0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c\"}}"
    )
}

fn parse_error(json: &str, tag: &str, channel: Channel) -> String {
    match parse_export(json.as_bytes(), tag, channel) {
        Err(e @ NotesError::Parse(_)) => e.to_string(),
        other => panic!("expected INPUT-PARSE, got {other:?}"),
    }
}

/// The export must be for this tag and channel, its previous tag on the same channel and
/// lower, and every PR listed once.
#[test]
fn export_range_and_shape_errors() {
    let ok = export_json("\"v1.3.0\"", &pr("1"));
    assert!(parse_export(ok.as_bytes(), "v1.4.0", Channel::Stable).is_ok());
    assert!(parse_export(export_json("null", "").as_bytes(), "v1.4.0", Channel::Stable).is_ok());
    let cases = [
        (export_json("\"v1.3.0\"", &pr("1")), "v1.4.1", "export is for tag"),
        (export_json("\"v1.4.0-beta.2\"", &pr("1")), "v1.4.0", "VERSION-CHANNEL-SUFFIX"),
        (export_json("\"v1.4.0\"", &pr("1")), "v1.4.0", "is not lower than"),
        (export_json("\"v1.5.0\"", &pr("1")), "v1.4.0", "is not lower than"),
        (export_json("\"v1.3.0\"", &format!("{},{}", pr("7"), pr("7"))), "v1.4.0", "PR #7 is listed twice"),
        (export_json("\"v1.3.0\"", &pr("0")), "v1.4.0", "positive integer"),
        (export_json("\"v1.3.0\"", &pr("1.5")), "v1.4.0", "positive integer"),
        (export_json("\"v1.3.0\"", &pr("-3")), "v1.4.0", "positive integer"),
        (export_json("\"v1.3.0\"", &pr("1e300")), "v1.4.0", "positive integer"),
        (export_json("\"v1.3.0\"", "{}"), "v1.4.0", "missing `number`"),
        (export_json("\"v1.3.0\"", "{\"number\":1,\"number\":2}"), "v1.4.0", "duplicate_key"),
        (export_json("\"v1.3.0\"", &pr("1").replace("0f1e", "0F1E")), "v1.4.0", "not a git object ID"),
        (export_json("3", &pr("1")), "v1.4.0", "string or null"),
        (export_json("\"v1.3.0\"", &pr("1")), "v1.4", "--tag: VERSION-TAG-FORMAT"),
        ("[]".to_string(), "v1.4.0", "not a JSON object"),
    ];
    for (json, tag, want) in cases {
        let got = parse_error(&json, tag, Channel::Stable);
        assert!(got.starts_with("INPUT-PARSE: ") && got.contains(want), "{got} (wanted {want})");
    }
    let beta = export_json("\"v1.3.0\"", &pr("1"));
    assert!(parse_error(&beta, "v1.4.0", Channel::Beta).contains("VERSION-CHANNEL-SUFFIX"));
}

#[test]
fn gates_and_threat_errors() {
    let g = |s: &str| parse_gates(s.as_bytes(), "v1.4.0", Channel::Stable);
    let base = r#"{"release":"v1.4.0","channel":"stable","verdict":"pass","gates":[]}"#;
    assert_eq!(g(base).unwrap().tier2_failures, Vec::<String>::new());
    for (bad, want) in [
        (base.replace("v1.4.0", "v1.3.0"), "verdict is for release"),
        (base.replace("stable", "beta"), "verdict is for channel"),
        (base.replace("pass", "maybe"), "must be pass or block"),
        (base.replace("[]", r#"[{"id":"pkg-plan"},{"id":"PKG-PLAN"}]"#), "listed twice"),
        (base.replace("[]", r#"[{"id":"pkg-plan","notes":[1]}]"#), "every note must be a string"),
    ] {
        let e = g(&bad).unwrap_err().to_string();
        assert!(e.starts_with("INPUT-PARSE: ") && e.contains(want), "{e}");
    }
    let t = |s: &str| parse_threat_summary(s.as_bytes());
    assert_eq!(t(r#"{"schema":"qual-threat-summary/1","counts":{"high":0}}"#).unwrap(), 0);
    for bad in [
        r#"{"schema":"qual-threat-summary/2","counts":{"high":0}}"#,
        r#"{"schema":"qual-threat-summary/1","counts":{"high":-1}}"#,
        r#"{"schema":"qual-threat-summary/1","counts":{}}"#,
        r#"{"schema":"qual-threat-summary/1","counts":{"high":2},"high_ids":["TM-001"]}"#,
    ] {
        assert!(matches!(t(bad), Err(NotesError::Parse(_))), "{bad}");
    }
}

/// Usage errors and unreadable input exit 2; nothing panics on cut-off or garbage input.
#[test]
fn bad_usage_and_garbage_never_panic() {
    let basic = fixture("basic");
    assert_eq!(cli(&basic, &["--tag", "v1.4.0"]).0, 2);
    assert_eq!(cli(&basic, &["--tag", "v1.4.0", "--channel", "weekly"]).0, 2);
    let out = tmp_dir("garbage");
    let out_s = out.to_str().unwrap();
    let args = ["--tag", "v1.4.0", "--channel", "stable", "--prs", "nope.json", "--reports", "reports/", "--out", out_s];
    let (code, stdout) = cli(&basic, &args);
    assert_eq!(code, 2);
    assert!(stdout.starts_with("INPUT-PARSE: "), "{stdout}");
    let bytes = std::fs::read(basic.join("prs.json")).unwrap();
    for end in (0..bytes.len()).step_by(7) {
        let _ = parse_export(&bytes[..end], "v1.4.0", Channel::Stable);
    }
    let _ = parse_export(&[0xff, 0xfe, 0x00], "v1.4.0", Channel::Stable);
    let deep = "[".repeat(100_000);
    assert!(parse_export(deep.as_bytes(), "v1.4.0", Channel::Stable).is_err());
}

/// Performance budget: 2 000 PRs in under 1 s (parse and render).
#[test]
fn two_thousand_prs_under_a_second() {
    let mut prs = String::new();
    for n in 1..=2000 {
        if n > 1 {
            prs.push(',');
        }
        let label = ["feature", "fix", "docs", "ci"][n % 4];
        write!(
            prs,
            "{{\"number\":{n},\"title\":\"Change number {n} [x](y) *z*\",\"labels\":[\"{label}\"],\
             \"merged_at\":\"2026-10-01T00:00:00Z\",\"merge_commit\":\"0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c\"}}"
        )
        .unwrap();
    }
    let json = export_json("\"v1.3.0\"", &prs);
    let reports = read_reports(&fixture("basic/reports"), "v1.4.0", Channel::Stable).unwrap();
    let start = Instant::now();
    let export = parse_export(json.as_bytes(), "v1.4.0", Channel::Stable).unwrap();
    let text = render(&export, &reports);
    assert!(start.elapsed().as_secs_f64() < 1.0, "{:?}", start.elapsed());
    assert_eq!(text.matches("\n- #").count(), 2000);
}

/// `run` writes into a new output directory and returns the same text it wrote.
#[test]
fn run_writes_what_it_returns() {
    let out = tmp_dir("run").join("nested");
    let b = fixture("basic");
    let text = run("v1.4.0", Channel::Stable, &b.join("prs.json"), &b.join("reports"), &out).unwrap();
    assert_eq!(std::fs::read_to_string(out.join("release-notes.md")).unwrap(), text);
}
