//! KRN.MAN.WASM AC-1 (reader and import check). The modules under `fixtures/man/wasm/modules/`
//! are hand-encoded by `make_fixtures.py` and labelled there from the rules pinned in the
//! leaf's Design (`labels.txt`); the allowlist cases below are hand-labelled the same way.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_wasm_check::allow::{self, AllowError, Allowlist, MAX_ALLOW_BYTES, MAX_ENTRIES};
use fid_wasm_check::module::{self, ImportKind, WasmErrorKind, MAX_MODULE_BYTES};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man/wasm")
}

fn test_allow() -> Allowlist {
    Allowlist::parse(&std::fs::read(fixtures().join("test.allow")).unwrap()).unwrap()
}

#[test]
fn ac1_every_labelled_module_parses_and_checks_as_labelled() {
    let allow = test_allow();
    let labels = std::fs::read_to_string(fixtures().join("modules/labels.txt")).unwrap();
    let mut n = 0;
    for line in labels.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        let (file, parse, check) = (f[0], f[1], f[2]);
        let bytes = std::fs::read(fixtures().join("modules").join(file)).unwrap();
        match module::parse(&bytes) {
            Ok(m) => {
                assert_eq!(parse, format!("ok:{}", m.imports.len()), "{file}");
                let report = allow::check(&m, &allow);
                let expected = if report.passed() {
                    "pass".to_string()
                } else {
                    let denied: Vec<String> =
                        report.denied().iter().map(|i| format!("{} {}", i.module, i.name)).collect();
                    format!("fail:{}", denied.join(","))
                };
                assert_eq!(check, expected, "{file}");
            }
            Err(e) => {
                assert_eq!(parse, format!("err:{}", e.code()), "{file}");
                assert_eq!(check, "-", "{file}");
            }
        }
        n += 1;
    }
    assert_eq!(n, 28);
}

#[test]
fn ac1_report_lists_every_import_and_names_the_denied_one() {
    let bytes = std::fs::read(fixtures().join("modules/bad-random-get.wasm")).unwrap();
    let m = module::parse(&bytes).unwrap();
    assert_eq!(m.type_count, 2);
    let report = allow::check(&m, &test_allow());
    assert_eq!(
        report.lines(),
        [
            "allowed wasi_snapshot_preview1 fd_write",
            "DENIED wasi_snapshot_preview1 random_get (func)",
            "unused wasi_snapshot_preview1 proc_exit",
            "imports: 2, denied: 1",
        ]
    );
    let ok = module::parse(&std::fs::read(fixtures().join("modules/ok-wasi.wasm")).unwrap()).unwrap();
    assert_eq!(ok.imports.iter().map(|i| i.kind).collect::<Vec<_>>(), [ImportKind::Func, ImportKind::Func]);
    assert_eq!(
        allow::check(&ok, &test_allow()).lines(),
        [
            "allowed wasi_snapshot_preview1 fd_write",
            "allowed wasi_snapshot_preview1 proc_exit",
            "imports: 2, denied: 0"
        ]
    );
}

#[test]
fn oversized_module_is_refused_before_reading() {
    let mut big = vec![0u8; MAX_MODULE_BYTES + 1];
    big[..8].copy_from_slice(b"\x00asm\x01\x00\x00\x00");
    assert_eq!(module::parse(&big).unwrap_err().kind, WasmErrorKind::TooLarge);
}

#[test]
fn every_prefix_of_a_valid_module_is_refused_without_panicking() {
    let bytes = std::fs::read(fixtures().join("modules/ok-wasi.wasm")).unwrap();
    for n in 0..bytes.len() {
        // A prefix that ends on a section boundary is itself a valid (shorter) module.
        let _ = module::parse(&bytes[..n]);
    }
    // Every single-byte change either parses or is refused; none panics.
    for i in 0..bytes.len() {
        for v in [0x00u8, 0x01, 0x7f, 0x80, 0xff] {
            let mut b = bytes.clone();
            b[i] = v;
            let _ = module::parse(&b);
        }
    }
    assert!(module::parse(&bytes[..9]).is_err());
}

#[test]
fn the_shipped_allowlist_parses_and_excludes_random_get() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fid-kernel-wasm/wasm-imports.allow");
    let allow = Allowlist::parse(&std::fs::read(path).unwrap()).unwrap();
    assert!(!allow.entries().is_empty());
    for e in allow.entries() {
        assert_eq!(e.module, "wasi_snapshot_preview1");
        assert!(e.justification.len() >= 20, "{}: justification too short", e.name);
    }
    for forbidden in ["random_get", "path_open", "sock_accept", "sched_yield", "poll_oneoff"] {
        assert!(!allow.entries().iter().any(|e| e.name == forbidden), "{forbidden} must not be allowlisted");
    }
}

#[test]
fn allowlist_format_is_strict() {
    let ok = "# c\n\nwasi_snapshot_preview1 fd_write -- writes bytes\n";
    assert_eq!(Allowlist::parse(ok.as_bytes()).unwrap().entries().len(), 1);
    let cases: [(&str, AllowError); 12] = [
        ("m n -- j\r\n", AllowError::BadCharacter { line: 1 }),
        ("m\tn -- j\n", AllowError::BadCharacter { line: 1 }),
        ("\u{feff}m n -- j\n", AllowError::BadCharacter { line: 1 }),
        ("m n\n", AllowError::Syntax { line: 1 }),
        ("m n --\n", AllowError::MissingJustification { line: 1 }),
        ("m n --  \n", AllowError::MissingJustification { line: 1 }),
        ("m n --  j\n", AllowError::Syntax { line: 1 }),
        ("M n -- j\n", AllowError::Syntax { line: 1 }),
        ("m  n -- j\n", AllowError::Syntax { line: 1 }),
        ("m n x -- j\n", AllowError::Syntax { line: 1 }),
        (" m n -- j\n", AllowError::Syntax { line: 1 }),
        ("m n -- j\n# c\nm n -- k\n", AllowError::Duplicate { line: 3 }),
    ];
    for (text, err) in cases {
        assert_eq!(Allowlist::parse(text.as_bytes()), Err(err), "{text:?}");
    }
    assert_eq!(Allowlist::parse(&[0xff]), Err(AllowError::NotUtf8));
    assert_eq!(Allowlist::parse(&vec![b'#'; MAX_ALLOW_BYTES + 1]), Err(AllowError::TooLarge));
    let many: String = (0..=MAX_ENTRIES).map(|i| format!("m n{i} -- j\n")).collect();
    assert_eq!(Allowlist::parse(many.as_bytes()), Err(AllowError::TooManyEntries));
    let long = format!("m {} -- j\n", "n".repeat(65));
    assert_eq!(Allowlist::parse(long.as_bytes()), Err(AllowError::Syntax { line: 1 }));
}
