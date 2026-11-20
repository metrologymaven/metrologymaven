//! D-044: the `cargo deny` advisory exceptions for the egui UI spike stay scoped and dated.
//!
//! `real_workspace_*` checks this repository's `deny.toml` against its resolved graph (all
//! targets, all features) and today's date; the other tests pin each rule on hand-built
//! `cargo metadata` graphs. `FID_DENY_TODAY=YYYY-MM-DD` can only move the date later than the
//! system clock (`effective_today`), never keep an expired exception alive.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::deny_scope::{
    check, effective_today, expiry, parse_ignores, Date, EXCEPTIONS, LATEST_EXPIRY, SCOPED_ROOT,
};
use fid_xtask::Graph;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Today: the later of the system clock (UTC) and `FID_DENY_TODAY`, when set.
fn today() -> Date {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let system = Date::from_unix_days(i64::try_from(secs / 86_400).unwrap());
    let over = std::env::var("FID_DENY_TODAY")
        .ok()
        .map(|s| Date::parse(s.trim()).unwrap_or_else(|| panic!("FID_DENY_TODAY={s:?} is not YYYY-MM-DD")));
    effective_today(system, over)
}

#[test]
fn real_workspace_exceptions_are_scoped_and_unexpired() {
    let deny = std::fs::read_to_string(root().join("deny.toml")).unwrap();
    let g = Graph::load_all_targets(&root(), true).unwrap();
    let findings = check(&deny, &g, today()).unwrap();
    assert!(findings.is_empty(), "deny.toml exceptions (D-044) no longer hold:\n{}", findings.join("\n"));
}

#[test]
fn real_deny_toml_ignores_exactly_the_listed_advisories() {
    let deny = std::fs::read_to_string(root().join("deny.toml")).unwrap();
    let ids: Vec<String> = parse_ignores(&deny).unwrap().into_iter().map(|i| i.id).collect();
    let want: Vec<String> = EXCEPTIONS.iter().map(|(id, _, _)| id.to_string()).collect();
    assert_eq!(ids, want);
    for ig in parse_ignores(&deny).unwrap() {
        assert_eq!(expiry(ig.reason.as_deref().unwrap()), Some(LATEST_EXPIRY), "{}", ig.id);
    }
}

// ---- hand-built graphs ----

