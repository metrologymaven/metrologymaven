//! Acceptance criteria of spec node IO.CORP.HOSTILE (AC-1 .. AC-4), plus the requirements.
//!
//! Expected variant digests and errors come from the independent Python reference
//! `fixtures/io/corp/hostile/make_fixtures.py` (expected.txt); expected errors are
//! hand-labelled in `fixtures/io/corp/hostile/plan/hostile.toml`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_corp::{
    check_corpus, parse_plans, run_hostile, CheckOptions, Expect, FormatId, HostileEntry, HostileOptions,
    HostileReport, Manifest, Mutator, MutatorPlan, DEFAULT_MAX_BYTES, MAX_BOMB_BYTES,
};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/io/corp/hostile");

fn corpus() -> PathBuf {
    PathBuf::from(FIX)
}

fn plans() -> BTreeMap<FormatId, MutatorPlan> {
    parse_plans(&fs::read_to_string(corpus().join("plan/hostile.toml")).unwrap()).unwrap()
}

fn out_dir(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("corp-hostile-{name}"));
    let _ = fs::remove_dir_all(&p);
    p
}

fn run(name: &str, seed: u64) -> (PathBuf, HostileReport) {
    let out = out_dir(name);
    let report = run_hostile(&corpus(), &plans(), &out, &HostileOptions::new(seed)).unwrap();
    (out, report)
}

fn entries<'a>(r: &'a HostileReport, format: &str) -> &'a [HostileEntry] {
    r.formats.get(&FormatId::parse(format).unwrap()).map(Vec::as_slice).unwrap_or_default()
}

fn sha(bytes: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(bytes).into())
}

#[test]
fn variants_match_the_python_reference() {
    let (out, report) = run("reference", 42);
    let expected: Vec<(String, String, String, String)> = fs::read_to_string(corpus().join("expected.txt"))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            (f[0].to_string(), f[1].to_string(), f[2].to_string(), f[3].to_string())
        })
        .collect();
    let got: Vec<(String, String, String, String)> = report
        .formats
        .iter()
        .flat_map(|(f, es)| {
            es.iter().map(move |e| (f.to_string(), e.path.clone(), e.sha256.to_string(), e.error.clone()))
        })
        .collect();
    assert_eq!(got, expected);
    for (_, path, digest, _) in &got {
        assert_eq!(sha(&fs::read(out.join(path)).unwrap()).to_string(), *digest, "{path}");
    }
}

#[test]
fn ac1_stl_variants_are_tagged_and_reproducible() {
    let (out1, r1) = run("ac1-a", 42);
    let (out2, r2) = run("ac1-b", 42);
    let stl = entries(&r1, "mesh.stl");
    assert!(stl.len() >= 3);
    let text = fs::read_to_string(out1.join("mesh.stl.toml")).unwrap();
    let m = Manifest::parse(&text, "mesh.stl").unwrap();
    assert_eq!(m.entries.len(), stl.len());
    for e in &m.entries {
        assert!(e.tags.iter().any(|t| t == "hostile"));
        match &e.expect {
            Expect::Reject { error } => {
                assert!(["truncated_file", "malformed", "limit_exceeded"].contains(&error.as_str()), "{error}")
            }
            other => panic!("{other:?}"),
        }
    }
    // REQ-IO.CORP.HOSTILE-003: same seed and inputs, byte-identical variants and manifests.
    assert_eq!(r1, r2);
    for f in r1.formats.keys() {
        let name = format!("{f}.toml");
        assert_eq!(fs::read(out1.join(&name)).unwrap(), fs::read(out2.join(&name)).unwrap());
    }
    for e in r1.formats.values().flatten() {
        assert_eq!(fs::read(out1.join(&e.path)).unwrap(), fs::read(out2.join(&e.path)).unwrap());
    }
}

