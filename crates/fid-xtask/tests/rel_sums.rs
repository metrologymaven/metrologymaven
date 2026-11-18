//! Acceptance criteria of spec node ECO.REL.SIGN.SUMS (AC-1 .. AC-4). Expected outputs come
//! from `fixtures/rel/sums/make_fixtures.py` (an independent Python reference using hashlib)
//! or are hand-labelled from the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::rel::sums::{is_valid_name, run, unsummed_kind, SumsError};
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/rel/sums").join(name)
}

/// A fresh, empty output directory under the target directory (never inside the fixtures).
fn out_dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("rel-sums").join(name);
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// Runs `fid-xtask rel sums` on fixture `case` (what `cargo xtask rel sums ...` runs) from the
/// fixture root; returns (exit code, stdout).
fn cli(case: &str, out: &Path) -> (i32, String) {
    let manifest = format!("{case}/release-manifest.json");
    let dir = format!("{case}/");
    let out_s = out.to_str().unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .current_dir(fixture(""))
        .args(["rel", "sums", "--manifest", &manifest, "--dir", &dir, "--out", out_s])
        .output()
        .unwrap();
    (o.status.code().unwrap(), String::from_utf8(o.stdout).unwrap())
}

/// AC-1: the `ok` fixture gives `expected/SHA256SUMS` and `expected/sums-statement.json`.
#[test]
fn ac1_ok_matches_expected() {
    let out = out_dir("ac1");
    let (code, stdout) = cli("ok", &out);
    assert_eq!((code, stdout.as_str()), (0, "sums: ok 4 artefacts (1.4.0)\n"));
    assert_eq!(std::fs::read(out.join("SHA256SUMS")).unwrap(), std::fs::read(fixture("expected/SHA256SUMS")).unwrap());
    assert_eq!(
        std::fs::read(out.join("sums-statement.json")).unwrap(),
        std::fs::read(fixture("expected/sums-statement.json")).unwrap()
    );
}

/// AC-1: stock tools accept the file: `sha256sum -c` on Linux, `shasum -a 256 -c` on macOS and
/// a PowerShell `Get-FileHash` loop on Windows, each run in the artefact directory.
#[test]
fn ac1_stock_tools_accept() {
    let out = out_dir("ac1-stock");
    assert_eq!(cli("ok", &out).0, 0);
    let sums = out.join("SHA256SUMS");
    let sums_s = sums.to_str().unwrap();
    let mut cmd = if cfg!(target_os = "linux") {
        let mut c = Command::new("sha256sum");
        c.args(["--strict", "-c", sums_s]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("shasum");
        c.args(["-a", "256", "-c", sums_s]);
        c
    } else if cfg!(windows) {
        // Every line must be `<64 hex>  <name>` and every hash must match.
        let script = format!(
            "$ErrorActionPreference = 'Stop'; $bad = 0; \
             foreach ($line in [IO.File]::ReadAllLines('{sums_s}')) {{ \
               if ($line -cnotmatch '^([0-9a-f]{{64}})  ([A-Za-z0-9._+-]+)$') {{ $bad++; continue }}; \
               $h = (Get-FileHash -Algorithm SHA256 -LiteralPath $Matches[2]).Hash.ToLowerInvariant(); \
               if ($h -cne $Matches[1]) {{ $bad++ }} }}; \
             exit $bad"
        );
        // Windows PowerShell 5.1 with its default module path: a PSModulePath inherited from
        // PowerShell 7 (as on CI runners) stops Get-FileHash from autoloading.
        let mut c = Command::new("powershell");
        c.env_remove("PSModulePath");
        c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script]);
        c
    } else {
        return;
    };
    let status = cmd.current_dir(fixture("ok")).status().unwrap();
    assert!(status.success(), "stock checksum tool rejected {sums_s}");
}

/// AC-2: a file not in the manifest exits 1 with SUMS-UNLISTED; a listed file that is absent
/// exits 1 with SUMS-MISSING. Nothing is written.
#[test]
fn ac2_unlisted_and_missing() {
    let out = out_dir("ac2-extra");
    assert_eq!(cli("extra-file", &out), (1, "SUMS-UNLISTED notes.txt\n".to_string()));
    assert!(!out.exists());
    let out = out_dir("ac2-missing");
    assert_eq!(cli("missing-file", &out), (1, "SUMS-MISSING fid-1.4.0+src.tar.gz\n".to_string()));
    assert!(!out.exists());
}

