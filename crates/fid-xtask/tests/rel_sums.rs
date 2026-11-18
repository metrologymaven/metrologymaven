//! Acceptance criteria of spec node ECO.REL.SIGN.SUMS (AC-1 .. AC-4). Expected outputs come
//! from `fixtures/rel/sums/make_fixtures.py` (an independent Python reference using hashlib)
//! or are hand-labelled from the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::rel::sums::{is_unsummed_name, is_valid_name, run, SumsError};
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
               if ($line -cnotmatch '^([0-9a-f]{{64}})  ([A-Za-z0-9._+-]+)$') {{ ('bad line: ' + $line); $bad++; continue }}; \
               $h = (Get-FileHash -Algorithm SHA256 -LiteralPath $Matches[2]).Hash.ToLowerInvariant(); \
               if ($h -cne $Matches[1]) {{ ('mismatch: ' + $line + ' ' + $h); $bad++ }} }}; \
             exit $bad"
        );
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        c
    } else {
        return;
    };
    let o = cmd.current_dir(fixture("ok")).output().unwrap();
    assert!(
        o.status.success(),
        "stock checksum tool rejected {sums_s}: {:?}
stdout: {}
stderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
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

/// REQ-005: the checksum file, its signatures, the statement and SBOMs are never summed.
#[test]
fn unsummed_names() {
    for n in ["SHA256SUMS", "SHA256SUMS.sig", "SHA256SUMS.dsse.json", "sums-statement.json", "x.msi.spdx.json"] {
        assert!(is_unsummed_name(n), "{n}");
    }
    for n in ["SHA256SUMSX", "fid.msi", "spdx.json.msi"] {
        assert!(!is_unsummed_name(n), "{n}");
    }
}

/// The `ok` manifest lists SHA256SUMS, its envelope and an SBOM (which also sits in the
/// directory); none of them reaches the outputs.
#[test]
fn req5_unsummed_entries_excluded() {
    let out = out_dir("req5");
    let sums = run(&fixture("ok/release-manifest.json"), &fixture("ok"), &out).unwrap();
    assert!(sums.entries.iter().all(|e| !is_unsummed_name(&e.name)));
    let text = std::fs::read_to_string(out.join("SHA256SUMS")).unwrap();
    assert!(!text.contains("spdx") && !text.contains("SHA256SUMS"));
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