#[test]
fn ac2_zip_bomb_is_small_and_declares_more_than_the_byte_limit() {
    let (out, r) = run("ac2", 42);
    let bomb = entries(&r, "mesh.3mf").iter().find(|e| e.mutator == Mutator::ZipBomb).unwrap();
    assert_eq!(bomb.error, "limit_exceeded");
    let b = fs::read(out.join(&bomb.path)).unwrap();
    assert!(b.len() <= MAX_BOMB_BYTES && b.len() <= 1 << 20, "{} bytes", b.len());
    let u16_at = |p: usize| u64::from(u16::from_le_bytes([b[p], b[p + 1]]));
    let u32_at = |p: usize| u64::from(u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]));
    let eocd = b.len() - 22;
    assert_eq!(u32_at(eocd), 0x0605_4B50);
    let count = u16_at(eocd + 10);
    let mut p = u32_at(eocd + 16) as usize;
    let mut declared = 0u64;
    for _ in 0..count {
        assert_eq!(u32_at(p), 0x0201_4B50);
        assert_eq!(u16_at(p + 10), 8, "deflate");
        let csize = u32_at(p + 20);
        let usize_ = u32_at(p + 24);
        assert!(usize_ / csize >= 100, "ratio");
        declared += usize_;
        p += 46 + u16_at(p + 28) as usize + u16_at(p + 30) as usize + u16_at(p + 32) as usize;
    }
    assert_eq!(p, eocd);
    assert!(declared > DEFAULT_MAX_BYTES, "{declared}");
}

#[test]
fn ac3_billion_laughs_for_xml() {
    let (out, r) = run("ac3", 42);
    let qif = entries(&r, "qif.xml");
    let bl = qif.iter().find(|e| e.mutator == Mutator::BillionLaughs).unwrap();
    assert_eq!(bl.error, "xml_forbidden_construct");
    let text = String::from_utf8(fs::read(out.join(&bl.path)).unwrap()).unwrap();
    assert!(text.contains("<!DOCTYPE QIFDocument [") && text.contains("<!ENTITY a9 ") && text.contains("&a9;"));
    let nest = qif.iter().find(|e| e.mutator == Mutator::Nest).unwrap();
    assert_eq!(nest.error, "limit_exceeded");
}

#[test]
fn ac4_override_is_used_and_its_reason_recorded() {
    let (out, r) = run("ac4", 42);
    let flips: Vec<_> = entries(&r, "cloud.e57").iter().filter(|e| e.mutator == Mutator::BitFlip).collect();
    assert_eq!(flips.len(), 3);
    for e in &flips {
        assert_eq!(e.error, "checksum_mismatch");
        assert!(e.reason.as_deref().unwrap().contains("CRC-32C"));
    }
    let text = fs::read_to_string(out.join("cloud.e57.toml")).unwrap();
    assert!(text.contains("expected error overridden: IO.CLOUD.E57.PHYS REQ-001"));
    // Formats without an override keep the table default.
    let qif_flip = entries(&r, "qif.xml").iter().find(|e| e.mutator == Mutator::BitFlip).unwrap();
    assert_eq!((qif_flip.error.as_str(), qif_flip.reason.as_deref()), ("malformed", None));
}

#[test]
fn every_format_gets_the_required_kinds() {
    // REQ-IO.CORP.HOSTILE-001 and -002.
    let (_, r) = run("req1", 42);
    assert_eq!(r.formats.len(), 4);
    for (f, es) in &r.formats {
        for m in [Mutator::Truncate, Mutator::BitFlip, Mutator::InflateCount] {
            assert!(es.iter().any(|e| e.mutator == m), "{f} lacks {}", m.as_str());
        }
        assert!(es.iter().all(|e| !e.error.is_empty() && e.path.starts_with(&format!("{f}/"))));
    }
    assert!(entries(&r, "mesh.3mf").iter().any(|e| e.mutator == Mutator::ZipBomb));
}

#[test]
fn output_is_a_valid_corpus() {
    let (out, _) = run("valid", 42);
    let report = check_corpus(&out, &[], &CheckOptions::default());
    assert!(report.is_ok(), "{:?}", report.errors);
    assert_eq!(report.files_checked, 33);
}

#[test]
fn seed_changes_bit_flips_only() {
    let (_, a) = run("seed-a", 42);
    let (_, b) = run("seed-b", 7);
    let pick = |r: &HostileReport, m: Mutator| -> Vec<Digest> {
        r.formats.values().flatten().filter(|e| e.mutator == m).map(|e| e.sha256).collect()
    };
    assert_ne!(pick(&a, Mutator::BitFlip), pick(&b, Mutator::BitFlip));
    assert_eq!(pick(&a, Mutator::Truncate), pick(&b, Mutator::Truncate));
}

