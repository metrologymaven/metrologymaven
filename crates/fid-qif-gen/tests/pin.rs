//! Acceptance criteria of spec node IO.QIF.GEN.PIN (AC-1 .. AC-4; AC-5 deferred) and its
//! error paths. Fixture digests come from `fixtures/io/qif/gen/pin/make_fixtures.py`
//! (Python hashlib), not from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_qif_gen::pin::{Manifest, PinError, PinnedSet, Role, MANIFEST_FILE, MAX_DEPTH};
use fid_types::Digest;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// SHA-256 of the three bytes "abc" (FIPS 180-2 example), for hand-written manifests.
const ABC: &str = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/qif/gen/pin").join(rel)
}

fn expected() -> toml::Table {
    std::fs::read_to_string(fix("expected.toml")).unwrap().parse().unwrap()
}

fn exp_str(t: &toml::Table, k: &str) -> String {
    t.get(k).and_then(|v| v.as_str()).unwrap_or_else(|| panic!("expected.toml: {k}")).to_string()
}

fn d(s: &str) -> Digest {
    Digest::parse(s).unwrap()
}

/// A fresh empty directory under the system temp dir.
fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let p = std::env::temp_dir().join(format!("fid-qif-pin-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), &target).unwrap();
        }
    }
}

/// A scratch copy of `fixtures/.../ok`.
fn ok_copy(tag: &str) -> PathBuf {
    let p = scratch(tag);
    copy_dir(&fix("ok"), &p);
    p
}

fn verify_err(dir: &Path) -> PinError {
    match PinnedSet::verify(dir) {
        Ok(_) => panic!("verification of {} should fail", dir.display()),
        Err(e) => e,
    }
}

// ---------------------------------------------------------------- AC-1

#[test]
fn ac1_ok_set_has_three_files_with_their_digests() {
    let exp = expected();
    let ok = exp.get("ok").and_then(|v| v.as_table()).unwrap();
    let set = PinnedSet::verify(&fix("ok")).unwrap();
    assert_eq!(set.len(), 3);
    assert!(!set.is_empty());
    let got: Vec<(String, Digest)> = set.files().map(|f| (f.path().to_string(), f.sha256())).collect();
    let mut want: Vec<(String, Digest)> = ok.iter().map(|(k, v)| (k.clone(), d(v.as_str().unwrap()))).collect();
    want.sort();
    assert_eq!(got, want);
    for f in set.files() {
        assert_eq!(f.role(), Role::Xsd);
        let on_disk = std::fs::read(fix("ok").join(f.path())).unwrap();
        assert_eq!(f.bytes(), on_disk.as_slice());
    }
    assert_eq!(set.licence_sha256(), d(&exp_str(&exp, "licence_sha256")));
    assert_eq!(set.source().url, "https://example.org/fid-test/pin-fixtures.git");
    assert_eq!(set.source().rev, "0123456789abcdef0123456789abcdef01234567");
}

#[test]
fn verification_is_deterministic() {
    let a = PinnedSet::verify(&fix("ok")).unwrap();
    let b = PinnedSet::verify(&fix("ok")).unwrap();
    assert_eq!(a, b);
    assert_eq!(verify_err(&fix("changed")), verify_err(&fix("changed")));
}

// ---------------------------------------------------------------- AC-2

#[test]
fn ac2_changed_byte_fails_digest_mismatch_naming_b_with_both_digests() {
    let exp = expected();
    let err = verify_err(&fix("changed"));
    assert_eq!(err.code(), "xsd_digest_mismatch");
    assert_eq!(err.path(), Some("b.xsd"));
    let n = exp.get("changed_b_bytes").and_then(|v| v.as_integer()).unwrap() as u64;
    let want = PinError::DigestMismatch {
        path: "b.xsd".into(),
        expected: d(&exp_str(&exp, "changed_b_expected")),
        expected_bytes: Some(n),
        actual: Some(d(&exp_str(&exp, "changed_b_actual"))),
        actual_bytes: Some(n),
    };
    assert_eq!(err, want);
    let msg = err.to_string();
    assert!(msg.contains("b.xsd"), "{msg}");
    assert!(msg.contains(&exp_str(&exp, "changed_b_expected")), "{msg}");
    assert!(msg.contains(&exp_str(&exp, "changed_b_actual")), "{msg}");
}

#[test]
fn missing_file_fails_digest_mismatch() {
    let dir = ok_copy("missing");
    std::fs::remove_file(dir.join("b.xsd")).unwrap();
    let err = verify_err(&dir);
    assert_eq!(err.code(), "xsd_digest_mismatch");
    assert_eq!(err.path(), Some("b.xsd"));
    assert!(matches!(err, PinError::DigestMismatch { actual: None, actual_bytes: None, .. }));
    assert!(err.to_string().contains("no regular file"));
}

