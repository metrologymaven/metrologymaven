//! Acceptance criteria of spec node PLAT.DEPLOY.OFFLINE.NETLIST (AC-1 .. AC-4). Expected
//! outputs come from the independent Python reference `fixtures/plat/deploy/make_expected.py`;
//! expected findings are hand-labelled from the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_xtask::plat_deploy_netlist::{cli_name, generate, is_off, NetlistError};
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/plat/deploy").join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fid-netlist-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs `fid-xtask plat-deploy netlist ...` from the workspace root; returns (exit code, stdout).
fn cli(manifest: &str, settings: &str, out: &Path, check: bool) -> (i32, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fid-xtask"));
    cmd.current_dir(root()).args(["plat-deploy", "netlist", "--manifest"]).arg(fixture(manifest));
    cmd.arg("--settings").arg(fixture(settings));
    cmd.arg("--json").arg(out.join("network-features.json"));
    cmd.arg("--md").arg(out.join("network-features.md"));
    if check {
        cmd.arg("--check");
    }
    let o = cmd.output().unwrap();
    (o.status.code().unwrap(), String::from_utf8(o.stdout).unwrap())
}

fn run_gen(manifest: &[u8], settings: &str) -> Result<fid_xtask::plat_deploy_netlist::Outputs, Vec<NetlistError>> {
    generate(manifest, "m.json", &std::fs::read(fixture(settings)).unwrap(), "s.json")
}

#[test]
fn ac1_fixture_output_matches_reference_and_lists_three_commands() {
    let dir = scratch("ac1");
    let (code, out) = cli("manifest-net.json", "settings-manifest.json", &dir, false);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.trim(), "netlist: 3 commands, written");
    for f in ["network-features.json", "network-features.md"] {
        assert_eq!(std::fs::read(dir.join(f)).unwrap(), std::fs::read(fixture(f)).unwrap(), "{f}");
    }
    // The committed reference outputs are up to date.
    let (code, out) = cli("manifest-net.json", "settings-manifest.json", &fixture(""), true);
    assert_eq!((code, out.trim()), (0, "netlist: 3 commands, up to date"));
}

#[test]
fn ac1_output_is_independent_of_manifest_order() {
    let text = std::fs::read_to_string(fixture("manifest-net.json")).unwrap();
    let a = run_gen(text.as_bytes(), "settings-manifest.json").unwrap();
    let mut doc = fid_canon::parse(text.as_bytes()).unwrap();
    if let CValue::Obj(o) = &mut doc {
        if let Some(CValue::Arr(c)) = o.get_mut("commands") {
            c.reverse();
        }
    }
    let b = run_gen(&fid_canon::to_canonical_bytes(&doc).unwrap(), "settings-manifest.json").unwrap();
    assert_eq!(a, b);
    assert_eq!(a.count, 3);
}

#[test]
fn ac2_x1_default_on_fails() {
    let dir = scratch("ac2");
    let (code, out) = cli("manifest-net.json", "settings-consent-on.json", &dir, false);
    assert_eq!(code, 1);
    assert_eq!(out.trim(), r#"X1DefaultOn { command: "update.check@1" }"#);
    assert!(!dir.join("network-features.json").exists());
}

#[test]
fn ac3_x1_without_opt_in_fails() {
    let dir = scratch("ac3");
    let (code, out) = cli("manifest-x1-no-optin.json", "settings-manifest.json", &dir, false);
    assert_eq!(code, 1);
    assert_eq!(out.trim(), r#"X1NoOptIn { command: "update.check@1" }"#);
}

#[test]
fn x1_with_undeclared_setting_fails() {
    let dir = scratch("unknown");
    let (code, out) = cli("manifest-x1-unknown-key.json", "settings-manifest.json", &dir, false);
    assert_eq!(code, 1);
    assert_eq!(out.trim(), r#"X1UnknownSetting { command: "update.check@1", key: "update.not_declared" }"#);
}

#[test]
fn ac4_hand_edited_page_fails_check() {
    let dir = scratch("ac4");
    for f in ["network-features.json", "network-features.md"] {
        std::fs::copy(fixture(f), dir.join(f)).unwrap();
    }
    assert_eq!(cli("manifest-net.json", "settings-manifest.json", &dir, true).0, 0);
    let md = dir.join("network-features.md");
    let edited = std::fs::read_to_string(&md).unwrap().replace("`\"off\"`", "`\"on\"`");
    std::fs::write(&md, edited).unwrap();
    let (code, out) = cli("manifest-net.json", "settings-manifest.json", &dir, true);
    assert_eq!(code, 1);
    assert!(out.starts_with("Drift { file: ") && out.contains("network-features.md"), "{out}");
    assert!(!out.contains("network-features.json"), "{out}");
    // A missing committed file is drift too.
    std::fs::remove_file(dir.join("network-features.json")).unwrap();
    let (code, out) = cli("manifest-net.json", "settings-manifest.json", &dir, true);
    assert_eq!((code, out.lines().count()), (1, 2), "{out}");
}

#[test]
fn malformed_manifest_is_manifest_parse() {
    for bad in [&b"{"[..], b"[]", br#"{"commands": [{"id": "x.y", "major": 1, "side_effect": "Z9"}]}"#] {
        match run_gen(bad, "settings-manifest.json") {
            Err(e) => assert!(matches!(e.as_slice(), [NetlistError::ManifestParse { .. }]), "{e:?}"),
            Ok(o) => panic!("accepted {o:?}"),
        }
    }
}

#[test]
fn no_network_commands_gives_empty_list() {
    let o = run_gen(
        br#"{"commands": [{"id": "align.run_bestfit", "major": 1, "side_effect": "W2"}]}"#,
        "settings-manifest.json",
    )
    .unwrap();
    assert_eq!(o.json, b"{\"commands\":[],\"format\":1}\n");
    assert!(o.md.ends_with("_No command in the registry manifest can use the network._\n"));
}

#[test]
fn off_values_and_cli_names_follow_the_spec() {
    assert!(is_off(&CValue::Bool(false)));
    assert!(is_off(&CValue::Str("off".into())));
    assert!(is_off(&CValue::Str("disabled".into())));
    assert!(is_off(&CValue::Arr(vec![])));
    assert!(!is_off(&CValue::Bool(true)));
    assert!(!is_off(&CValue::Str("on".into())));
    assert!(!is_off(&CValue::Arr(vec![CValue::Str("https://example.invalid".into())])));
    assert!(!is_off(&CValue::Null));
    assert_eq!(cli_name("align.run_bestfit", false), "fid align run-bestfit");
    assert_eq!(cli_name("doctor.run", true), "fid doctor");
}