#[test]
fn output_cap_stops_before_writing() {
    let out = out_dir("cap");
    let opts = HostileOptions { max_total_bytes: 4096, ..HostileOptions::new(42) };
    let e = run_hostile(&corpus(), &plans(), &out, &opts).unwrap_err();
    assert_eq!(e.code(), "limit_exceeded");
    assert!(!out.exists());
}

#[test]
fn bad_plans_are_malformed() {
    let base = "[[format]]\nid = \"mesh.stl\"\ncount = { le = 4, at = 80 }\n";
    assert!(parse_plans(base).is_ok());
    for bad in [
        "[[format]]\nid = \"mesh.stl\"\n".to_string(),
        "[[format]]\nid = \"Mesh\"\ncount = { le = 4, at = 80 }\n".to_string(),
        base.replace("le = 4", "le = 9"),
        base.replace("at = 80", "at = -1"),
        format!("{base}flip = {{ start = 4, end = 4 }}\n"),
        format!("{base}extra = 1\n"),
        format!("{base}{base}"),
        format!("{base}[[format.override]]\nmutator = \"zip_bomb\"\nerror = \"x\"\nreason = \"r\"\n"),
        format!("{base}[[format.override]]\nmutator = \"bitflip\"\nerror = \"Bad\"\nreason = \"r\"\n"),
        format!("{base}[[format.override]]\nmutator = \"bitflip\"\nerror = \"x\"\nreason = \" \"\n"),
        format!("{base}[[format.override]]\nmutator = \"bitflip\"\nerror = \"x\"\n"),
        "format = 1".to_string(),
        "[[format".to_string(),
    ] {
        assert_eq!(parse_plans(&bad).map(|_| ()).map_err(|e| e.code()), Err("malformed"), "{bad}");
    }
    // A manifest whose format has no plan.
    let e = run_hostile(&corpus(), &BTreeMap::new(), &out_dir("noplan"), &HostileOptions::new(1)).unwrap_err();
    assert_eq!(e.code(), "malformed");
    // A count field the file does not have.
    let mut p = plans();
    let stl = FormatId::parse("mesh.stl").unwrap();
    p.get_mut(&stl).unwrap().count = fid_io_corp::CountField::Le { at: 10_000, width: 4 };
    let e = run_hostile(&corpus(), &p, &out_dir("nocount"), &HostileOptions::new(1)).unwrap_err();
    assert_eq!(e.code(), "malformed");
}

#[test]
fn cli_writes_the_same_corpus() {
    let (lib_out, _) = run("cli-lib", 42);
    let out = out_dir("cli");
    let status = Command::new(env!("CARGO_BIN_EXE_corpus-hostile"))
        .args(["--corpus", FIX, "--plan", &format!("{FIX}/plan/hostile.toml"), "--seed", "42", "--out"])
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());
    for f in ["cloud.e57.toml", "mesh.3mf.toml", "mesh.stl.toml", "qif.xml.toml"] {
        assert_eq!(fs::read(out.join(f)).unwrap(), fs::read(lib_out.join(f)).unwrap(), "{f}");
    }
    let usage = Command::new(env!("CARGO_BIN_EXE_corpus-hostile")).args(["--corpus", FIX]).status().unwrap();
    assert_eq!(usage.code(), Some(2));
}

#[test]
fn output_must_be_disjoint_from_the_corpus() {
    let c = corpus();
    for out in [c.join("plan").join("out"), c.clone(), c.join(".."), c.join("new").join("deeper")] {
        let e = run_hostile(&c, &plans(), &out, &HostileOptions::new(42)).unwrap_err();
        assert_eq!(e.code(), "malformed", "{}", out.display());
    }
    assert!(!c.join("plan").join("out").exists() && !c.join("new").exists());
}