#[test]
fn size_change_fails_digest_mismatch() {
    let dir = ok_copy("grown");
    let p = dir.join("lib").join("units.xsd");
    let mut bytes = std::fs::read(&p).unwrap();
    let before = bytes.len() as u64;
    bytes.push(b'\n');
    std::fs::write(&p, &bytes).unwrap();
    match verify_err(&dir) {
        PinError::DigestMismatch { path, expected_bytes, actual_bytes, actual, .. } => {
            assert_eq!(path, "lib/units.xsd");
            assert_eq!(expected_bytes, Some(before));
            assert_eq!(actual_bytes, Some(before + 1));
            assert!(actual.is_some());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn changed_licence_fails_digest_mismatch() {
    let dir = ok_copy("licence");
    std::fs::write(dir.join("LICENSE.txt"), b"another licence\n").unwrap();
    let err = verify_err(&dir);
    assert_eq!(err.code(), "xsd_digest_mismatch");
    assert_eq!(err.path(), Some("LICENSE.txt"));
}

#[test]
fn file_name_case_must_match() {
    // On every OS, B.xsd on disk is not the pinned b.xsd: it is unlisted.
    let dir = ok_copy("case");
    std::fs::remove_file(dir.join("b.xsd")).unwrap();
    std::fs::write(dir.join("B.xsd"), std::fs::read(fix("ok/b.xsd")).unwrap()).unwrap();
    let err = verify_err(&dir);
    assert_eq!(err.code(), "xsd_unpinned_file");
    assert_eq!(err.path(), Some("B.xsd"));
}

// ---------------------------------------------------------------- AC-3

#[test]
fn ac3_extra_unlisted_xsd_fails_unpinned_file() {
    let err = verify_err(&fix("extra"));
    assert_eq!(err, PinError::UnpinnedFile { path: "c.xsd".into() });
    assert_eq!(err.code(), "xsd_unpinned_file");
}

#[test]
fn unlisted_schema_files_of_every_extension_and_depth_fail() {
    for name in ["x.XSD", "x.xsl", "x.xslt", "lib/deep/x.xsd"] {
        let dir = ok_copy("ext");
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"abc").unwrap();
        let err = verify_err(&dir);
        assert_eq!(err, PinError::UnpinnedFile { path: name.into() }, "{name}");
    }
}

#[test]
fn unlisted_non_schema_files_are_ignored() {
    let dir = ok_copy("readme");
    std::fs::write(dir.join("README.md"), b"notes").unwrap();
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::write(dir.join("docs").join("x.html"), b"<p/>").unwrap();
    let set = PinnedSet::verify(&dir).unwrap();
    assert_eq!(set.len(), 3);
    assert_eq!(set.resolve("README.md").unwrap_err().code(), "xsd_unpinned_reference");
}

#[test]
fn xslt_role_is_pinned_and_resolvable() {
    let dir = scratch("xslt");
    std::fs::write(dir.join("rules.xsl"), b"abc").unwrap();
    std::fs::write(dir.join("L"), b"abc").unwrap();
    let manifest = format!(
        "manifest_version = 1\nlicence_path = \"L\"\nlicence_sha256 = \"{ABC}\"\n[source]\nurl = \"u\"\nrev = \"r\"\n\
         [[file]]\npath = \"rules.xsl\"\nbytes = 3\nsha256 = \"{ABC}\"\nrole = \"xslt\"\n"
    );
    std::fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
    let set = PinnedSet::verify(&dir).unwrap();
    let f = set.resolve("rules.xsl").unwrap();
    assert_eq!(f.role(), Role::Xslt);
    assert_eq!(f.bytes(), b"abc");
    assert_eq!(f.sha256(), d(ABC));
}

// ---------------------------------------------------------------- AC-4

#[test]
fn ac4_url_and_parent_references_fail_unpinned_reference() {
    let set = PinnedSet::verify(&fix("ok")).unwrap();
    for loc in ["http://example.org/x.xsd", "../outside.xsd"] {
        let err = set.resolve(loc).unwrap_err();
        assert_eq!(err, PinError::UnpinnedReference { location: loc.into() }, "{loc}");
        assert_eq!(err.code(), "xsd_unpinned_reference");
    }
}

#[test]
fn every_non_manifest_location_fails() {
    let set = PinnedSet::verify(&fix("ok")).unwrap();
    for loc in [
        "",
        ".",
        "/a.xsd",
        "//a.xsd",
        "C:/a.xsd",
        "C:\\a.xsd",
        "c:a.xsd",
        "\\\\server\\share\\a.xsd",
        "lib\\units.xsd",
        "file:///a.xsd",
        "https://example.org/a.xsd",
        "urn:x:a.xsd",
        "lib/../a.xsd",
        "lib/units.xsd/..",
        "a.xsd/",
        "lib//units.xsd",
        "%2e%2e/a.xsd",
        "a.xsd?x",
        "a.xsd#frag",
        " a.xsd",
        "A.xsd",
        "c.xsd",
        "LICENSE.txt",
        MANIFEST_FILE,
        "lib",
    ] {
        assert_eq!(set.resolve(loc).unwrap_err().code(), "xsd_unpinned_reference", "{loc:?}");
    }
    assert_eq!(set.resolve("a.xsd").unwrap().path(), "a.xsd");
    assert_eq!(set.resolve("./lib/./units.xsd").unwrap().path(), "lib/units.xsd");
}

#[test]
fn includes_resolve_relative_to_the_referring_file_inside_the_set_only() {
    let set = PinnedSet::verify(&fix("ok")).unwrap();
    assert_eq!(set.resolve_relative("a.xsd", "lib/units.xsd").unwrap().path(), "lib/units.xsd");
    assert_eq!(set.resolve_relative("a.xsd", "b.xsd").unwrap().path(), "b.xsd");
    assert_eq!(set.resolve_relative("lib/units.xsd", "../a.xsd").unwrap().path(), "a.xsd");
    for (referrer, loc) in [
        ("a.xsd", "../outside.xsd"),
        ("lib/units.xsd", "../../outside.xsd"),
        ("lib/units.xsd", "a.xsd"),
        ("a.xsd", "http://example.org/x.xsd"),
        ("a.xsd", "/a.xsd"),
        ("c.xsd", "a.xsd"),
        ("../a.xsd", "a.xsd"),
    ] {
        let err = set.resolve_relative(referrer, loc).unwrap_err();
        assert_eq!(err.code(), "xsd_unpinned_reference", "{referrer} -> {loc}");
    }
}

// ---------------------------------------------------------------- AC-5 (deferred)

#[test]
#[ignore = "AC-5 deferred: the real DMSC QIF 3.0 XSDs are not pinned yet (licence terms unverified); see crates/fid-qif-gen/xsd/qif3/README.md"]
fn ac5_real_qif3_set_verifies() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("xsd/qif3");
    let set = PinnedSet::verify(&dir).unwrap();
    assert!(!set.is_empty());
}

// ---------------------------------------------------------------- manifest and directory errors

fn manifest_text(top: &str, rows: &str) -> String {
    format!("manifest_version = 1\n{top}\n[source]\nurl = \"u\"\nrev = \"r\"\n{rows}")
}

const LIC: &str = "licence_path = \"L\"\nlicence_sha256 = \"sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\"";

fn row(path: &str, role: &str) -> String {
    format!("[[file]]\npath = \"{path}\"\nbytes = 3\nsha256 = \"{ABC}\"\nrole = \"{role}\"\n")
}

#[test]
fn well_formed_manifest_parses_sorted() {
    let m = Manifest::parse(&manifest_text(LIC, &(row("z.xsd", "xsd") + &row("a/b.xslt", "xslt")))).unwrap();
    let paths: Vec<&str> = m.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["a/b.xslt", "z.xsd"]);
    assert_eq!(m.files[0].bytes, 3);
    assert_eq!(m.licence_path, "L");
    assert_eq!(m.source.url, "u");
}

