//! Acceptance criteria of spec node PLAT.DEPLOY.NOADMIN.LAYOUT (AC-1 .. AC-4) and the
//! requirement edge cases. Expected results are hand-labelled from the spec's rules (each
//! fixture in `fixtures/plat/deploy/layout-*.toml` states its expected outcome in its header
//! comment); none is produced by running this crate. Every environment is a fake value map:
//! no test reads the real user profile, and no test writes any file.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_platform::matrix::{self, OsFamily, Phase};
use fid_platform::peruser_layout::{
    check_coverage, is_allowed_write, parse, required_families, resolve_env, validate, Entry, ExpandError,
    ExpandedLayout, LayoutError, MapEnv, NormPath, PathFlavor, PathProblem, PeruserLayout, Registration, Var,
    MAX_BYTES,
};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap()
}

const MATRIX_FIXTURE: &str = "fixtures/plat/matrix/valid.toml";
const SHIPPED: &str = "platform/peruser-layout.toml";

/// Runs `fid-platform plat-deploy validate-layout <args>` from the repository root.
fn cli(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-platform"))
        .current_dir(root())
        .args(["plat-deploy", "validate-layout"])
        .args(args)
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

fn shipped() -> PeruserLayout {
    parse(&read(SHIPPED)).unwrap()
}

fn errors(text: &str) -> Vec<LayoutError> {
    validate(text.as_bytes()).errors
}

/// A fake Windows profile for user `op` (AC-3), export root `D:\QA\out`.
fn windows_env() -> MapEnv {
    MapEnv { export_roots: vec![r"D:\QA\out".into()], ..MapEnv::default() }
        .with(Var::Home, r"C:\Users\op")
        .with(Var::LocalAppData, r"C:\Users\op\AppData\Local")
        .with(Var::AppData, r"C:\Users\op\AppData\Roaming")
        .with(Var::Tmp, r"C:\Users\op\AppData\Local\Temp")
}

fn windows() -> ExpandedLayout {
    shipped().expand(OsFamily::Windows, &windows_env()).unwrap()
}

fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let map: BTreeMap<String, OsString> = pairs.iter().map(|(k, v)| (k.to_string(), OsString::from(v))).collect();
    move |name| map.get(name).cloned()
}

fn strings(paths: &[NormPath]) -> Vec<String> {
    paths.iter().map(ToString::to_string).collect()
}

const ALL_REGS: &str = r#"forbidden_registrations = ["service", "driver", "machine_assoc", "scheduled_task"]"#;

/// A one-family file for `os` with the given install_dir and allowed_writes (TOML literals).
fn one(os: &str, install: &str, writes: &str) -> String {
    format!("schema_version = 1\n[os.{os}]\ninstall_dir = {install:?}\nallowed_writes = {writes}\n{ALL_REGS}\n")
}

// ---------------------------------------------------------------- acceptance criteria