/// AC-3: a manifest entry named `a b.msi` exits 1 with SUMS-NAME.
#[test]
fn ac3_bad_name() {
    let out = out_dir("ac3");
    let (code, stdout) = cli("bad-name", &out);
    assert_eq!(code, 1);
    assert!(stdout.starts_with("SUMS-NAME "), "{stdout}");
    assert_eq!(stdout, "SUMS-NAME \"a b.msi\"\n");
    assert!(!out.exists());
}

/// AC-4: output is byte-identical across runs; across OSes it equals the committed expected
/// files (checked by AC-1 on every CI OS).
#[test]
fn ac4_deterministic() {
    let a = out_dir("ac4-a");
    let b = out_dir("ac4-b");
    run(&fixture("ok/release-manifest.json"), &fixture("ok"), &a).unwrap();
    run(&fixture("ok/release-manifest.json"), &fixture("ok"), &b).unwrap();
    for f in ["SHA256SUMS", "sums-statement.json"] {
        let x = std::fs::read(a.join(f)).unwrap();
        assert_eq!(x, std::fs::read(b.join(f)).unwrap());
        assert_eq!(x, std::fs::read(fixture("expected").join(f)).unwrap());
        assert!(!x.contains(&b'\r'), "{f} must use LF only");
    }
}

/// REQ-003 name rule, hand-labelled.
#[test]
fn name_rule() {
    for ok in ["a.msi", "fid-1.4.0+src.tar.gz", "A_b-C.9", "..x"] {
        assert!(is_valid_name(ok), "{ok}");
    }
    for bad in ["", ".", "..", "a b.msi", "a/b", "a\\b", "a\nb", "a\tb", "é.msi", "a:b", "a*b"] {
        assert!(!is_valid_name(bad), "{bad:?}");
    }
}

/// REQ-005 name families, hand-labelled: only exact `SHA256SUMS` / `sums-statement.json`,
/// `*.sig`, `*.dsse.json` and `*.spdx.json` imply a never-summed kind.
#[test]
fn unsummed_names() {
    let cases = [
        ("SHA256SUMS", Some("checksums")),
        ("sums-statement.json", Some("checksums")),
        ("SHA256SUMS.sig", Some("signature")),
        ("SHA256SUMS.dsse.json", Some("signature")),
        ("fid.msi.sig", Some("signature")),
        ("x.msi.spdx.json", Some("sbom")),
        ("SHA256SUMS.exe", None),
        ("sums-statement.json.bin", None),
        ("SHA256SUMSX", None),
        ("fid.msi", None),
        ("spdx.json.msi", None),
    ];
    for (n, k) in cases {
        assert_eq!(unsummed_kind(n), k, "{n}");
    }
}

/// The `ok` manifest lists SHA256SUMS, its envelope and an SBOM (which also sits in the
/// directory), each with the matching kind; none of them reaches the outputs.
#[test]
fn req5_unsummed_entries_excluded() {
    let out = out_dir("req5");
    let sums = run(&fixture("ok/release-manifest.json"), &fixture("ok"), &out).unwrap();
    assert!(sums.entries.iter().all(|e| unsummed_kind(&e.name).is_none()));
    let text = std::fs::read_to_string(out.join("SHA256SUMS")).unwrap();
    assert!(!text.contains("spdx") && !text.contains("SHA256SUMS"));
    // Only the two outputs, no temporary files left behind.
    let mut names: Vec<String> =
        std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    assert_eq!(names, ["SHA256SUMS", "sums-statement.json"]);
}

/// A scratch artefact directory holding `files` (each a few bytes) and a manifest listing
/// `entries` as (name, kind).
fn scratch_case(tag: &str, files: &[&str], entries: &[(&str, Option<&str>)]) -> PathBuf {
    let d = out_dir(&format!("case-{tag}"));
    std::fs::create_dir_all(&d).unwrap();
    for f in files {
        std::fs::write(d.join(f), f.as_bytes()).unwrap();
    }
    let arts: Vec<String> = entries
        .iter()
        .map(|(n, k)| match k {
            Some(k) => format!("{{\"name\":\"{n}\",\"kind\":\"{k}\"}}"),
            None => format!("{{\"name\":\"{n}\"}}"),
        })
        .collect();
    let manifest = format!("{{\"version\":\"1.4.0\",\"artefacts\":[{}]}}", arts.join(","));
    std::fs::write(d.join("release-manifest.json"), manifest).unwrap();
    d
}

/// Runs the binary on a scratch case; returns (exit code, stdout).
fn cli_case(d: &Path, out: &Path) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .args(["rel", "sums", "--manifest"])
        .arg(d.join("release-manifest.json"))
        .arg("--dir")
        .arg(d)
        .arg("--out")
        .arg(out)
        .output()
        .unwrap();
    (o.status.code().unwrap(), String::from_utf8(o.stdout).unwrap())
}