#[test]
fn malformed_manifests_fail_manifest_invalid() {
    let good_row = row("a.xsd", "xsd");
    let cases: Vec<(&str, String)> = vec![
        ("toml syntax", "manifest_version = \n".into()),
        ("duplicate key", format!("{}\nmanifest_version = 1\n", manifest_text(LIC, &good_row))),
        ("version 2", manifest_text(LIC, &good_row).replacen("manifest_version = 1", "manifest_version = 2", 1)),
        ("no version", manifest_text(LIC, &good_row).replacen("manifest_version = 1\n", "", 1)),
        ("unknown top key", manifest_text(&format!("{LIC}\nextra = 1"), &good_row)),
        ("no source", format!("manifest_version = 1\n{LIC}\n{good_row}")),
        ("no rows", manifest_text(LIC, "")),
        ("no licence", manifest_text("", &good_row)),
        ("licence is xsd", manifest_text(&LIC.replace("\"L\"", "\"L.xsd\""), &good_row)),
        ("licence is a row", manifest_text(&LIC.replace("\"L\"", "\"a.xsd\""), &good_row)),
        ("dup path", manifest_text(LIC, &(good_row.clone() + &good_row))),
        ("case collision", manifest_text(LIC, &(good_row.clone() + &row("A.xsd", "xsd")))),
        ("role vs ext", manifest_text(LIC, &row("a.xsd", "xslt"))),
        ("xsd role on .xsl", manifest_text(LIC, &row("a.xsl", "xsd"))),
        ("unknown role", manifest_text(LIC, &row("a.xsd", "dtd"))),
        ("non-schema row", manifest_text(LIC, &row("a.txt", "xsd"))),
        ("dotdot path", manifest_text(LIC, &row("../a.xsd", "xsd"))),
        ("absolute path", manifest_text(LIC, &row("/a.xsd", "xsd"))),
        ("backslash", manifest_text(LIC, &row("lib\\\\a.xsd", "xsd"))),
        ("drive", manifest_text(LIC, &row("C:/a.xsd", "xsd"))),
        ("url path", manifest_text(LIC, &row("http://x/a.xsd", "xsd"))),
        ("negative bytes", manifest_text(LIC, &good_row.replace("bytes = 3", "bytes = -1"))),
        ("string bytes", manifest_text(LIC, &good_row.replace("bytes = 3", "bytes = \"3\""))),
        ("huge bytes", manifest_text(LIC, &good_row.replace("bytes = 3", "bytes = 9000000000"))),
        ("upper hex", manifest_text(LIC, &good_row.replace("ba7816bf", "BA7816BF"))),
        ("no prefix", manifest_text(LIC, &good_row.replace("sha256:", ""))),
        ("unknown row key", manifest_text(LIC, &good_row.replace("role = ", "note = \"x\"\nrole = "))),
        ("missing sha", manifest_text(LIC, &format!("[[file]]\npath = \"a.xsd\"\nbytes = 3\nrole = \"xsd\"\n"))),
        ("empty url", manifest_text(LIC, &good_row).replace("url = \"u\"", "url = \"\"")),
        ("control char rev", manifest_text(LIC, &good_row).replace("rev = \"r\"", "rev = \"r\\u0007\"")),
        ("too large", format!("# {}\n{}", "x".repeat(1 << 20), manifest_text(LIC, &good_row))),
    ];
    for (name, text) in cases {
        match Manifest::parse(&text) {
            Ok(m) => panic!("{name}: parsed {m:?}"),
            Err(e) => assert_eq!(e.code(), "manifest_invalid", "{name}: {e}"),
        }
    }
}

