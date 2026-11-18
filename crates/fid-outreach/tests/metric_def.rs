//! Acceptance criteria of spec node OUTR.COMM.METRIC.DEF (AC-1 .. AC-5) and further error
//! paths. Fixtures under `fixtures/outr/comm/metric/def/` are hand-written; every expected
//! finding below is hand-labelled from the spec's rules, not produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_outreach::metrics::def::{check, check_files, normalise, Code, DENY_LIST};
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

fn fixture(name: &str) -> PathBuf {
    repo("fixtures/outr/comm/metric/def").join(name)
}

/// Runs `fid-outreach metric-def check`; returns (exit code, stdout).
fn cli(defs: &Path, sources: &Path, json: bool) -> (i32, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fid-outreach"));
    cmd.args(["metric-def", "check", "--defs"]).arg(defs).arg("--sources").arg(sources);
    if json {
        cmd.arg("--json");
    }
    let out = cmd.output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

fn finding_lines(defs: &str, sources: &str) -> Vec<String> {
    let r = check_files(&fixture(defs), &fixture(sources)).unwrap();
    r.findings.iter().map(ToString::to_string).collect()
}

#[test]
fn ac1_committed_files_pass_and_list_six_metrics() {
    let defs = repo("metrics/definitions.toml");
    let sources = repo("metrics/sources.toml");
    let (code, out) = cli(&defs, &sources, true);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with('{') && out.contains("\"status\":\"ok\""), "{out}");
    assert!(out.contains("\"findings\":[]"), "{out}");
    let ids = [
        "release_downloads_github",
        "repo_stars",
        "repo_forks",
        "contributors_distinct",
        "forum_posts",
        "chat_messages_public",
    ];
    for id in ids {
        assert!(out.contains(&format!("\"id\":\"{id}\"")), "{id} missing in {out}");
    }
    let r = check_files(&defs, &sources).unwrap();
    assert!(r.passed());
    assert_eq!(r.metrics.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ids);
    // Every metric states at least one bias and names a listed public source.
    for m in &r.metrics {
        assert!(!m.biases.is_empty());
        assert!(r.sources.iter().any(|s| s.id == m.source));
    }
    // Text mode lists them too.
    let (code, text) = cli(&defs, &sources, false);
    assert_eq!(code, 0);
    for id in ids {
        assert!(text.contains(&format!("metric {id}\n")), "{text}");
    }
    assert!(text.ends_with("metric-def check: 6 metric(s), 5 source(s), 0 finding(s)\n"), "{text}");
}

#[test]
fn baseline_fixtures_pass() {
    let (code, out) = cli(&fixture("definitions.toml"), &fixture("sources.toml"), false);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn ac2_no_bias_field_missing_biases() {
    let (code, out) = cli(&fixture("no-bias.toml"), &fixture("sources.toml"), false);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("field_missing biases"), "{out}");
    assert_eq!(finding_lines("no-bias.toml", "sources.toml"), ["field_missing biases (metric #1 \"repo_stars\")"]);
    let (code, json) = cli(&fixture("no-bias.toml"), &fixture("sources.toml"), true);
    assert_eq!(code, 1);
    assert!(json.contains("\"code\":\"field_missing\"") && json.contains("\"subject\":\"biases\""), "{json}");
}

#[test]
fn ac3_private_source_not_public() {
    let (code, out) = cli(&fixture("definitions.toml"), &fixture("private-source.toml"), false);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("source_not_public"), "{out}");
    assert_eq!(
        finding_lines("definitions.toml", "private-source.toml"),
        ["source_not_public (source #1 \"repo-api\")"]
    );
}

#[test]
fn ac4_telemetry_source_is_product_data_even_if_public() {
    let (code, out) = cli(&fixture("definitions.toml"), &fixture("telemetry-source.toml"), true);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("\"code\":\"product_data_source\""), "{out}");
    assert_eq!(
        finding_lines("definitions.toml", "telemetry-source.toml"),
        ["product_data_source telemetry (source #2 \"usage-summary\")"]
    );
}

#[test]
fn ac5_unknown_source() {
    let (code, out) = cli(&fixture("unknown-source.toml"), &fixture("sources.toml"), false);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("source_unknown"), "{out}");
    assert_eq!(
        finding_lines("unknown-source.toml", "sources.toml"),
        ["source_unknown no-such-source (metric #1 \"repo_forks\")"]
    );
}

#[test]
fn every_missing_field_is_named() {
    let expected: Vec<String> = ["title", "selector", "unit", "aggregation", "biases"]
        .iter()
        .map(|f| format!("field_missing {f} (metric #1 \"repo_stars\")"))
        .collect();
    assert_eq!(finding_lines("many-missing.toml", "sources.toml"), expected);
}

#[test]
fn mixed_error_paths() {
    let expected = [
        "duplicate_id id (source #2 \"repo-api\")",
        "field_invalid id (source #3 \"Crash-Reports\")",
        "field_invalid url (source #3 \"Crash-Reports\")",
        "source_not_public (source #3 \"Crash-Reports\")",
        "product_data_source crash (source #3 \"Crash-Reports\")",
        "source_not_public (source #4 \"updates\")",
        "product_data_source update-check (source #4 \"updates\")",
        "field_invalid method (metric #1 \"repo_stars\")",
        "field_invalid selector (metric #1 \"repo_stars\")",
        "field_invalid aggregation (metric #1 \"repo_stars\")",
        "field_invalid biases (metric #2 \"repo_stars\")",
        "duplicate_id id (metric #2 \"repo_stars\")",
    ];
    assert_eq!(finding_lines("mixed.toml", "mixed-sources.toml"), expected);
    let (code, _) = cli(&fixture("mixed.toml"), &fixture("mixed-sources.toml"), false);
    assert_eq!(code, 1);
}