#[test]
fn ac1_shipped_file_against_matrix_fixture_exits_0() {
    let (code, out) = cli(&["--matrix", MATRIX_FIXTURE, "--phase", "P1a"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "layout: 3 os, 0 errors\n");
    // Defaults: shipped layout, shipped matrix, phase P1a.
    let (code, out) = cli(&[]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "layout: 3 os, 0 errors\n");
}

#[test]
fn ac1_shipped_file_is_provisional_and_complete() {
    let l = shipped();
    assert_eq!(l.os.keys().copied().collect::<Vec<_>>(), [OsFamily::Windows, OsFamily::Ubuntu, OsFamily::Macos]);
    for ol in l.os.values() {
        assert!(ol.provisional);
        assert_eq!(ol.forbidden_registrations, Registration::ALL);
    }
    let m = matrix::parse(&read(MATRIX_FIXTURE)).unwrap();
    assert!(check_coverage(&l, &m, Phase::P1a).is_empty());
    assert_eq!(
        required_families(&m, Phase::P1a).into_iter().collect::<Vec<_>>(),
        [OsFamily::Windows, OsFamily::Ubuntu, OsFamily::Macos]
    );
    // Before P1a only the Ubuntu row (since P0) is effective.
    assert_eq!(required_families(&m, Phase::P0).into_iter().collect::<Vec<_>>(), [OsFamily::Ubuntu]);
}

#[test]
fn ac2_program_files_install_dir_is_outside_profile() {
    let file = "fixtures/plat/deploy/layout-programfiles.toml";
    let (code, out) = cli(&["--file", file, "--matrix", MATRIX_FIXTURE]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "OutsideProfile { os: windows, entry: install_dir }\nlayout: 3 os, 1 errors\n");
    let v = validate(&read(file));
    assert_eq!(v.errors, [LayoutError::OutsideProfile { os: OsFamily::Windows, entry: Entry::InstallDir }]);
    assert!(v.layout.is_none());
}

#[test]
fn ac3_fake_windows_environment() {
    let e = windows();
    assert!(is_allowed_write(r"d:\qa\out\r.pdf", &e));
    assert!(!is_allowed_write(r"C:\Users\op\..\admin\x", &e));
    // `..` is rejected even when the unresolved prefix looks allowed.
    assert!(!is_allowed_write(r"C:\Users\op\AppData\Local\Fiducial\..\..\x", &e));
    assert!(e.is_allowed_write(r"C:\Users\op\AppData\Local\Fiducial\settings.toml"));
    assert!(e.is_allowed_write("c:/users/OP/appdata/roaming/fiducial/x"));
    assert!(e.is_allowed_write(r"C:\Users\op\AppData\Local\Temp\f.tmp"));
    // The install directory and the rest of the profile are not write locations.
    assert!(!e.is_allowed_write(r"C:\Users\op\AppData\Local\Programs\Fiducial\app.exe"));
    assert!(!e.is_allowed_write(r"C:\Users\op\Documents\x"));
    assert!(!e.is_allowed_write(r"C:\Program Files\Fiducial\x"));
    assert!(!e.is_allowed_write(r"D:\QA\outside\r.pdf"));
    assert_eq!(e.install_dir.to_string(), r"C:\Users\op\AppData\Local\Programs\Fiducial");
    assert_eq!(
        strings(&e.allowed_writes),
        [
            r"C:\Users\op\AppData\Local\Fiducial",
            r"C:\Users\op\AppData\Roaming\Fiducial",
            r"C:\Users\op\AppData\Local\Temp",
            r"D:\QA\out",
        ]
    );
}

#[test]
fn ac4_missing_macos() {
    let file = "fixtures/plat/deploy/layout-missing-macos.toml";
    let l = parse(&read(file)).unwrap();
    let m = matrix::parse(&read(MATRIX_FIXTURE)).unwrap();
    assert_eq!(check_coverage(&l, &m, Phase::P1a), [LayoutError::MissingOs { os: OsFamily::Macos }]);
    let (code, out) = cli(&["--file", file, "--matrix", MATRIX_FIXTURE, "--phase", "P1a"]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "MissingOs { os: macos }\nlayout: 2 os, 1 errors\n");
    // With the shipped matrix (macOS Tier 1 from P1a) too.
    let (code, out) = cli(&["--file", file]);
    assert_eq!((code, out.as_str()), (1, "MissingOs { os: macos }\nlayout: 2 os, 1 errors\n"));
    assert_eq!(l.expand(OsFamily::Macos, &MapEnv::default()), Err(ExpandError::MissingOs { os: OsFamily::Macos }));
}

// ---------------------------------------------------------------- validation rules (REQ-001..004)

#[test]
fn bad_templates_fixture_lists_every_error_in_order() {
    use LayoutError::*;
    use OsFamily::*;
    let v = validate(&read("fixtures/plat/deploy/layout-bad-templates.toml"));
    let got: Vec<String> = v.errors.iter().map(ToString::to_string).collect();
    let expected = [
        OutsideProfile { os: Macos, entry: Entry::AllowedWrite(0) }.to_string(),
        UnknownVariable { os: Macos, entry: Entry::AllowedWrite(1), name: "xdg_data".into() }.to_string(),
        OutsideProfile { os: Ubuntu, entry: Entry::InstallDir }.to_string(),
        "BadTemplate { os: ubuntu, entry: allowed_writes[0] }".into(),
        "BadTemplate { os: ubuntu, entry: allowed_writes[1] }".into(),
        MissingRegistration { os: Ubuntu, registration: Registration::ScheduledTask }.to_string(),
        OutsideProfile { os: Windows, entry: Entry::InstallDir }.to_string(),
        UnknownVariable { os: Windows, entry: Entry::AllowedWrite(0), name: "program_files".into() }.to_string(),
        "BadField: os.windows.forbidden_registrations[1]".into(),
        MissingRegistration { os: Windows, registration: Registration::Driver }.to_string(),
    ];
    assert_eq!(got.len(), expected.len(), "{got:#?}");
    for (g, e) in got.iter().zip(&expected) {
        assert!(g.starts_with(e.as_str()), "{g} does not start with {e}");
    }
    assert_eq!(got[6], "OutsideProfile { os: windows, entry: install_dir }");
    assert_eq!(got[1], "UnknownVariable { os: macos, entry: allowed_writes[1], name: xdg_data }");
}

#[test]
fn literal_and_dotdot_templates_are_outside_profile() {
    let e = errors(&one("ubuntu", "{home}/x/../../etc", r#"["{tmp}"]"#));
    assert_eq!(e, [LayoutError::OutsideProfile { os: OsFamily::Ubuntu, entry: Entry::InstallDir }]);
    let e = errors(&one("ubuntu", "{home}/app", r#"["{tmp}", "/var/lib/fiducial"]"#));
    assert_eq!(e, [LayoutError::OutsideProfile { os: OsFamily::Ubuntu, entry: Entry::AllowedWrite(1) }]);
    let e = errors(&one("ubuntu", "relative/app", r#"["{tmp}"]"#));
    assert_eq!(e, [LayoutError::OutsideProfile { os: OsFamily::Ubuntu, entry: Entry::InstallDir }]);
    // Windows installs must be below {local_app_data}, not merely below {home}.
    let e = errors(&one("windows", "{home}/AppData/Local/Programs/Fiducial", r#"["{tmp}"]"#));
    assert_eq!(e, [LayoutError::OutsideProfile { os: OsFamily::Windows, entry: Entry::InstallDir }]);
}

#[test]
fn malformed_templates() {
    let bad = |install: &str, writes: &str| {
        let e = errors(&one("macos", install, writes));
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(matches!(e[0], LayoutError::BadTemplate { os: OsFamily::Macos, .. }), "{e:?}");
    };
    bad("{home}", r#"["{tmp}"]"#); // install_dir must name a subdirectory
    bad("{home}/", r#"["{tmp}"]"#);
    bad("{home}//Applications", r#"["{tmp}"]"#);
    bad("{home}/./Applications", r#"["{tmp}"]"#);
    bad("{home}Applications", r#"["{tmp}"]"#);
    bad("{home", r#"["{tmp}"]"#);
    bad("", r#"["{tmp}"]"#);
    bad("{home}/Applications", r#"["{home}/Library/{tmp}"]"#);
    bad("{home}/Applications", r#"["{home}/Library\\Caches"]"#);
    bad("{home}/Applications", r#"["{home}/Library:x"]"#);
    // The whole profile is not a data directory.
    bad("{home}/Applications", r#"["{home}"]"#);
    // macOS {home} write entries must be below ~/Library/{Application Support,Caches,Preferences}.
    bad("{home}/Applications", r#"["{home}/.ssh"]"#);
    bad("{home}/Applications", r#"["{home}/Documents/Fiducial"]"#);
    bad("{home}/Applications", r#"["{home}/Library/Caches"]"#);
    bad("{home}/Applications", r#"["{home}/Library/Keychains/Fiducial"]"#);
    bad("{home}/Applications", r#"["{home}/Library/LaunchAgents/fiducial.plist"]"#);
    bad("{home}/Applications", r#"["{home}/library/caches/Fiducial"]"#);
    assert!(errors(&one(
        "macos",
        "{home}/Applications",
        r#"["{home}/Library/Application Support/F", "{home}/Library/Caches/F", "{home}/Library/Preferences/F.plist"]"#
    ))
    .is_empty());
    // {tmp} and the configured roots may stand alone.
    assert!(
        errors(&one("macos", "{home}/Applications", r#"["{tmp}", "{project_roots}", "{export_roots}"]"#)).is_empty()
    );
}

#[test]
fn variables_are_per_os() {
    let e = errors(&one("windows", "{local_app_data}/P", r#"["{xdg_config}/f"]"#));
    assert_eq!(
        e,
        [LayoutError::UnknownVariable {
            os: OsFamily::Windows,
            entry: Entry::AllowedWrite(0),
            name: "xdg_config".into()
        }]
    );
    let e = errors(&one("ubuntu", "{local_app_data}/P", r#"["{tmp}"]"#));
    assert_eq!(
        e,
        [LayoutError::UnknownVariable {
            os: OsFamily::Ubuntu,
            entry: Entry::InstallDir,
            name: "local_app_data".into()
        }]
    );
    let e = errors(&one("ubuntu", "{home}/app", r#"["{HOME}/x"]"#));
    assert_eq!(
        e,
        [LayoutError::UnknownVariable { os: OsFamily::Ubuntu, entry: Entry::AllowedWrite(0), name: "HOME".into() }]
    );
}

#[test]
fn structural_errors() {
    let e = errors("schema_version = 1\n[os.solaris]\ninstall_dir = \"{home}/x\"\n");
    assert_eq!(e.len(), 1);
    assert!(e[0].to_string().starts_with("BadField: os.solaris "), "{e:?}");
    let e = errors("schema_version = 1\nextra = 1\n[os.macos]\ninstall_dir = 3\nallowed_writes = []\ncolour = 1\n");
    let s: Vec<String> = e.iter().map(ToString::to_string).collect();
    assert_eq!(s.len(), 5, "{s:#?}");
    assert!(s[0].starts_with("BadField: extra "));
    assert!(s[1].starts_with("BadField: os.macos.colour "));
    assert!(s[2].starts_with("BadField: os.macos.install_dir "));
    assert!(s[3].starts_with("BadField: os.macos.allowed_writes is empty"));
    assert!(s[4].starts_with("BadField: os.macos.forbidden_registrations is missing"));
    let e = errors(&format!(
        "schema_version = 1\n[os.macos]\ninstall_dir = \"{{home}}/A\"\nallowed_writes = [\"{{tmp}}\"]\n{}\n",
        r#"forbidden_registrations = ["service", "service", "driver", "machine_assoc", "scheduled_task"]"#
    ));
    assert_eq!(e.len(), 1);
    assert!(e[0].to_string().starts_with("BadField: os.macos.forbidden_registrations[1] repeats service"));
    assert!(errors("[os.macos]\n")[0].to_string().starts_with("BadField: schema_version is missing"));
}

#[test]
fn fatal_errors() {
    assert_eq!(errors("schema_version = 2\n"), [LayoutError::UnsupportedSchema { found: 2 }]);
    assert!(matches!(errors("schema_version = \n")[..], [LayoutError::Parse { line: Some(_), .. }]));
    assert!(matches!(validate(&[0xff, 0xfe]).errors[..], [LayoutError::Parse { line: None, .. }]));
    let mut big = read(SHIPPED);
    big.resize(MAX_BYTES + 1, b'#');
    assert_eq!(validate(&big).errors, [LayoutError::TooLarge { found: MAX_BYTES + 1, max: MAX_BYTES }]);
    // Exactly at the cap is accepted (trailing comment padding after a final newline).
    let mut at_cap = read(SHIPPED);
    at_cap.push(b'#');
    at_cap.resize(MAX_BYTES, b'#');
    assert!(validate(&at_cap).errors.is_empty());
}

#[test]
fn render_is_deterministic() {
    let bytes = read("fixtures/plat/deploy/layout-bad-templates.toml");
    assert_eq!(validate(&bytes).render(), validate(&bytes).render());
    assert!(validate(&bytes).render().ends_with("layout: 3 os, 10 errors\n"));
}

// ---------------------------------------------------------------- expansion and REQ-004 at run time

/// The shipped Linux layout with a fake `HOME=/home/op`.
fn linux_env(pairs: &[(&str, &str)]) -> Result<MapEnv, ExpandError> {
    let mut all = vec![("HOME", "/home/op")];
    all.extend_from_slice(pairs);
    resolve_env(OsFamily::Ubuntu, &lookup(&all), vec!["/srv/projects".into()], vec!["/mnt/qa/out".into()])
}

#[test]
fn linux_xdg_fallbacks_and_case_sensitivity() {
    // Empty and relative XDG values are ignored (XDG Base Directory specification).
    let env = linux_env(&[("XDG_DATA_HOME", ""), ("XDG_CACHE_HOME", "rel/cache"), ("XDG_CONFIG_HOME", "/home/op/cfg")])
        .unwrap();
    let e = shipped().expand(OsFamily::Ubuntu, &env).unwrap();
    assert_eq!(e.install_dir.to_string(), "/home/op/.local/share/fiducial/app");
    assert_eq!(
        strings(&e.allowed_writes),
        [
            "/home/op/.local/share/fiducial",
            "/home/op/.cache/fiducial",
            "/home/op/cfg/fiducial",
            "/tmp",
            "/srv/projects",
            "/mnt/qa/out",
        ]
    );
    assert!(e.is_allowed_write("/home/op/.local/share/fiducial/db/x"));
    assert!(!e.is_allowed_write("/home/op/.local/share/Fiducial/db/x"));
    assert!(!e.is_allowed_write("/HOME/op/.cache/fiducial/x"));
    assert!(e.is_allowed_write("/srv/projects//p1/./run.json"));
    assert!(!e.is_allowed_write("/srv/projects/../etc/passwd"));
    assert!(!e.is_allowed_write("srv/projects/p1"));
    assert!(!e.is_allowed_write(""));
    assert!(!e.is_allowed_write("/srv/projectsX/p1"));
    assert!(!e.is_allowed_write("/home/op/.bashrc"));
}

#[test]
fn xdg_outside_home_fails_expansion() {
    let env = linux_env(&[("XDG_DATA_HOME", "/data/xdg")]).unwrap();
    assert_eq!(
        shipped().expand(OsFamily::Ubuntu, &env),
        Err(ExpandError::OutsideProfile { os: OsFamily::Ubuntu, entry: Entry::AllowedWrite(0) })
    );
    // Inside a configured project root it is allowed (REQ-004 lists the roots).
    let env = linux_env(&[("XDG_DATA_HOME", "/srv/projects/xdg")]).unwrap();
    assert!(shipped().expand(OsFamily::Ubuntu, &env).is_ok());
}

#[test]
fn macos_library_and_case_insensitivity() {
    let env = resolve_env(
        OsFamily::Macos,
        &lookup(&[("HOME", "/Users/op"), ("TMPDIR", "/var/folders/ab/T/")]),
        vec![],
        vec![],
    )
    .unwrap();
    let e = shipped().expand(OsFamily::Macos, &env).unwrap();
    assert_eq!(e.install_dir.to_string(), "/Users/op/Applications");
    assert_eq!(
        strings(&e.allowed_writes),
        [
            "/Users/op/Library/Application Support/Fiducial",
            "/Users/op/Library/Caches/Fiducial",
            "/Users/op/Library/Preferences/Fiducial.plist",
            "/var/folders/ab/T",
        ]
    );
    assert!(e.is_allowed_write("/users/OP/library/caches/fiducial/tiles/1"));
    assert!(e.is_allowed_write("/Users/op/Library/Preferences/fiducial.plist"));
    assert!(!e.is_allowed_write("/Users/op/Library/Preferences/com.other.plist"));
    assert!(!e.is_allowed_write("/Users/op/Applications/Fiducial.app/Contents/x"));
}

#[test]
fn resolve_env_rejects_relative_and_empty_required_values() {
    let r = |os, pairs: &[(&str, &str)]| resolve_env(os, &lookup(pairs), vec![], vec![]);
    assert_eq!(r(OsFamily::Ubuntu, &[("HOME", "")]), Err(ExpandError::Unset { var: Var::Home, source: "HOME".into() }));
    assert_eq!(r(OsFamily::Ubuntu, &[]), Err(ExpandError::Unset { var: Var::Home, source: "HOME".into() }));
    assert_eq!(
        r(OsFamily::Ubuntu, &[("HOME", "home/op")]),
        Err(ExpandError::BadValue { var: Var::Home, source: "HOME".into(), problem: PathProblem::Relative })
    );
    assert_eq!(
        r(OsFamily::Macos, &[("HOME", "/")]),
        Err(ExpandError::BadValue { var: Var::Home, source: "HOME".into(), problem: PathProblem::RootOnly })
    );
    assert_eq!(
        r(OsFamily::Macos, &[("HOME", "/Users/op"), ("TMPDIR", "tmp")]),
        Err(ExpandError::BadValue { var: Var::Tmp, source: "TMPDIR".into(), problem: PathProblem::Relative })
    );
    assert_eq!(
        r(OsFamily::Ubuntu, &[("HOME", "/home/op"), ("XDG_CACHE_HOME", "/home/op/../x")]),
        Err(ExpandError::BadValue {
            var: Var::XdgCache,
            source: "XDG_CACHE_HOME".into(),
            problem: PathProblem::DotDot
        })
    );
    assert_eq!(
        r(OsFamily::Windows, &[("USERPROFILE", "")]),
        Err(ExpandError::Unset { var: Var::Home, source: "USERPROFILE".into() })
    );
    assert_eq!(
        r(OsFamily::Windows, &[("USERPROFILE", r"C:\Users\op"), ("LOCALAPPDATA", r"AppData\Local")]),
        Err(ExpandError::BadValue {
            var: Var::LocalAppData,
            source: "LOCALAPPDATA".into(),
            problem: PathProblem::Relative
        })
    );
    assert_eq!(
        r(OsFamily::Windows, &[("USERPROFILE", "C:")]),
        Err(ExpandError::BadValue { var: Var::Home, source: "USERPROFILE".into(), problem: PathProblem::Relative })
    );
}

#[test]
fn resolve_env_windows_fallbacks() {
    let env = resolve_env(OsFamily::Windows, &lookup(&[("USERPROFILE", r"C:\Users\op\")]), vec![], vec![]).unwrap();
    assert_eq!(env.paths[&Var::LocalAppData], r"C:\Users\op\AppData\Local");
    assert_eq!(env.paths[&Var::AppData], r"C:\Users\op\AppData\Roaming");
    // GetTempPath order: TMP, then TEMP, then USERPROFILE.
    assert_eq!(env.paths[&Var::Tmp], r"C:\Users\op\");
    let env = resolve_env(
        OsFamily::Windows,
        &lookup(&[("USERPROFILE", r"C:\Users\op"), ("TEMP", r"C:\Users\op\TEMP"), ("TMP", r"C:\Users\op\TMP")]),
        vec![],
        vec![],
    )
    .unwrap();
    assert_eq!(env.paths[&Var::Tmp], r"C:\Users\op\TMP");
    let env = resolve_env(
        OsFamily::Windows,
        &lookup(&[("USERPROFILE", r"C:\Users\op"), ("TEMP", r"C:\Users\op\TEMP")]),
        vec![],
        vec![],
    )
    .unwrap();
    assert_eq!(env.paths[&Var::Tmp], r"C:\Users\op\TEMP");
    let env = resolve_env(
        OsFamily::Windows,
        &lookup(&[("USERPROFILE", r"C:\Users\op"), ("TEMP", ""), ("TMP", r"C:\Users\op\T")]),
        vec![],
        vec![],
    )
    .unwrap();
    assert_eq!(env.paths[&Var::Tmp], r"C:\Users\op\T");
    let e = shipped().expand(OsFamily::Windows, &env).unwrap();
    assert!(e.is_allowed_write(r"c:\users\op\t\x"));
    // Known-folder values injected directly (redirected Roaming on another drive) fail REQ-004.
    let redirected = windows_env().with(Var::AppData, r"\\fileserver\profiles\op\Roaming");
    assert_eq!(
        shipped().expand(OsFamily::Windows, &redirected),
        Err(ExpandError::OutsideProfile { os: OsFamily::Windows, entry: Entry::AllowedWrite(1) })
    );
}

#[test]
fn expand_validates_injected_values() {
    let l = shipped();
    let e = l.expand(OsFamily::Windows, &windows_env().with(Var::LocalAppData, ""));
    assert_eq!(
        e,
        Err(ExpandError::BadValue {
            var: Var::LocalAppData,
            source: "local_app_data".into(),
            problem: PathProblem::Empty
        })
    );
    let mut env = windows_env();
    env.paths.remove(&Var::AppData);
    assert_eq!(
        l.expand(OsFamily::Windows, &env),
        Err(ExpandError::Unset { var: Var::AppData, source: "app_data".into() })
    );
    let env = MapEnv { export_roots: vec!["out".into()], ..windows_env() };
    assert_eq!(
        l.expand(OsFamily::Windows, &env),
        Err(ExpandError::BadValue {
            var: Var::ExportRoots,
            source: "export_roots".into(),
            problem: PathProblem::Relative
        })
    );
    // No configured roots: those templates expand to nothing.
    let env = MapEnv { export_roots: vec![], ..windows_env() };
    assert_eq!(l.expand(OsFamily::Windows, &env).unwrap().allowed_writes.len(), 3);
}

// ---------------------------------------------------------------- path normalisation (REQ-005)

#[test]
fn windows_path_rules() {
    let w = |s: &str| NormPath::parse(s, PathFlavor::Windows);
    assert_eq!(w(r"c:\a\\b\.\c\").unwrap().to_string(), r"C:\a\b\c");
    assert_eq!(w("C:/").unwrap().to_string(), r"C:\");
    assert_eq!(w(r"\\nas\qa\out\r.pdf").unwrap().to_string(), r"\\nas\qa\out\r.pdf");
    assert_eq!(w(r"C:\a\..\b"), Err(PathProblem::DotDot));
    assert_eq!(w(r"C:a\b"), Err(PathProblem::Relative));
    assert_eq!(w(r"\a\b"), Err(PathProblem::Relative));
    assert_eq!(w(r"a\b"), Err(PathProblem::Relative));
    assert_eq!(w(r"\\nas"), Err(PathProblem::Relative));
    // Verbatim paths are rewritten to plain form; device paths stay rejected.
    assert_eq!(w(r"\\?\C:\a\b").unwrap().to_string(), r"C:\a\b");
    assert_eq!(w(r"\\?\c:\a"), w(r"C:\a"));
    assert_eq!(w(r"\\?\UNC\nas\qa\x").unwrap().to_string(), r"\\nas\qa\x");
    assert_eq!(w(r"\\?\unc\nas\qa\x"), w(r"\\nas\qa\x"));
    assert_eq!(w(r"\\?\C:\a\..\b"), Err(PathProblem::DotDot));
    assert_eq!(w(r"\\?\a\b"), Err(PathProblem::Relative));
    assert_eq!(w(r"\\?\UNC\?\x\y"), Err(PathProblem::Verbatim));
    assert_eq!(w(r"\\.\PhysicalDrive0"), Err(PathProblem::Verbatim));
    assert_eq!(w(r"\\.\C:\a"), Err(PathProblem::Verbatim));
    assert_eq!(w(r"C:\a\file.txt:stream"), Err(PathProblem::BadChar));
    assert_eq!(w(r"C:\a\dir.\x"), Err(PathProblem::BadChar));
    assert_eq!(w(r"C:\a\dir \x"), Err(PathProblem::BadChar));
    assert_eq!(w("C:\\a\0"), Err(PathProblem::BadChar));
    assert_eq!(w(""), Err(PathProblem::Empty));
    // UNC export root, compared case-insensitively.
    let env = MapEnv { export_roots: vec![r"\\nas\QA\out".into()], ..windows_env() };
    let e = shipped().expand(OsFamily::Windows, &env).unwrap();
    assert!(e.is_allowed_write(r"\\NAS\qa\OUT\r.pdf"));
    assert!(!e.is_allowed_write(r"\\nas\qa\other\r.pdf"));
    assert!(e.is_allowed_write(r"\\?\UNC\nas\QA\out\r.pdf"));
    assert!(!e.is_allowed_write(r"\\.\UNC\nas\QA\out\r.pdf"));
    // Store-style verbatim paths (as fid-store and std::fs::canonicalize produce them).
    let e = windows();
    assert!(e.is_allowed_write(r"\\?\C:\Users\op\AppData\Local\Fiducial\store\objects\ab\cdef"));
    assert!(e.is_allowed_write(r"\\?\D:\QA\out\r.pdf"));
    assert!(!e.is_allowed_write(r"\\?\C:\Users\op\AppData\Local\Programs\Fiducial\app.exe"));
    assert!(!e.is_allowed_write(r"\\?\C:\Windows\System32\x"));
}

#[test]
fn filesystem_roots_cannot_be_configured_as_roots() {
    let env = MapEnv { project_roots: vec!["/".into()], ..MapEnv::default() }
        .with(Var::Home, "/home/op")
        .with(Var::XdgData, "/home/op/.local/share")
        .with(Var::XdgCache, "/home/op/.cache")
        .with(Var::XdgConfig, "/home/op/.config")
        .with(Var::Tmp, "/tmp");
    assert_eq!(
        shipped().expand(OsFamily::Ubuntu, &env),
        Err(ExpandError::BadValue {
            var: Var::ProjectRoots,
            source: "project_roots".into(),
            problem: PathProblem::RootOnly
        })
    );
    let env = MapEnv { project_roots: vec!["/srv/projects".into()], ..env };
    let e = shipped().expand(OsFamily::Ubuntu, &env).unwrap();
    assert!(!e.is_allowed_write("/etc/passwd"));
    assert!(e.is_allowed_write("/srv/projects/p/x"));
    for root in [r"C:\", "C:/", r"\\?\C:\"] {
        let env = MapEnv { export_roots: vec![root.into()], ..windows_env() };
        assert_eq!(
            shipped().expand(OsFamily::Windows, &env),
            Err(ExpandError::BadValue {
                var: Var::ExportRoots,
                source: "export_roots".into(),
                problem: PathProblem::RootOnly
            }),
            "{root}"
        );
    }
    let e = windows();
    assert!(!e.is_allowed_write(r"C:\Windows\System32\drivers\etc\hosts"));
}

#[cfg(unix)]
fn not_unicode() -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(vec![b'/', b'h', 0xff])
}

#[cfg(windows)]
fn not_unicode() -> OsString {
    use std::os::windows::ffi::OsStringExt;
    OsString::from_wide(&[u16::from(b'C'), u16::from(b':'), u16::from(b'\\'), 0xD800])
}

#[test]
fn non_unicode_environment_values_are_errors_not_fallbacks() {
    let bad = |os, name: &'static str, base: &[(&str, &str)]| {
        let map = lookup(base);
        resolve_env(os, &move |n: &str| if n == name { Some(not_unicode()) } else { map(n) }, vec![], vec![])
    };
    assert_eq!(
        bad(OsFamily::Ubuntu, "HOME", &[]),
        Err(ExpandError::BadValue { var: Var::Home, source: "HOME".into(), problem: PathProblem::NotUnicode })
    );
    // An XDG value that is not Unicode is an error, not a silent fallback.
    assert_eq!(
        bad(OsFamily::Ubuntu, "XDG_DATA_HOME", &[("HOME", "/home/op")]),
        Err(ExpandError::BadValue {
            var: Var::XdgData,
            source: "XDG_DATA_HOME".into(),
            problem: PathProblem::NotUnicode
        })
    );
    assert_eq!(
        bad(OsFamily::Windows, "TMP", &[("USERPROFILE", r"C:\Users\op"), ("TEMP", r"C:\Users\op\T")]),
        Err(ExpandError::BadValue { var: Var::Tmp, source: "TMP".into(), problem: PathProblem::NotUnicode })
    );
}

#[test]
fn posix_path_rules() {
    let p = |s: &str, f| NormPath::parse(s, f);
    assert_eq!(p("//a///b/./", PathFlavor::Linux).unwrap().to_string(), "/a/b");
    assert_eq!(p("/", PathFlavor::Macos).unwrap().to_string(), "/");
    assert_eq!(p("/a/../b", PathFlavor::Linux), Err(PathProblem::DotDot));
    assert_eq!(p(r"C:\a", PathFlavor::Linux), Err(PathProblem::Relative));
    // Backslash is an ordinary character on POSIX; ':' is allowed there.
    assert_eq!(p(r"/a\b:c", PathFlavor::Linux).unwrap().segments(), [r"a\b:c"]);
    let a = p("/Users/Op/X", PathFlavor::Macos).unwrap();
    assert!(a.starts_with(&p("/users/op", PathFlavor::Macos).unwrap()));
    let b = p("/home/Op/x", PathFlavor::Linux).unwrap();
    assert!(!b.starts_with(&p("/home/op", PathFlavor::Linux).unwrap()));
    assert!(b.starts_with(&p("/home/Op", PathFlavor::Linux).unwrap()));
    // Different flavors never match.
    assert!(!b.starts_with(&p("/home/Op", PathFlavor::Macos).unwrap()));
}