/// Name family and kind must agree; a disagreement is SUMS-KIND-MISMATCH (exit 1), never a
/// silent skip, and nothing is written.
#[test]
fn kind_mismatch_is_a_finding() {
    let cases: [(&str, Option<&str>, Option<&str>); 8] = [
        ("fid.msi", Some("signature"), None),
        ("SHA256SUMS.exe", Some("checksums"), None),
        ("sums-statement.json.bin", Some("checksums"), None),
        ("x.exe.spdx.json", Some("installer"), Some("sbom")),
        ("x.exe.spdx.json", None, Some("sbom")),
        ("SHA256SUMS", Some("installer"), Some("checksums")),
        ("SHA256SUMS.sig", Some("sbom"), Some("signature")),
        ("fid.msi.dsse.json", Some("checksums"), Some("signature")),
    ];
    for (i, (name, kind, implied)) in cases.into_iter().enumerate() {
        let d = scratch_case(&format!("mismatch-{i}"), &["a.msi", name], &[("a.msi", Some("installer")), (name, kind)]);
        let out = out_dir(&format!("mismatch-{i}-out"));
        let errs = run(&d.join("release-manifest.json"), &d, &out).unwrap_err();
        let want = SumsError::KindMismatch { name: name.to_string(), kind: kind.map(str::to_string), implied };
        assert_eq!(errs, [want], "{name} {kind:?}");
        assert!(!out.exists(), "{name}");
    }
    let d = scratch_case("mismatch-cli", &["fid.msi"], &[("fid.msi", Some("signature"))]);
    let out = out_dir("mismatch-cli-out");
    assert_eq!(cli_case(&d, &out), (1, "SUMS-KIND-MISMATCH fid.msi (kind signature, name implies an artefact)\n".to_string()));
}

/// Names that only look like a never-summed family are ordinary artefacts and are summed.
#[test]
fn lookalike_names_are_summed() {
    let files = ["SHA256SUMS.exe", "sums-statement.json.bin"];
    let d = scratch_case("lookalike", &files, &[(files[0], Some("installer")), (files[1], Some("source"))]);
    let out = out_dir("lookalike-out");
    let sums = run(&d.join("release-manifest.json"), &d, &out).unwrap();
    let names: Vec<&str> = sums.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, files);
}

/// A never-summed name in the directory is skipped only when the manifest lists it with the
/// matching kind; otherwise it is an unlisted file.
#[test]
fn unlisted_checksum_file_in_dir_is_a_finding() {
    let d = scratch_case("stray-sums", &["a.msi", "SHA256SUMS"], &[("a.msi", None)]);
    let out = out_dir("stray-sums-out");
    assert_eq!(cli_case(&d, &out), (1, "SUMS-UNLISTED SHA256SUMS\n".to_string()));
}

/// Only regular files are artefacts: a listed directory is SUMS-UNLISTED (not a regular file).
#[test]
fn directory_entry_is_not_an_artefact() {
    let d = scratch_case("subdir", &["a.msi"], &[("a.msi", None), ("sub.msi", None)]);
    std::fs::create_dir(d.join("sub.msi")).unwrap();
    let out = out_dir("subdir-out");
    assert_eq!(cli_case(&d, &out), (1, "SUMS-UNLISTED sub.msi (not a regular file)\n".to_string()));
    assert!(!out.exists());
}

/// A symbolic link is never followed, even to a regular file in the same directory.
#[cfg(unix)]
#[test]
fn symlink_is_not_an_artefact() {
    let d = scratch_case("symlink", &["a.msi"], &[("a.msi", None), ("link.msi", None)]);
    std::os::unix::fs::symlink("a.msi", d.join("link.msi")).unwrap();
    let out = out_dir("symlink-out");
    assert_eq!(cli_case(&d, &out), (1, "SUMS-UNLISTED link.msi (not a regular file)\n".to_string()));
    assert!(!out.exists());
}

/// A missing manifest is SUMS-IO; a manifest without `version` is INPUT-PARSE (exit 2).
#[test]
fn manifest_errors() {
    let out = out_dir("manifest-errors");
    let errs = run(&fixture("ok/no-such.json"), &fixture("ok"), &out).unwrap_err();
    assert!(matches!(errs.as_slice(), [SumsError::Io(_)]), "{errs:?}");
    let bad = out_dir("manifest-bad-input");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("m.json"), b"{\"artefacts\":[]}").unwrap();
    let errs = run(&bad.join("m.json"), &fixture("ok"), &out).unwrap_err();
    assert_eq!(errs.len(), 1);
    assert_eq!((errs[0].code(), errs[0].exit_code()), ("INPUT-PARSE", 2));
}