#[test]
fn manifests_count_toward_the_cap_before_any_write() {
    let (_, full) = run("cap-full", 42);
    let variants: u64 = full.formats.values().flatten().map(|e| e.bytes).sum();
    assert!(full.bytes_written > variants);
    let out = out_dir("cap-manifests");
    let opts = HostileOptions { max_total_bytes: variants, ..HostileOptions::new(42) };
    assert_eq!(run_hostile(&corpus(), &plans(), &out, &opts).unwrap_err().code(), "limit_exceeded");
    assert!(!out.exists());
    let opts = HostileOptions { max_total_bytes: full.bytes_written, ..HostileOptions::new(42) };
    assert!(run_hostile(&corpus(), &plans(), &out, &opts).is_ok());
}

#[test]
fn rerun_replaces_files() {
    let out = out_dir("rerun");
    let a = run_hostile(&corpus(), &plans(), &out, &HostileOptions::new(1)).unwrap();
    let b = run_hostile(&corpus(), &plans(), &out, &HostileOptions::new(1)).unwrap();
    assert_eq!(a, b);
}

#[cfg(unix)]
#[test]
fn symlinks_and_hard_links_are_never_written_through() {
    // A symlinked output folder is refused and its target stays empty.
    let victim = out_dir("victim");
    fs::create_dir_all(&victim).unwrap();
    let out = out_dir("symlinked");
    fs::create_dir_all(&out).unwrap();
    std::os::unix::fs::symlink(&victim, out.join("mesh.stl")).unwrap();
    let e = run_hostile(&corpus(), &plans(), &out, &HostileOptions::new(42)).unwrap_err();
    assert_eq!(e.code(), "malformed");
    assert_eq!(fs::read_dir(&victim).unwrap().count(), 0);
    let linked_out = out_dir("symlinked-root");
    std::os::unix::fs::symlink(&victim, &linked_out).unwrap();
    assert!(run_hostile(&corpus(), &plans(), &linked_out, &HostileOptions::new(42)).is_err());
    assert_eq!(fs::read_dir(&victim).unwrap().count(), 0);
    // A hard link to a generated file keeps its bytes when the file is regenerated.
    let (out, r) = run("hardlink", 42);
    let flip = r.formats.values().flatten().find(|e| e.mutator == Mutator::BitFlip).unwrap();
    let keep = victim.join("kept");
    fs::hard_link(out.join(&flip.path), &keep).unwrap();
    let before = fs::read(&keep).unwrap();
    run_hostile(&corpus(), &plans(), &out, &HostileOptions::new(7)).unwrap();
    assert_eq!(fs::read(&keep).unwrap(), before);
    assert_ne!(fs::read(out.join(&flip.path)).unwrap(), before);
}

#[test]
fn e57_reader_returns_the_labelled_errors() {
    let (out, r) = run("e57-reader", 42);
    let es = entries(&r, "cloud.e57");
    assert!(!es.is_empty());
    for e in es {
        let bytes = fs::read(out.join(&e.path)).unwrap();
        let got = fid_io_e57::LogicalReader::open(std::io::Cursor::new(bytes)).and_then(|mut rd| rd.verify_all());
        assert_eq!(got.map_err(|x| x.code()), Err(e.error.as_str()), "{}", e.path);
    }
}

#[test]
fn xml_reader_returns_the_labelled_errors() {
    // The syntax-level mutators; inflate_count needs the QIF reader (count rules), not fid-xml.
    let (out, r) = run("xml-reader", 42);
    let mut checked = 0;
    for e in entries(&r, "qif.xml").iter().filter(|e| e.mutator != Mutator::InflateCount) {
        let bytes = fs::read(out.join(&e.path)).unwrap();
        let budget =
            fid_io_reg::Budget::new(fid_io_reg::Principal::Human, &fid_io_reg::LimitRequest::default()).unwrap();
        let got = fid_xml::read_slice(&bytes, budget).map(|_| ());
        assert_eq!(got.map_err(|x| x.code()), Err(e.error.as_str()), "{}", e.path);
        checked += 1;
    }
    assert_eq!(checked, 8);
    // The unmutated input is accepted.
    let budget = fid_io_reg::Budget::new(fid_io_reg::Principal::Human, &fid_io_reg::LimitRequest::default()).unwrap();
    assert!(fid_xml::read_slice(&fs::read(corpus().join("min.qif")).unwrap(), budget).is_ok());
}