#[test]
fn missing_manifest_fails_unreadable() {
    let dir = scratch("nomanifest");
    std::fs::write(dir.join("a.xsd"), b"abc").unwrap();
    assert_eq!(verify_err(&dir).code(), "schema_dir_unreadable");
}

#[test]
fn missing_directory_fails_unreadable() {
    let dir = scratch("gone").join("not-here");
    assert_eq!(verify_err(&dir).code(), "schema_dir_unreadable");
}

#[test]
fn non_utf8_manifest_fails_manifest_invalid() {
    let dir = scratch("latin1");
    std::fs::write(dir.join(MANIFEST_FILE), b"manifest_version = 1\n# \xff\n").unwrap();
    assert_eq!(verify_err(&dir).code(), "manifest_invalid");
}

#[test]
fn overly_deep_directories_fail_unreadable() {
    let dir = ok_copy("deep");
    let mut p = dir.clone();
    for _ in 0..=MAX_DEPTH {
        p.push("d");
    }
    std::fs::create_dir_all(&p).unwrap();
    assert_eq!(verify_err(&dir).code(), "schema_dir_unreadable");
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_never_followed() {
    use std::os::unix::fs::symlink;
    let outside = scratch("outside");
    std::fs::copy(fix("ok/b.xsd"), outside.join("b.xsd")).unwrap();

    // A linked schema file that the manifest does not list is unpinned.
    let dir = ok_copy("link-extra");
    symlink(outside.join("b.xsd"), dir.join("link.xsd")).unwrap();
    assert_eq!(verify_err(&dir), PinError::UnpinnedFile { path: "link.xsd".into() });

    // A pinned path that is a link (even to identical bytes) counts as missing.
    let dir = ok_copy("link-pinned");
    std::fs::remove_file(dir.join("b.xsd")).unwrap();
    symlink(outside.join("b.xsd"), dir.join("b.xsd")).unwrap();
    let err = verify_err(&dir);
    assert_eq!(err.code(), "xsd_digest_mismatch");
    assert!(matches!(err, PinError::DigestMismatch { actual: None, .. }));

    // A linked directory is not entered, so the files behind it are missing.
    let dir = ok_copy("link-dir");
    std::fs::remove_dir_all(dir.join("lib")).unwrap();
    let real_lib = scratch("real-lib");
    std::fs::copy(fix("ok/lib/units.xsd"), real_lib.join("units.xsd")).unwrap();
    symlink(&real_lib, dir.join("lib")).unwrap();
    let err = verify_err(&dir);
    assert_eq!(err.code(), "xsd_digest_mismatch");
    assert_eq!(err.path(), Some("lib/units.xsd"));
}