/// A `cargo metadata` document: packages (name, version), workspace members (names), and
/// edges (from, to, kind) where kind is "normal", "dev" or "build".
fn metadata(pkgs: &[(&str, &str)], members: &[&str], edges: &[(&str, &str, &str)]) -> Vec<u8> {
    let id = |n: &str| format!("{n} 0.0.0 (test)");
    let packages: Vec<String> = pkgs
        .iter()
        .map(|(n, v)| format!(r#"{{"id":"{}","name":"{n}","version":"{v}","dependencies":[]}}"#, id(n)))
        .collect();
    let members: Vec<String> = members.iter().map(|n| format!(r#""{}""#, id(n))).collect();
    let nodes: Vec<String> = pkgs
        .iter()
        .map(|(n, _)| {
            let deps: Vec<String> = edges
                .iter()
                .filter(|(f, _, _)| f == n)
                .map(|(_, t, k)| {
                    let kind = if *k == "normal" { "null".to_string() } else { format!(r#""{k}""#) };
                    format!(r#"{{"pkg":"{}","dep_kinds":[{{"kind":{kind}}}]}}"#, id(t))
                })
                .collect();
            format!(r#"{{"id":"{}","deps":[{}]}}"#, id(n), deps.join(","))
        })
        .collect();
    format!(
        r#"{{"packages":[{}],"workspace_members":[{}],"resolve":{{"nodes":[{}]}}}}"#,
        packages.join(","),
        members.join(","),
        nodes.join(",")
    )
    .into_bytes()
}

const PKGS: &[(&str, &str)] = &[
    ("fid-spike-ui", "0.0.1"),
    ("fid-hal-idl", "0.0.1"),
    ("fid-cli", "0.0.1"),
    ("egui", "0.33.3"),
    ("egui-winit", "0.33.3"),
    ("epaint", "0.33.3"),
    ("ttf-parser", "0.25.1"),
    ("accesskit_unix", "0.17.2"),
    ("zbus", "5.19.0"),
    ("zbus_names", "4.3.5"),
    ("zvariant", "5.15.1"),
    ("zvariant_derive", "5.15.1"),
];
const MEMBERS: &[&str] = &["fid-spike-ui", "fid-hal-idl", "fid-cli"];
const EDGES: &[(&str, &str, &str)] = &[
    ("fid-spike-ui", "fid-hal-idl", "normal"),
    ("fid-spike-ui", "egui", "dev"),
    ("fid-spike-ui", "egui-winit", "dev"),
    ("fid-cli", "fid-hal-idl", "normal"),
    ("egui", "epaint", "normal"),
    ("epaint", "ttf-parser", "normal"),
    ("egui-winit", "egui", "normal"),
    ("egui-winit", "accesskit_unix", "normal"),
    ("accesskit_unix", "zbus", "normal"),
    ("zbus", "zbus_names", "normal"),
    ("zbus", "zvariant", "normal"),
    ("zbus_names", "zvariant", "normal"),
    ("zvariant", "zvariant_derive", "build"),
];

fn entry(id: &str, crate_name: &str, expires: &str) -> String {
    format!(
        "    {{ id = \"{id}\", reason = \"{crate_name}: only via fid-spike-ui -> egui; unmaintained, not a vulnerability. D-044: expires {expires} or the egui >= 0.34 upgrade.\" }},\n"
    )
}

/// The non-ignore part of a compliant deny.toml.
const HEAD: &str = "[graph]\nall-features = true\n\n[advisories]\nversion = 2\nyanked = \"deny\"\n";

fn deny(expires: &str) -> String {
    let mut s = format!("{HEAD}ignore = [\n");
    for (id, crates, _) in EXCEPTIONS {
        s += &entry(id, crates[0], expires);
    }
    s + "]\n"
}

fn graph(pkgs: &[(&str, &str)], edges: &[(&str, &str, &str)]) -> Graph {
    Graph::from_metadata(&metadata(pkgs, MEMBERS, edges)).unwrap()
}

const DAY: Date = Date { y: 2026, m: 11, d: 19 };

#[test]
fn clean_graph_has_no_findings() {
    assert_eq!(check(&deny("2027-03-01"), &graph(PKGS, EDGES), DAY).unwrap(), Vec::<String>::new());
    // The last valid day is still fine.
    assert_eq!(check(&deny("2027-03-01"), &graph(PKGS, EDGES), LATEST_EXPIRY).unwrap(), Vec::<String>::new());
}

#[test]
fn another_workspace_crate_reaching_an_advisory_crate_fails() {
    let mut edges = EDGES.to_vec();
    edges.push(("fid-hal-idl", "zbus", "normal"));
    let got = check(&deny("2027-03-01"), &graph(PKGS, &edges), DAY).unwrap();
    assert_eq!(
        got,
        [
            // The spike reaches fid-hal-idl too, so its path now also skips accesskit_unix.
            "RUSTSEC-2026-0370: bypass_path fid-spike-ui -> fid-hal-idl -> zbus -> zvariant (must go through accesskit_unix)",
            "RUSTSEC-2026-0371: bypass_path fid-spike-ui -> fid-hal-idl -> zbus -> zbus_names (must go through accesskit_unix)",
            "RUSTSEC-2026-0372: bypass_path fid-spike-ui -> fid-hal-idl -> zbus -> zvariant -> zvariant_derive (must go through accesskit_unix)",
            "fid-cli: escaped_scope fid-cli -> fid-hal-idl -> zbus -> zbus_names",
            "fid-hal-idl: escaped_scope fid-hal-idl -> zbus -> zbus_names",
        ]
    );
}

#[test]
fn dev_and_build_edges_count() {
    let mut edges = EDGES.to_vec();
    edges.push(("fid-cli", "epaint", "dev"));
    let got = check(&deny("2027-03-01"), &graph(PKGS, &edges), DAY).unwrap();
    assert_eq!(got, ["fid-cli: escaped_scope fid-cli -> epaint -> ttf-parser"]);
    let mut edges = EDGES.to_vec();
    edges.push(("fid-cli", "zvariant_derive", "build"));
    let got = check(&deny("2027-03-01"), &graph(PKGS, &edges), DAY).unwrap();
    assert_eq!(got, ["fid-cli: escaped_scope fid-cli -> zvariant_derive"]);
}

#[test]
fn depending_on_the_spike_itself_fails() {
    let mut edges = EDGES.to_vec();
    edges.push(("fid-cli", "fid-spike-ui", "dev"));
    let got = check(&deny("2027-03-01"), &graph(PKGS, &edges), DAY).unwrap();
    assert_eq!(got, ["fid-cli: escaped_scope fid-cli -> fid-spike-ui -> egui -> epaint -> ttf-parser"]);
}

#[test]
fn passed_expiry_fails() {
    let after = Date { y: 2027, m: 3, d: 2 };
    let got = check(&deny("2027-03-01"), &graph(PKGS, EDGES), after).unwrap();
    let want: Vec<String> =
        EXCEPTIONS.iter().map(|(id, _, _)| format!("{id}: expired on 2027-03-01 (today 2027-03-02)")).collect();
    assert_eq!(got, want);
}

#[test]
fn expiry_past_the_limit_fails() {
    let got = check(&deny("2027-03-02"), &graph(PKGS, EDGES), DAY).unwrap();
    let want: Vec<String> = EXCEPTIONS
        .iter()
        .map(|(id, _, _)| format!("{id}: expired date 2027-03-02 is past the D-044 limit 2027-03-01"))
        .collect();
    assert_eq!(got, want);
}

#[test]
fn egui_034_ends_every_exception() {
    let pkgs: Vec<(&str, &str)> = PKGS.iter().map(|&(n, v)| if n == "egui" { (n, "0.34.0") } else { (n, v) }).collect();
    let got = check(&deny("2027-03-01"), &graph(&pkgs, EDGES), DAY).unwrap();
    let want: Vec<String> =
        EXCEPTIONS.iter().map(|(id, _, _)| format!("{id}: egui_upgraded egui >= 0.34 is in the graph")).collect();
    assert_eq!(got, want);
}

#[test]
fn unlisted_ignore_fails() {
    let d = deny("2027-03-01").strip_suffix("]\n").unwrap().to_string()
        + &entry("RUSTSEC-2026-9999", "wgpu", "2027-03-01")
        + "]\n";
    let got = check(&d, &graph(PKGS, EDGES), DAY).unwrap();
    assert_eq!(got, ["RUSTSEC-2026-9999: unlisted_ignore not in fid_xtask::deny_scope::EXCEPTIONS"]);
}

#[test]
fn reasons_must_be_complete() {
    let base = format!("{HEAD}ignore = [\n");
    let bare = format!("{base}\"RUSTSEC-2026-0192\"]\n");
    assert_eq!(check(&bare, &graph(PKGS, EDGES), DAY).unwrap(), ["RUSTSEC-2026-0192: bad_reason missing"]);

    let vague = format!("{base}{{ id = \"RUSTSEC-2026-0192\", reason = \"egui stuff\" }}]\n");
    assert_eq!(
        check(&vague, &graph(PKGS, EDGES), DAY).unwrap(),
        [
            "RUSTSEC-2026-0192: bad_reason does not name ttf-parser",
            "RUSTSEC-2026-0192: bad_reason lacks \"only via fid-spike-ui\"",
            "RUSTSEC-2026-0192: bad_reason lacks \"unmaintained, not a vulnerability\"",
            "RUSTSEC-2026-0192: bad_reason needs exactly one \"expires YYYY-MM-DD\"",
        ]
    );

    let two = format!(
        "{base}{{ id = \"RUSTSEC-2026-0192\", reason = \"ttf-parser: only via fid-spike-ui; unmaintained, not a vulnerability; expires 2027-01-01, expires 2027-02-01\" }}]\n"
    );
    assert_eq!(
        check(&two, &graph(PKGS, EDGES), DAY).unwrap(),
        ["RUSTSEC-2026-0192: bad_reason needs exactly one \"expires YYYY-MM-DD\""]
    );
}

#[test]
fn crate_gone_from_graph_is_a_stale_ignore() {
    let pkgs: Vec<(&str, &str)> = PKGS.iter().copied().filter(|(n, _)| *n != "ttf-parser").collect();
    let edges: Vec<(&str, &str, &str)> = EDGES.iter().copied().filter(|(_, t, _)| *t != "ttf-parser").collect();
    let got = check(&deny("2027-03-01"), &graph(&pkgs, &edges), DAY).unwrap();
    assert_eq!(got, ["ttf-parser: stale_ignore RUSTSEC-2026-0192 names a crate no longer in the graph"]);
}

#[test]
fn malformed_deny_toml_is_an_error() {
    let dup = "[advisories]\nignore = [\"RUSTSEC-2026-0192\", \"RUSTSEC-2026-0192\"]\n";
    assert!(parse_ignores(dup).unwrap_err().contains("duplicate"));
    assert!(parse_ignores("[advisories]\nignore = [1]\n").is_err());
    assert!(parse_ignores("[advisories]\nignore = [{ reason = \"x\" }]\n").is_err());
    assert!(parse_ignores("[advisories\n").is_err());
}

#[test]
fn scoped_root_is_the_spike() {
    assert_eq!(SCOPED_ROOT, "fid-spike-ui");
}

#[test]
fn dates() {
    assert_eq!(Date::from_unix_days(0), Date { y: 1970, m: 1, d: 1 });
    assert_eq!(Date::from_unix_days(19_782), Date { y: 2024, m: 2, d: 29 });
    assert_eq!(Date::from_unix_days(20_000), Date { y: 2024, m: 10, d: 4 });
    assert_eq!(Date::from_unix_days(-1), Date { y: 1969, m: 12, d: 31 });
    assert_eq!(Date::parse("2027-03-01"), Some(LATEST_EXPIRY));
    assert_eq!(Date::parse("2028-02-29"), Some(Date { y: 2028, m: 2, d: 29 }));
    for bad in ["2027-02-29", "2027-13-01", "2027-00-10", "2027-04-31", "2027-3-01", "2027/03/01", "+027-03-01"] {
        assert_eq!(Date::parse(bad), None, "{bad}");
    }
    assert!(Date { y: 2027, m: 3, d: 2 } > LATEST_EXPIRY);
    assert_eq!(LATEST_EXPIRY.to_string(), "2027-03-01");
}

#[test]
fn earlier_date_override_cannot_keep_an_expired_exception_alive() {
    let system = Date { y: 2027, m: 3, d: 2 };
    let earlier = Some(Date { y: 2026, m: 1, d: 1 });
    assert_eq!(effective_today(system, earlier), system);
    let got = check(&deny("2027-03-01"), &graph(PKGS, EDGES), effective_today(system, earlier)).unwrap();
    assert_eq!(got.len(), EXCEPTIONS.len(), "{got:?}");
    assert!(got.iter().all(|f| f.contains(": expired on 2027-03-01 (today 2027-03-02)")), "{got:?}");
    // A later override does move the date forward.
    assert_eq!(effective_today(DAY, Some(Date { y: 2030, m: 1, d: 1 })), Date { y: 2030, m: 1, d: 1 });
    assert_eq!(effective_today(DAY, None), DAY);
}

#[test]
fn other_relaxations_beside_the_ignores_fail() {
    let ok = deny("2027-03-01");
    let g = graph(PKGS, EDGES);
    assert_eq!(check(&ok, &g, DAY).unwrap(), Vec::<String>::new());
    let beside = " is not allowed beside the D-044 exceptions";
    let cases = [
        ("[advisories]\n", "[advisories]\nunmaintained = \"workspace\"\n", format!("advisories.unmaintained{beside}")),
        ("[advisories]\n", "[advisories]\nunsound = \"workspace\"\n", format!("advisories.unsound{beside}")),
        ("[graph]\n", "[graph]\nexclude = [\"fid-cli\"]\n", format!("graph.exclude{beside}")),
        ("[graph]\n", "[graph]\nexclude-dev = true\n", format!("graph.exclude-dev{beside}")),
        ("[graph]\n", "[graph]\ntargets = [\"x86_64-pc-windows-msvc\"]\n", format!("graph.targets{beside}")),
        ("yanked = \"deny\"", "yanked = \"warn\"", "advisories.yanked must be \"deny\"".to_string()),
        ("all-features = true", "all-features = false", "graph.all-features must be true".to_string()),
    ];
    for (from, to, want) in cases {
        let d = ok.replacen(from, to, 1);
        assert_ne!(d, ok, "{to}");
        assert_eq!(check(&d, &g, DAY).unwrap(), [format!("deny.toml: relaxed_config {want}")], "{to}");
    }
}

#[test]
fn spike_paths_must_go_through_the_granted_crate() {
    let mut edges = EDGES.to_vec();
    edges.push(("egui-winit", "zbus", "normal"));
    edges.push(("fid-spike-ui", "ttf-parser", "dev"));
    let got = check(&deny("2027-03-01"), &graph(PKGS, &edges), DAY).unwrap();
    assert_eq!(
        got,
        [
            "RUSTSEC-2026-0192: bypass_path fid-spike-ui -> ttf-parser (must go through epaint)",
            "RUSTSEC-2026-0370: bypass_path fid-spike-ui -> egui-winit -> zbus -> zvariant (must go through accesskit_unix)",
            "RUSTSEC-2026-0371: bypass_path fid-spike-ui -> egui-winit -> zbus -> zbus_names (must go through accesskit_unix)",
            "RUSTSEC-2026-0372: bypass_path fid-spike-ui -> egui-winit -> zbus -> zvariant -> zvariant_derive (must go through accesskit_unix)",
        ]
    );
}