#[test]
fn unparsable_or_unreadable_input_exits_2() {
    let (code, out) = cli(&fixture("duplicate-key.toml"), &fixture("sources.toml"), true);
    assert_eq!(code, 2);
    assert!(out.contains("\"code\":\"parse\"") && out.contains("\"status\":\"error\""), "{out}");
    let (code, _) = cli(&fixture("does-not-exist.toml"), &fixture("sources.toml"), false);
    assert_eq!(code, 2);
    let out = Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(["metric-def", "check"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn empty_files_name_the_missing_lists() {
    let r = check("", "d.toml", "", "s.toml").unwrap();
    let lines: Vec<String> = r.findings.iter().map(ToString::to_string).collect();
    assert_eq!(lines, ["field_missing source (sources)", "field_missing metric (definitions)"]);
}

#[test]
fn deny_list_matches_id_or_url_case_insensitively() {
    // Each pattern from the spec, upper-cased, in the ID and then in the URL.
    let spec = [
        "telemetry",
        "crash",
        "support-bundle",
        "support_bundle",
        "update-check",
        "activation",
        "licence-server",
        "license-server",
        "diagnostic",
    ];
    // Maintainers may add patterns, never remove: the spec's nine must stay.
    for p in spec {
        assert!(DENY_LIST.contains(&p), "{p}");
    }
    for p in spec {
        let up = p.to_ascii_uppercase();
        for (id, url) in [
            (format!("x-{p}"), "https://example.org/a".to_string()),
            ("x".to_string(), format!("https://example.org/{up}/a")),
        ] {
            let src = format!(
                "[[source]]\nid = \"{id}\"\nurl = \"{url}\"\npublic = true\npublic_basis = \"Claimed public.\"\n"
            );
            let r = check("[[metric]]\n", "d", &src, "s").unwrap();
            assert!(
                // `support_bundle` normalises to the same text as `support-bundle`, listed first.
                r.findings.iter().any(|f| f.code == Code::ProductDataSource
                    && f.subject.as_deref().map(normalise) == Some(normalise(p))),
                "{p}: {:?}",
                r.findings
            );
            assert!(r.sources.is_empty());
        }
    }
}

#[test]
fn json_report_is_deterministic() {
    let a = check_files(&fixture("mixed.toml"), &fixture("mixed-sources.toml")).unwrap().to_json();
    let b = check_files(&fixture("mixed.toml"), &fixture("mixed-sources.toml")).unwrap().to_json();
    assert_eq!(a, b);
}

/// Every pattern of the extended deny-list, with case, separator and percent-encoding variants.
#[test]
fn deny_list_fixture() {
    let expected = [
        ("d01", "telemetry"),
        ("d02", "crash"),
        ("d03", "support-bundle"),
        ("d04", "support-bundle"),
        ("d05", "support-bundle"),
        ("d06", "update-check"),
        ("activation-keys", "activation"),
        ("d08", "licence-server"),
        ("d09", "license-server"),
        ("d10", "diagnostic"),
        ("d11", "analytics"),
        ("d12", "usage-stats"),
        ("d13", "phone-home"),
        ("d14", "heartbeat"),
        ("d15", "error-report"),
        ("d16", "sentry"),
        ("d17", "licensing"),
        ("d18", "telemetry"),
    ];
    let lines = finding_lines("definitions.toml", "deny-list.toml");
    let want: Vec<String> = expected
        .iter()
        .enumerate()
        .map(|(n, (id, p))| format!("product_data_source {p} (source #{} \"{id}\")", n + 2))
        .collect();
    assert_eq!(lines, want);
    for p in [
        "analytics",
        "usage-stats",
        "phone-home",
        "heartbeat",
        "error-report",
        "sentry",
        "licensing",
        "update-check",
        "license-server",
    ] {
        assert!(DENY_LIST.contains(&p), "{p}");
    }
    let (code, _) = cli(&fixture("definitions.toml"), &fixture("deny-list.toml"), false);
    assert_eq!(code, 1);
}

#[test]
fn person_level_guard() {
    let expected = [
        "person_data selector login (metric #2 \"owner_login\")",
        "person_data selector author (metric #3 \"author_count\")",
        "person_data selector stargazers (metric #4 \"stargazer_list\")",
        "person_data selector email (metric #5 \"email\")",
        "person_data url contributors (metric #6 \"top_contributor\")",
        "person_data url committer (metric #7 \"committers\")",
        "person_data url author (metric #8 \"one_author_commits\")",
    ];
    assert_eq!(finding_lines("person-data.toml", "person-sources.toml"), expected);
    let r = check_files(&fixture("person-data.toml"), &fixture("person-sources.toml")).unwrap();
    assert_eq!(r.metrics.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["stars"]);
    let (code, _) = cli(&fixture("person-data.toml"), &fixture("person-sources.toml"), false);
    assert_eq!(code, 1);
}
