//! Acceptance criteria and error-path tests for `AUTO.MCP.SRV` (AC-1..AC-7).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_mcp::profiles::{
    check_all_served, emit_profiles, parse_profile_bytes, read_profile_file, resolve_profiles, ProfileError,
    ProfileIndex, CORE_PROFILE_TOML, EXIT_USAGE, MAX_FRONT_DOOR, MAX_PROFILE_BYTES,
};
use fid_surface::project;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("fixtures").join("mcp").join("profiles")
}

fn load_projection(file: &str) -> fid_surface::Projection {
    let bytes = fs::read(fixtures_dir().join(file)).expect("read manifest fixture");
    project(&bytes).expect("manifest fixture must project cleanly")
}

#[test]
fn ac1_emits_canonical_mcp_profiles_matching_golden_without_m1_s1_a0() {
    let proj = load_projection("manifest.json");
    let core = read_profile_file(&fixtures_dir().join("core.toml")).expect("parse core.toml");
    let inspect = read_profile_file(&fixtures_dir().join("inspect.toml")).expect("parse inspect.toml");

    // Pass profiles in reversed order to also confirm deterministic sorting by profile name.
    let emitted = emit_profiles(&proj, &[inspect, core]).expect("emit_profiles must succeed");

    let expected = fs::read(fixtures_dir().join("expected.json")).expect("read expected.json");
    assert_eq!(
        String::from_utf8_lossy(&emitted.bytes),
        String::from_utf8_lossy(&expected),
        "generated mcp-profiles.json must match fixtures/mcp/profiles/expected.json byte for byte"
    );

    // No M1, S1 or A0 command may appear in any served set (CONSTITUTION §3, AC-1).
    for profile in &emitted.index.profiles {
        for forbidden in ["motion.move_axis@1", "trust.sign_record@1", "plugin.install_pack@1"] {
            assert!(
                !profile.served.iter().any(|id| id.as_str() == forbidden),
                "profile `{}` must never serve `{forbidden}`",
                profile.name
            );
        }
        assert!(profile.front_door.len() <= MAX_FRONT_DOOR);
    }

    // Round-trip canonical JSON loader.
    let reloaded = ProfileIndex::from_canonical_bytes(&emitted.bytes).expect("reload canonical index");
    assert_eq!(reloaded, emitted.index);
}

#[test]
fn ac2_sixteen_front_door_entries_fails_profile_invalid_naming_profile_and_sixteenth() {
    let proj = load_projection("manifest-many.json");
    let too_many = read_profile_file(&fixtures_dir().join("too-many.toml")).expect("parse too-many.toml");

    let err = resolve_profiles(&proj, &[too_many]).expect_err("16 front-door tools must fail");
    assert_eq!(err.code(), "profile_invalid");
    assert_eq!(err.profile(), Some("too_many"));
    assert_eq!(err.entry(), Some("demo.cmd_12@1"));
}

#[test]
fn ac3_missing_disclosure_command_fails_profile_invalid() {
    let proj = load_projection("manifest.json");
    let missing =
        read_profile_file(&fixtures_dir().join("missing-disclosure.toml")).expect("parse missing-disclosure.toml");

    let err = resolve_profiles(&proj, &[missing]).expect_err("missing tools.search_tools@1 must fail");
    assert_eq!(err.code(), "profile_invalid");
    assert_eq!(err.profile(), Some("missing_disc"));
    assert_eq!(err.entry(), Some("tools.search_tools@1"));
}

#[test]
fn ac4_m1_command_in_front_door_fails_profile_invalid_naming_it() {
    let proj = load_projection("manifest-m1-demo.json");
    let bad = read_profile_file(&fixtures_dir().join("m1-front-door.toml")).expect("parse m1-front-door.toml");

    let err = resolve_profiles(&proj, &[bad]).expect_err("M1 demo.move_axis@1 in front_door must fail");
    assert_eq!(err.code(), "profile_invalid");
    assert_eq!(err.profile(), Some("bad_motion"));
    assert_eq!(err.entry(), Some("demo.move_axis@1"));
}

#[test]
fn ac5_unserved_projectable_command_fails_command_unserved() {
    let proj = load_projection("manifest-orphan.json");
    let core = read_profile_file(&fixtures_dir().join("core.toml")).expect("parse core.toml");
    let inspect = read_profile_file(&fixtures_dir().join("inspect.toml")).expect("parse inspect.toml");

    let index = resolve_profiles(&proj, &[core.clone(), inspect.clone()]).expect("individual profiles valid");
    let err = check_all_served(&proj, &index).expect_err("orphan command must fail CI completeness check");
    assert_eq!(err.code(), "command_unserved");
    assert_eq!(err.entry(), Some("demo.orphan@1"));

    let err2 = emit_profiles(&proj, &[core, inspect]).expect_err("emit_profiles must enforce check_all_served");
    assert_eq!(err2, err);
}

