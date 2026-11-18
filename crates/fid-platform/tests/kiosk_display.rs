//! Acceptance criteria of spec node PLAT.DEPLOY.KIOSK.DISPLAY (AC-1 .. AC-3) and the
//! requirement edge cases. Expected diagnostics are hand-labelled from the spec's rules (the
//! fixtures in `fixtures/plat/kiosk/` state their expected outcome in their header comments);
//! expected pixel densities come from the independent Python reference
//! `fixtures/plat/kiosk/make_px_per_mm_cases.py`. None is produced by running this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_platform::kiosk_display::{parse, validate, DisplayError, KioskDisplay, Touch, MAX_BYTES};
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/plat/kiosk").join(name)
}

/// Runs `fid-platform plat-kiosk display [--file <path>]` from the repository root.
fn cli(file: Option<PathBuf>) -> (i32, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fid-platform"));
    cmd.current_dir(root()).args(["plat-kiosk", "display"]);
    if let Some(f) = file {
        cmd.arg("--file").arg(f);
    }
    let out = cmd.output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

const BASE: &str = r#"schema_version = 1
diagonal_in = 21.5
resolution = [1920, 1080]
touch = "projected_capacitive"
glove_touch = true
description = "test panel"
"#;

/// `BASE` with the line starting with `key =` replaced by `line` (or removed if empty).
fn with(key: &str, line: &str) -> String {
    let prefix = format!("{key} =");
    let mut out = String::new();
    for l in BASE.lines() {
        if l.starts_with(&prefix) {
            if !line.is_empty() {
                out.push_str(line);
                out.push('\n');
            }
        } else {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

fn errs(text: &str) -> Vec<DisplayError> {
    validate(text.as_bytes()).unwrap_err()
}

fn out_of_range_field(e: &DisplayError) -> Option<&str> {
    match e {
        DisplayError::OutOfRange { field, .. } => Some(field.as_str()),
        _ => None,
    }
}

#[test]
fn ac1_shipped_file_prints_px_per_mm() {
    let (code, out) = cli(None);
    assert_eq!(code, 0, "{out}");
    // The spec's AC-1 proposes 4.040 [unverified arithmetic]; the independent Python
    // reference gives 4.034 (see px-per-mm-cases.toml, first case).
    assert_eq!(out, "px_per_mm = 4.034\n");
}

#[test]
fn req004_shipped_file_describes_the_proposed_panel() {
    let bytes = std::fs::read(root().join("platform/kiosk-display.toml")).unwrap();
    let d = parse(&bytes).unwrap();
    assert_eq!(d.schema_version, 1);
    assert_eq!(d.diagonal_in, 21.5);
    assert_eq!(d.resolution, (1920, 1080));
    assert_eq!(d.touch, Touch::ProjectedCapacitive);
    assert!(d.glove_touch);
    assert!(!d.description.is_empty());
    assert_eq!(d.px_per_mm(), 4.034);
}

#[test]
fn ac2_tiny_diagonal_is_out_of_range() {
    let (code, out) = cli(Some(fixture("display-tiny.toml")));
    assert_eq!(code, 1, "{out}");
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(out.starts_with("OutOfRange { field: diagonal_in }"), "{out}");
}

#[test]
fn ac3_extra_field_is_unknown() {
    let (code, out) = cli(Some(fixture("display-extra.toml")));
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "UnknownField { field: brightness }\n");
}

#[test]
fn req003_bad_resolution_fixture() {
    let (code, out) = cli(Some(fixture("display-bad-resolution.toml")));
    assert_eq!(code, 1, "{out}");
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(out.starts_with("OutOfRange { field: resolution }"), "{out}");
}

#[test]
fn cli_bad_usage_and_unreadable_file_exit_2() {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-platform")).args(["plat-kiosk", "show"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let (code, _) = cli(Some(fixture("no-such-file.toml")));
    assert_eq!(code, 2);
}

#[test]
fn req002_px_per_mm_matches_python_reference() {
    let text = std::fs::read_to_string(fixture("px-per-mm-cases.toml")).unwrap();
    let table: toml::Table = text.parse().unwrap();
    let cases = table["case"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    for c in cases {
        let diagonal_in = c["diagonal_in"].as_float().unwrap();
        let r = c["resolution"].as_array().unwrap();
        let w = u32::try_from(r[0].as_integer().unwrap()).unwrap();
        let h = u32::try_from(r[1].as_integer().unwrap()).unwrap();
        let expected = c["expected"].as_str().unwrap();
        let d = KioskDisplay {
            schema_version: 1,
            diagonal_in,
            resolution: (w, h),
            touch: Touch::None,
            glove_touch: false,
            description: "case".into(),
        };
        assert_eq!(d.px_per_mm_text(), expected, "{diagonal_in} {w}x{h}");
        assert_eq!(d.px_per_mm(), expected.parse::<f64>().unwrap());
        // The same case through the file parser.
        let file = with("diagonal_in", &format!("diagonal_in = {diagonal_in:?}"));
        let file = file.replace("resolution = [1920, 1080]", &format!("resolution = [{w}, {h}]"));
        assert_eq!(parse(file.as_bytes()).unwrap().px_per_mm_text(), expected);
    }
}

#[test]
fn req001_base_parses_with_every_touch_value() {
    const ALL: [(&str, Touch); 3] =
        [("projected_capacitive", Touch::ProjectedCapacitive), ("resistive", Touch::Resistive), ("none", Touch::None)];
    for (name, t) in ALL {
        let d = parse(with("touch", &format!("touch = \"{name}\"")).as_bytes()).unwrap();
        assert_eq!(d.touch, t);
        assert_eq!(t.as_str(), name);
    }
}

#[test]
fn req003_diagonal_bounds_are_inclusive() {
    for ok in ["10", "10.0", "43", "43.0", "21.5"] {
        assert!(validate(with("diagonal_in", &format!("diagonal_in = {ok}")).as_bytes()).is_ok(), "{ok}");
    }
    for bad in ["9.99", "43.01", "0", "-21.5", "nan", "inf", "-inf", "100000000000"] {
        let e = errs(&with("diagonal_in", &format!("diagonal_in = {bad}")));
        assert_eq!(e.len(), 1, "{bad}: {e:?}");
        assert_eq!(out_of_range_field(&e[0]), Some("diagonal_in"), "{bad}");
    }
}

#[test]
fn req003_resolution_bounds_are_inclusive() {
    for ok in ["[800, 800]", "[7680, 7680]", "[800, 7680]"] {
        assert!(validate(with("resolution", &format!("resolution = {ok}")).as_bytes()).is_ok(), "{ok}");
    }
    for bad in ["[799, 1080]", "[1920, 7681]", "[0, 0]", "[-1920, 1080]", "[1920, 99999999999]"] {
        let e = errs(&with("resolution", &format!("resolution = {bad}")));
        assert_eq!(e.len(), 1, "{bad}: {e:?}");
        assert_eq!(out_of_range_field(&e[0]), Some("resolution"), "{bad}");
    }
    for shape in ["[1920]", "[1920, 1080, 1]", "[1920.0, 1080.0]", "\"1920x1080\"", "[]"] {
        let e = errs(&with("resolution", &format!("resolution = {shape}")));
        assert_eq!(e.len(), 1, "{shape}: {e:?}");
        assert!(
            matches!(&e[0], DisplayError::Parse { line: None, msg } if msg.starts_with("resolution ")),
            "{shape}"
        );
    }
}

#[test]
fn unknown_touch_and_wrong_types_are_parse_errors() {
    let cases = [
        ("touch", "touch = \"surface_acoustic_wave\"", "touch "),
        ("glove_touch", "glove_touch = \"yes\"", "glove_touch "),
        ("description", "description = \"  \"", "description "),
        ("description", "description = 3", "description "),
        ("diagonal_in", "diagonal_in = \"21.5\"", "diagonal_in "),
        ("schema_version", "schema_version = \"1\"", "schema_version "),
    ];
    for (key, line, prefix) in cases {
        let e = errs(&with(key, line));
        assert_eq!(e.len(), 1, "{line}: {e:?}");
        assert!(matches!(&e[0], DisplayError::Parse { line: None, msg } if msg.starts_with(prefix)), "{line}: {e:?}");
    }
}

#[test]
fn missing_fields_are_reported_in_field_order() {
    let mut text = String::from("schema_version = 1\n");
    text.push_str("touch = \"none\"\n");
    let e = errs(&text);
    let msgs: Vec<String> = e.iter().map(ToString::to_string).collect();
    assert_eq!(
        msgs,
        [
            "Parse: diagonal_in is missing",
            "Parse: resolution is missing",
            "Parse: glove_touch is missing",
            "Parse: description is missing",
        ]
    );
}

#[test]
fn unknown_fields_come_first_in_key_order() {
    let text = format!("{}zeta = 1\nalpha = 2\n", with("diagonal_in", "diagonal_in = 5"));
    let e = errs(&text);
    assert_eq!(e[0], DisplayError::UnknownField { field: "alpha".into() });
    assert_eq!(e[1], DisplayError::UnknownField { field: "zeta".into() });
    assert_eq!(out_of_range_field(&e[2]), Some("diagonal_in"));
    assert_eq!(e.len(), 3);
}

#[test]
fn schema_version_other_than_1_is_out_of_range() {
    let e = errs(&with("schema_version", "schema_version = 2"));
    assert_eq!(e.len(), 1);
    assert_eq!(out_of_range_field(&e[0]), Some("schema_version"));
    let e = errs(&with("schema_version", ""));
    assert_eq!(e, [DisplayError::Parse { line: None, msg: "schema_version is missing".into() }]);
}

#[test]
fn too_large_not_utf8_and_bad_toml_are_fatal() {
    let mut big = BASE.to_string();
    while big.len() <= MAX_BYTES {
        big.push_str("# padding padding padding padding padding padding padding padding\n");
    }
    assert_eq!(errs(&big), [DisplayError::TooLarge { found: big.len(), max: MAX_BYTES }]);
    assert_eq!(MAX_BYTES, 16 * 1024);
    // Exactly at the cap is accepted.
    let mut at_cap = BASE.to_string();
    while at_cap.len() < MAX_BYTES {
        at_cap.push('\n');
    }
    assert_eq!(at_cap.len(), MAX_BYTES);
    assert!(validate(at_cap.as_bytes()).is_ok());

    let e = validate(&[0xff, 0xfe, 0x00]).unwrap_err();
    assert!(matches!(&e[..], [DisplayError::Parse { line: None, .. }]), "{e:?}");

    let e = errs("schema_version = 1\ndiagonal_in = = 3\n");
    assert!(matches!(&e[..], [DisplayError::Parse { line: Some(2), .. }]), "{e:?}");

    let e = errs("schema_version = 1\nschema_version = 1\n");
    assert!(matches!(&e[..], [DisplayError::Parse { line: Some(_), .. }]), "{e:?}");
}

#[test]
fn diagnostics_render_with_field_names() {
    let e = DisplayError::OutOfRange { field: "diagonal_in".into(), msg: "5 is outside".into() };
    assert_eq!(e.to_string(), "OutOfRange { field: diagonal_in }: 5 is outside");
    assert_eq!(
        DisplayError::UnknownField { field: "brightness".into() }.to_string(),
        "UnknownField { field: brightness }"
    );
    assert_eq!(DisplayError::TooLarge { found: 2, max: 1 }.to_string(), "TooLarge { found: 2, max: 1 }");
}

#[test]
fn validation_is_deterministic() {
    let text = format!("{}b = 1\na = 2\n", with("resolution", "resolution = [1, 2]"));
    let first = errs(&text);
    for _ in 0..5 {
        assert_eq!(errs(&text), first);
    }
}