#[test]
fn ac6_cli_unknown_profile_exits_2_lists_known_and_writes_no_stdout() {
    let bin = env!("CARGO_BIN_EXE_fid-mcp");
    let index_path = fixtures_dir().join("expected.json");
    let manifest_path = fixtures_dir().join("manifest.json");

    let out = Command::new(bin)
        .args([
            "--profiles-index",
            index_path.to_str().expect("utf-8 path"),
            "--manifest",
            manifest_path.to_str().expect("utf-8 path"),
            "--server",
            "nosuch",
        ])
        .output()
        .expect("run fid-mcp");

    assert_eq!(out.status.code(), Some(i32::from(EXIT_USAGE)));
    assert!(out.stdout.is_empty(), "no MCP traffic may be written to stdout, got: {:?}", out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown_profile"), "stderr must contain unknown_profile: {stderr}");
    assert!(stderr.contains("core"), "stderr must list `core`: {stderr}");
    assert!(stderr.contains("inspect"), "stderr must list `inspect`: {stderr}");
}

#[test]
fn cli_default_server_selects_core_and_named_server_selects_inspect() {
    let bin = env!("CARGO_BIN_EXE_fid-mcp");
    let index_path = fixtures_dir().join("expected.json");
    let manifest_path = fixtures_dir().join("manifest.json");

    let default_out = Command::new(bin)
        .args([
            "--profiles-index",
            index_path.to_str().expect("utf-8 path"),
            "--manifest",
            manifest_path.to_str().expect("utf-8 path"),
            "--print-profile",
        ])
        .output()
        .expect("run fid-mcp default");
    assert!(default_out.status.success(), "stderr: {}", String::from_utf8_lossy(&default_out.stderr));
    let stdout = String::from_utf8_lossy(&default_out.stdout);
    assert!(stdout.contains("profile=core "), "default must select core, got: {stdout}");

    let inspect_out = Command::new(bin)
        .args([
            "--profiles-index",
            index_path.to_str().expect("utf-8 path"),
            "--manifest",
            manifest_path.to_str().expect("utf-8 path"),
            "--server=inspect",
            "--print-profile",
        ])
        .output()
        .expect("run fid-mcp inspect");
    assert!(inspect_out.status.success(), "stderr: {}", String::from_utf8_lossy(&inspect_out.stderr));
    let stdout2 = String::from_utf8_lossy(&inspect_out.stdout);
    assert!(stdout2.contains("profile=inspect "), "must select inspect, got: {stdout2}");
}

#[test]
fn ac7_determinism_independent_of_input_order_and_crlf() {
    let proj = load_projection("manifest.json");
    let core_lf = fs::read_to_string(fixtures_dir().join("core.toml")).expect("read core.toml");
    let inspect_lf = fs::read_to_string(fixtures_dir().join("inspect.toml")).expect("read inspect.toml");

    // Simulate Windows CRLF line endings on profile files.
    let core_crlf = core_lf.replace('\n', "\r\n");
    let inspect_crlf = inspect_lf.replace('\n', "\r\n");

    let run_lf = emit_profiles(
        &proj,
        &[
            parse_profile_bytes("core.toml", core_lf.as_bytes()).expect("parse lf core"),
            parse_profile_bytes("inspect.toml", inspect_lf.as_bytes()).expect("parse lf inspect"),
        ],
    )
    .expect("emit lf");

    let run_crlf = emit_profiles(
        &proj,
        &[
            parse_profile_bytes("inspect.toml", inspect_crlf.as_bytes()).expect("parse crlf inspect"),
            parse_profile_bytes("core.toml", core_crlf.as_bytes()).expect("parse crlf core"),
        ],
    )
    .expect("emit crlf");

    assert_eq!(run_lf.bytes, run_crlf.bytes);
}

#[test]
fn error_paths_oversized_unknown_key_unserved_front_door_and_duplicate_profile() {
    // > 64 KiB rejected before parsing.
    let huge = vec![b' '; MAX_PROFILE_BYTES + 1];
    let err = parse_profile_bytes("huge.toml", &huge).expect_err("oversized profile must fail");
    assert_eq!(err.code(), "profile_parse");

    // Unknown extra key rejected (additionalProperties: false).
    let extra_key = format!("{CORE_PROFILE_TOML}\nextra_field = true\n");
    let err = parse_profile_bytes("extra.toml", extra_key.as_bytes()).expect_err("extra key must fail");
    assert_eq!(err.code(), "profile_parse");

    // Front-door command not in profile's served set fails profile_invalid.
    let proj = load_projection("manifest.json");
    let unserved_fd = r#"
format = "fid.mcp-profile"
version = 1
name = "unserved_fd"
title = "Unserved Front Door"
domains = ["tools"]
front_door = [
  "tools.search_tools@1",
  "tools.describe_tool@1",
  "tools.list_domains@1",
  "tools.get_workflow@1",
  "align.run_bestfit@1",
]
"#;
    let spec = parse_profile_bytes("unserved_fd.toml", unserved_fd.as_bytes()).expect("syntax valid");
    let err = resolve_profiles(&proj, &[spec]).expect_err("front_door not served must fail");
    assert_eq!(err.code(), "profile_invalid");
    assert_eq!(err.profile(), Some("unserved_fd"));
    assert_eq!(err.entry(), Some("align.run_bestfit@1"));

    // Duplicate profile names fail profile_invalid.
    let core = read_profile_file(&fixtures_dir().join("core.toml")).expect("parse core");
    let err = resolve_profiles(&proj, &[core.clone(), core]).expect_err("duplicate profile must fail");
    assert_eq!(err.code(), "profile_invalid");
    assert!(matches!(err, ProfileError::ProfileInvalid { ref profile, .. } if profile == "core"));
}

#[test]
fn tier5_tampered_profiles_index_rejected_for_digest_front_door_and_missing_tools() {
    let proj = load_projection("manifest.json");
    let expected = fs::read_to_string(fixtures_dir().join("expected.json")).expect("read expected.json");
    let parsed = ProfileIndex::from_canonical_bytes(expected.as_bytes()).expect("valid expected.json");
    assert!(parsed.validate_against(&proj).is_ok());

    // Wrong manifest_digest must fail validate_against.
    let bad_digest = expected.replacen(
        &proj.manifest_digest,
        "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        1,
    );
    let idx_bad_digest = ProfileIndex::from_canonical_bytes(bad_digest.as_bytes()).expect("syntactically valid");
    assert_eq!(idx_bad_digest.validate_against(&proj).expect_err("mismatched digest").code(), "profile_invalid");

    // > MAX_FRONT_DOOR entries in front_door must be rejected.
    let mut big_fd = Vec::new();
    for i in 0..=MAX_FRONT_DOOR {
        big_fd.push(format!("\"fake.cmd_{i}@1\""));
    }
    let tampered_fd = expected.replacen("\"front_door\":[", &format!("\"front_door\":[{},", big_fd.join(",")), 1);
    let err = ProfileIndex::from_canonical_bytes(tampered_fd.as_bytes()).expect_err("over-cap front_door");
    assert_eq!(err.code(), "profile_invalid");

    // Front door entry outside served must be rejected.
    let unserved_fd = expected.replacen("\"front_door\":[", "\"front_door\":[\"motion.move_axis@1\",", 1);
    let err = ProfileIndex::from_canonical_bytes(unserved_fd.as_bytes()).expect_err("unserved front_door");
    assert_eq!(err.code(), "profile_invalid");

    // Missing required discovery tool in served must be rejected.
    let missing_discovery = expected.replacen("\"tools.search_tools@1\",", "", 1);
    let err = ProfileIndex::from_canonical_bytes(missing_discovery.as_bytes()).expect_err("missing search_tools");
    assert_eq!(err.code(), "profile_invalid");
}

#[test]
fn tier5_cli_empty_flag_values_rejected_for_both_forms() {
    let bin = env!("CARGO_BIN_EXE_fid-mcp");
    for args in [
        vec!["--server", ""],
        vec!["--server="],
        vec!["--profiles-index", ""],
        vec!["--profiles-index="],
        vec!["--manifest", ""],
        vec!["--manifest="],
        vec!["--project", ""],
        vec!["--project="],
        vec!["--agent", ""],
        vec!["--agent="],
    ] {
        let out = Command::new(bin).args(&args).output().expect("run fid-mcp");
        assert_eq!(
            out.status.code(),
            Some(i32::from(EXIT_USAGE)),
            "args={args:?} stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.is_empty(), "args={args:?} wrote stdout");
    }
}
