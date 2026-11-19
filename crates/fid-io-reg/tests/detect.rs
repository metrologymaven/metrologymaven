//! Acceptance criteria of spec node IO.REG.DETECT (AC-1 .. AC-7) and the requirements and
//! pinned Design rules behind them. Fixtures and expected values come from
//! `fixtures/io/reg/detect/`, written by the independent reference script
//! `make_fixtures.py` next to them.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_io_reg::detect::list::{self, ListEvent, EXPORT_LIST_FORMATS, IMPORT_LIST_FORMATS};
use fid_io_reg::detect::{extension_of, MAX_CONTAINS_WINDOW, MAX_DESCRIPTORS, MAX_SIGNATURES};
use fid_io_reg::{
    read_prefix, DetectError, Direction, FormatDescriptor, FormatId, IoError, Matcher, Origin, OriginFilter, Phase,
    Predicate, Registry, Signature, VersionRange, DETECT_PREFIX_LEN,
};
use fid_replay::ReplayOutcome;
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/reg/detect").join(name)
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap()
}

fn expected() -> BTreeMap<String, String> {
    let text = String::from_utf8(read("expected.txt")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (k, v) = l.split_once(' ').unwrap();
            (k.to_string(), v.trim().to_string())
        })
        .collect()
}

fn fid(s: &str) -> FormatId {
    FormatId::new(s).unwrap()
}

fn ids(list: &str) -> Vec<FormatId> {
    list.split(',').map(fid).collect()
}

fn registry() -> Registry {
    Registry::from_json(&read("registry.json")).unwrap()
}

fn schema() -> CValue {
    CValue::Obj(BTreeMap::new())
}

fn desc(id: &str, sigs: Vec<Signature>, exts: &[&str], origin: Origin, dir: Direction) -> FormatDescriptor {
    FormatDescriptor::new(
        fid(id),
        VersionRange::new("1", "*").unwrap(),
        sigs,
        exts.iter().map(|e| e.to_string()).collect(),
        schema(),
        Phase::new(0).unwrap(),
        origin,
        dir,
    )
    .unwrap()
}

fn bytes_sig(b: &[u8], priority: u8) -> Signature {
    Signature::new(0, Matcher::Bytes(b.to_vec()), priority).unwrap()
}

/// Detects a whole file as the import path would: capped prefix plus the true length.
fn detect_file(reg: &Registry, data: &[u8], name: &str, named: Option<&FormatId>) -> Result<FormatId, DetectError> {
    reg.detect_reader(data, data.len() as u64, Some(name), named)
}

/// A reader that counts how many bytes it hands out.
struct Counting<R> {
    inner: R,
    served: u64,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.served += n as u64;
        Ok(n)
    }
}

// ------------------------------------------------------------------ AC-1 / REQ-001

#[test]
fn ac1_import_list_formats_json_matches_reference() {
    let e = expected();
    let out = list::list_formats(&registry(), Direction::Import, OriginFilter::Any).unwrap();
    // `fid import list-formats --json` prints the canonical envelope.
    assert_eq!(out.envelope.encode().unwrap(), read("list_import_any.json"));
    let CValue::Arr(items) = out.envelope.result.clone().unwrap() else { panic!("result is not an array") };
    assert_eq!(items.len().to_string(), e["list.count"]);
    let listed: Vec<String> = items
        .iter()
        .map(|i| match i {
            CValue::Obj(o) => match &o["id"] {
                CValue::Str(s) => s.clone(),
                _ => panic!("id"),
            },
            _ => panic!("item"),
        })
        .collect();
    assert_eq!(listed.join(","), e["list.ids"]);
    // Every REQ-001 field on every descriptor.
    for i in &items {
        let CValue::Obj(o) = i else { panic!() };
        let keys: Vec<&str> = o.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["direction", "extensions", "id", "options_schema", "origin", "phase", "signatures", "versions"]
        );
    }
    assert_eq!(out.event.result_sha256.as_str(), e["list.result_sha256"]);
    assert_eq!(out.event.command.as_str(), IMPORT_LIST_FORMATS);
}

#[test]
fn ac1_listing_round_trips_through_descriptor_json() {
    let reg = registry();
    for d in reg.list(Direction::Import, OriginFilter::Any) {
        assert_eq!(&FormatDescriptor::from_value(d.to_value()).unwrap(), d);
    }
}

#[test]
fn req001_export_listing_and_origin_filter() {
    let mut reg = registry();
    reg.register(desc("mesh.stl", vec![], &["stl"], Origin::FirstParty, Direction::Export)).unwrap();
    reg.register(desc("vendor.jt", vec![bytes_sig(b"JT", 1)], &["jt"], Origin::Plugin, Direction::Import)).unwrap();

    let ex = list::list_formats(&reg, Direction::Export, OriginFilter::Any).unwrap();
    assert_eq!(ex.envelope.command_id, EXPORT_LIST_FORMATS);
    let CValue::Arr(items) = ex.envelope.result.unwrap() else { panic!() };
    assert_eq!(items.len(), 1);

    let ids_of = |f| reg.list(Direction::Import, f).into_iter().map(|d| d.id().as_str()).collect::<Vec<_>>();
    assert_eq!(ids_of(OriginFilter::Plugin), ["vendor.jt"]);
    assert_eq!(ids_of(OriginFilter::FirstParty), ["cloud.e57", "mesh.ply", "mesh.stl", "mesh.stl-ascii", "qif.xml"]);
    assert_eq!(ids_of(OriginFilter::Any).len(), 6);
    assert_eq!(OriginFilter::default(), OriginFilter::Any);
}

#[test]
fn req001_listing_is_independent_of_registration_order() {
    let reg = registry();
    let mut rev = Registry::new();
    for d in reg.list(Direction::Import, OriginFilter::Any).into_iter().rev() {
        rev.register(d.clone()).unwrap();
    }
    let a = list::list_formats(&reg, Direction::Import, OriginFilter::Any).unwrap();
    let b = list::list_formats(&rev, Direction::Import, OriginFilter::Any).unwrap();
    assert_eq!(a.envelope.encode().unwrap(), b.envelope.encode().unwrap());
}

// ------------------------------------------------------------------ AC-2 .. AC-5

#[test]
fn ac2_ply_named_stl_is_a_mismatch() {
    let e = expected();
    assert_eq!(e["ac2.code"], "format_mismatch");
    let err = detect_file(&registry(), &read("ply_named.stl"), "ply_named.stl", None).unwrap_err();
    assert_eq!(
        err,
        DetectError::Mismatch {
            detected: fid(&e["ac2.detected"]),
            extension: e["ac2.extension"].clone(),
            extension_formats: ids(&e["ac2.extension_formats"]),
        }
    );
    assert_eq!(err.code(), "format_mismatch");
    assert_eq!(IoError::from(err.clone()), IoError::FormatMismatch);
    let shown = err.to_string();
    assert!(shown.contains("mesh.ply") && shown.contains("mesh.stl"), "{shown}");
}

#[test]
fn ac3_named_format_wins() {
    let e = expected();
    let named = fid("mesh.ply");
    let got = detect_file(&registry(), &read("ply_named.stl"), "ply_named.stl", Some(&named)).unwrap();
    assert_eq!(got.as_str(), e["ac3.selected"]);
    // A named format reads nothing from the file.
    let data = read("ply_named.stl");
    let mut src = Counting { inner: &data[..], served: 0 };
    registry().detect_reader(&mut src, data.len() as u64, Some("x.stl"), Some(&named)).unwrap();
    assert_eq!(src.served, 0);
}

#[test]
fn ac4_binary_stl_with_solid_header_is_binary() {
    let e = expected();
    let data = read("binary_stl_solid_header.stl");
    assert!(data.starts_with(b"solid"));
    let got = detect_file(&registry(), &data, "binary_stl_solid_header.stl", None).unwrap();
    assert_eq!(got.as_str(), e["ac4.selected"]);
    // Without the size predicate the ASCII keyword would win: the outcome is the priority rule.
    let one_byte_longer = [data.as_slice(), b"\n"].concat();
    assert_eq!(detect_file(&registry(), &one_byte_longer, "a.stl", None).unwrap().as_str(), "mesh.stl-ascii");
}

#[test]
fn ac5_random_bytes_are_unknown_and_read_is_capped() {
    let e = expected();
    let data = read("random_64k.bin");
    assert_eq!(data.len().to_string(), e["random_64k.size"]);
    let cap: u64 = e["ac5.max_read"].parse().unwrap();
    assert_eq!(cap, DETECT_PREFIX_LEN as u64);

    let mut src = Counting { inner: &data[..], served: 0 };
    let err = registry().detect_reader(&mut src, data.len() as u64, Some("random_64k.bin"), None).unwrap_err();
    assert_eq!(err.code(), e["ac5.code"]);
    assert_eq!(err, DetectError::Unknown);
    assert!(src.served <= cap);

    // A much longer stream is still read only up to the cap.
    let long = io::Read::chain(&data[..], io::repeat(7).take(4 * 1024 * 1024));
    let mut src = Counting { inner: long, served: 0 };
    let total = data.len() as u64 + 4 * 1024 * 1024;
    assert_eq!(registry().detect_reader(&mut src, total, None, None).unwrap_err(), DetectError::Unknown);
    assert_eq!(src.served, cap);
}

// ------------------------------------------------------------------ AC-6 / REQ-006

#[test]
fn ac6_duplicate_registration_fails() {
    let mut reg = Registry::new();
    reg.register(desc("mesh.stl", vec![bytes_sig(b"a", 1)], &["stl"], Origin::FirstParty, Direction::Import)).unwrap();
    let second = desc("mesh.stl", vec![bytes_sig(b"b", 2)], &[], Origin::Plugin, Direction::Import);
    let err = reg.register(second).unwrap_err();
    assert_eq!(err, DetectError::Duplicate { id: fid("mesh.stl"), direction: Direction::Import });
    assert_eq!(err.code(), "format_duplicate");
    // The first registration is untouched.
    assert_eq!(reg.len(), 1);
    assert_eq!(reg.get(&fid("mesh.stl"), Direction::Import).unwrap().origin(), Origin::FirstParty);
    // Same ID in the other direction is a different key.
    reg.register(desc("mesh.stl", vec![], &[], Origin::FirstParty, Direction::Export)).unwrap();
    assert_eq!(reg.len(), 2);
}

#[test]
fn ac6_duplicate_in_registry_document_fails() {
    let text = String::from_utf8(read("registry.json")).unwrap();
    let v = fid_canon::parse(text.as_bytes()).unwrap();
    let CValue::Obj(mut o) = v else { panic!() };
    let CValue::Arr(mut f) = o.remove("formats").unwrap() else { panic!() };
    f.push(f[1].clone());
    o.insert("formats".into(), CValue::Arr(f));
    let doc = fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap();
    assert_eq!(Registry::from_json(&doc).unwrap_err().code(), "format_duplicate");
}

// ------------------------------------------------------------------ AC-7 replay

#[test]
fn ac7_exact_replay_of_listing_is_identical() {
    let reg = registry();
    let out = list::list_formats(&reg, Direction::Import, OriginFilter::Any).unwrap();
    let journal = out.event.to_json().unwrap();
    let event = ListEvent::from_json(&journal).unwrap();
    assert_eq!(event, out.event);
    assert_eq!(list::replay(&event, &registry()).unwrap(), ReplayOutcome::Identical);

    // A registry that lists differently diverges.
    let mut more = registry();
    more.register(desc("vendor.jt", vec![], &["jt"], Origin::Plugin, Direction::Import)).unwrap();
    assert_eq!(list::replay(&event, &more).unwrap(), ReplayOutcome::Divergent);
    // The filter is part of the record.
    let fp = ListEvent { origin: OriginFilter::FirstParty, ..event.clone() };
    assert_eq!(list::replay(&fp, &more).unwrap(), ReplayOutcome::Identical);
}

#[test]
fn ac7_journal_record_is_strict() {
    let good = list::list_formats(&registry(), Direction::Import, OriginFilter::Any).unwrap().event.to_json().unwrap();
    let good = String::from_utf8(good).unwrap();
    for bad in [
        good.replace("import.list_formats@1", "import.file@1"),
        good.replace("\"any\"", "\"all\""),
        good.replace("\"params\"", "\"param\""),
        good.replace("}", ",\"extra\":1}"),
        good.replace("sha256:", "sha1:"),
        format!("{{\"command\":\"import.list_formats@1\",{}", &good[1..]),
        "[]".to_string(),
        "x".repeat(list::MAX_EVENT_JSON_BYTES + 1),
    ] {
        assert!(ListEvent::from_json(bad.as_bytes()).is_err(), "{bad}");
    }
}

// ------------------------------------------------------------------ REQ-002 .. REQ-005 rules

fn small_registry() -> Registry {
    let mut reg = Registry::new();
    let r = Direction::Import;
    reg.register(desc("a.one", vec![bytes_sig(b"MAGIC", 5)], &["one"], Origin::FirstParty, r)).unwrap();
    reg.register(desc("b.two", vec![bytes_sig(b"MAG", 5)], &["two"], Origin::FirstParty, r)).unwrap();
    reg.register(desc("c.three", vec![bytes_sig(b"MAGIC!", 9)], &["three"], Origin::Plugin, r)).unwrap();
    reg.register(desc("d.write", vec![bytes_sig(b"W", 255)], &["w"], Origin::FirstParty, Direction::Export)).unwrap();
    reg
}

#[test]
fn req003_tie_at_top_priority_is_ambiguous_with_sorted_candidates() {
    let reg = small_registry();
    let err = reg.detect(b"MAGIC?", 6, None, None).unwrap_err();
    assert_eq!(err, DetectError::Ambiguous { priority: 5, candidates: ids("a.one,b.two") });
    assert_eq!(err.code(), "format_ambiguous");
    // A higher priority beats the tie.
    assert_eq!(reg.detect(b"MAGIC!", 6, None, None).unwrap().as_str(), "c.three");
    // Lower-priority ties do not matter when one format is on top.
    assert_eq!(reg.detect(b"MAGIC!", 6, Some("x.three"), None).unwrap().as_str(), "c.three");
}

#[test]
fn req003_registration_order_does_not_change_the_outcome() {
    let mut rev = Registry::new();
    let reg = small_registry();
    for d in reg.list(Direction::Import, OriginFilter::Any).into_iter().rev() {
        rev.register(d.clone()).unwrap();
    }
    for input in [&b"MAGIC?"[..], b"MAGIC!", b"MAG", b"zzz"] {
        assert_eq!(reg.detect(input, input.len() as u64, None, None), rev.detect(input, input.len() as u64, None, None));
    }
}

#[test]
fn req002_extension_is_ignored_for_selection_and_writers_are_not_candidates() {
    let reg = small_registry();
    // The content says c.three; an unregistered extension never matters.
    assert_eq!(reg.detect(b"MAGIC!", 6, Some("file.unknownext"), None).unwrap().as_str(), "c.three");
    // A write-only signature never selects a reader.
    assert_eq!(reg.detect(b"W", 1, None, None).unwrap_err(), DetectError::Unknown);
    // A registered extension of another format is a mismatch.
    assert!(matches!(reg.detect(b"MAGIC!", 6, Some("file.ONE"), None), Err(DetectError::Mismatch { .. })));
    // A writer's extension is not a reader mapping.
    assert_eq!(reg.detect(b"MAGIC!", 6, Some("file.w"), None).unwrap().as_str(), "c.three");
}

#[test]
fn req002_only_the_first_64k_is_examined() {
    let mut reg = Registry::new();
    let at_end = Signature::new((DETECT_PREFIX_LEN - 2) as u32, Matcher::Bytes(b"OK".to_vec()), 1).unwrap();
    reg.register(desc("x.end", vec![at_end], &[], Origin::FirstParty, Direction::Import)).unwrap();
    let mut data = vec![0u8; DETECT_PREFIX_LEN + 10];
    data[DETECT_PREFIX_LEN - 2..DETECT_PREFIX_LEN].copy_from_slice(b"OK");
    assert_eq!(reg.detect(&data, data.len() as u64, None, None).unwrap().as_str(), "x.end");
    // A signature reaching past the prefix cannot be registered.
    assert!(Signature::new((DETECT_PREFIX_LEN - 1) as u32, Matcher::Bytes(b"OK".to_vec()), 1).is_err());
    // Bytes beyond 64 KiB are never looked at.
    let mut late = vec![0u8; DETECT_PREFIX_LEN + 10];
    late[DETECT_PREFIX_LEN..DETECT_PREFIX_LEN + 2].copy_from_slice(b"OK");
    assert_eq!(reg.detect(&late, late.len() as u64, None, None).unwrap_err(), DetectError::Unknown);
}

#[test]
fn req004_unknown_and_unregistered_named() {
    let reg = small_registry();
    assert_eq!(reg.detect(b"", 0, Some("a.one"), None).unwrap_err(), DetectError::Unknown);
    let err = reg.detect(b"MAGIC!", 6, None, Some(&fid("no.such"))).unwrap_err();
    assert_eq!(err, DetectError::NotRegistered { named: fid("no.such") });
    assert_eq!(err.code(), "format_unknown");
    // A writer is not a reader.
    assert_eq!(err.code(), reg.detect(b"", 0, None, Some(&fid("d.write"))).unwrap_err().code());
}

#[test]
fn detect_refuses_inconsistent_length() {
    let reg = small_registry();
    let err = reg.detect(b"MAGIC!", 3, None, None).unwrap_err();
    assert_eq!(err.code(), "malformed");
}

#[test]
fn stl_size_predicate() {
    let p = Predicate::StlBinarySize;
    let mut f = vec![0u8; 84];
    assert!(p.eval(&f, 84));
    f[80] = 2;
    assert!(p.eval(&f, 184));
    assert!(!p.eval(&f, 183));
    assert!(!p.eval(&f[..83], 184));
    f[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(p.eval(&f, 84 + 50 * u64::from(u32::MAX)));
    assert!(Signature::new(4, Matcher::Predicate(p), 1).is_err());
    assert_eq!(Predicate::from_wire("stl_binary_size"), Some(p));
    assert_eq!(Predicate::from_wire("shell"), None);
}

#[test]
fn contains_signature_is_windowed() {
    let s = Signature::new(2, Matcher::Contains { needle: b"<Q".to_vec(), window: 8 }, 1).unwrap();
    assert!(s.matches(b"xx  <Q", 6));
    assert!(s.matches(b"xx      <Q", 10));
    assert!(!s.matches(b"xx       <Q", 11)); // needle ends past offset + window
    assert!(!s.matches(b"<Q", 2)); // before the offset
    assert!(!s.matches(b"x", 1));
    assert!(Signature::new(0, Matcher::Contains { needle: b"abc".to_vec(), window: 2 }, 1).is_err());
    assert!(Signature::new(0, Matcher::Contains { needle: b"a".to_vec(), window: MAX_CONTAINS_WINDOW + 1 }, 1).is_err());
    assert!(Signature::new(0, Matcher::Contains { needle: vec![], window: 4 }, 1).is_err());
}

#[test]
fn extension_parsing_is_lexical_and_strict() {
    assert_eq!(extension_of("a.STL").as_deref(), Some("stl"));
    assert_eq!(extension_of("dir.ply/a.stl").as_deref(), Some("stl"));
    assert_eq!(extension_of("C:\\x.ply\\a.e57").as_deref(), Some("e57"));
    assert_eq!(extension_of("a.tar.gz").as_deref(), Some("gz"));
    for none in ["", "stl", ".stl", "dir/.stl", "a.", "a.st l", "a.\u{e9}", "a.abcdefghijklmnopq", "a.stl/", "../"] {
        assert_eq!(extension_of(none), None, "{none:?}");
    }
}

#[test]
fn read_prefix_caps_and_reports_errors() {
    struct Broken(u32);
    impl Read for Broken {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.0 == 0 {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "no"));
            }
            self.0 -= 1;
            let n = buf.len().min(10);
            buf[..n].fill(1);
            Ok(n)
        }
    }
    assert_eq!(read_prefix(Broken(3)).unwrap_err(), DetectError::Io { kind: io::ErrorKind::PermissionDenied, at: 30 });
    assert_eq!(read_prefix(io::repeat(0)).unwrap().len(), DETECT_PREFIX_LEN);
    assert_eq!(read_prefix(&b"abc"[..]).unwrap(), b"abc");
    let io_err: IoError = DetectError::Io { kind: io::ErrorKind::PermissionDenied, at: 30 }.into();
    assert_eq!(io_err.code(), "io");
}

// ------------------------------------------------------------------ descriptor validation

#[test]
fn descriptor_validation_fails_closed() {
    let v = || VersionRange::new("1", "*").unwrap();
    let mk = |sigs: Vec<Signature>, exts: Vec<&str>, schema: CValue| {
        FormatDescriptor::new(
            fid("x.y"),
            v(),
            sigs,
            exts.into_iter().map(String::from).collect(),
            schema,
            Phase::new(0).unwrap(),
            Origin::FirstParty,
            Direction::Import,
        )
    };
    assert!(mk(vec![bytes_sig(b"a", 1); MAX_SIGNATURES + 1], vec![], schema()).is_err());
    assert!(mk(vec![], vec!["STL"], schema()).is_err());
    assert!(mk(vec![], vec![".stl"], schema()).is_err());
    assert!(mk(vec![], vec!["stl", "stl"], schema()).is_err());
    assert!(mk(vec![], vec![], CValue::Arr(vec![])).is_err());
    assert!(mk(vec![], vec![], CValue::Obj(BTreeMap::from([("n".to_string(), CValue::Num(f64::NAN))]))).is_err());
    let sorted = mk(vec![], vec!["xml", "qif"], schema()).unwrap();
    assert_eq!(sorted.extensions(), ["qif", "xml"]);
    assert!(Signature::new(0, Matcher::Bytes(vec![]), 1).is_err());
    assert!(Signature::new(0, Matcher::Bytes(vec![0; 257]), 1).is_err());
    assert!(VersionRange::new("", "1").is_err());
    assert!(VersionRange::new("1 2", "1").is_err());
    assert!(Phase::parse("P10").is_none() && Phase::parse("p0").is_none() && Phase::new(10).is_none());
    assert_eq!(Phase::parse("P3").unwrap().to_string(), "P3");
}

#[test]
fn registry_capacity_is_bounded() {
    let mut reg = Registry::new();
    for i in 0..MAX_DESCRIPTORS {
        reg.register(desc(&format!("f{i}"), vec![], &[], Origin::Plugin, Direction::Import)).unwrap();
    }
    let err = reg.register(desc("one.more", vec![], &[], Origin::Plugin, Direction::Import)).unwrap_err();
    assert_eq!(err.code(), "malformed");
    assert_eq!(reg.len(), MAX_DESCRIPTORS);
}

#[test]
fn registry_document_is_strict() {
    let good = String::from_utf8(read("registry.json")).unwrap();
    let cases = [
        good.replacen("\"phase\": \"P0\"", "\"phase\": \"P0\", \"phase\": \"P1\"", 1), // duplicate key
        good.replacen("\"direction\": \"read\"", "\"direction\": \"read\", \"exec\": \"x\"", 1), // unknown field
        good.replacen("\"offset\": 0", "\"offset\": 0.5", 1),
        good.replacen("\"offset\": 0", "\"offset\": -1", 1),
        good.replacen("\"priority\": 10", "\"priority\": 256", 1),
        good.replacen("\"predicate\": \"stl_binary_size\"", "\"predicate\": \"run_script\"", 1),
        good.replacen("\"bytes\": \"736f6c6964\"", "\"bytes\": \"736F6C6964\"", 1),
        good.replacen("\"bytes\": \"736f6c6964\"", "\"bytes\": \"736f6c6964\", \"predicate\": \"stl_binary_size\"", 1),
        good.replacen("\"origin\": \"first_party\"", "\"origin\": \"vendor\"", 1),
        good.replacen("\"id\": \"mesh.ply\"", "\"id\": \"../mesh\"", 1),
        format!("{}{}", "[".repeat(100), "]".repeat(100)),
        "{\"formats\": []} trailing".to_string(),
    ];
    for (i, c) in cases.iter().enumerate() {
        assert_ne!(c, &good, "case {i} did not change the document");
        let err = Registry::from_json(c.as_bytes()).unwrap_err();
        assert_eq!(err.code(), "malformed", "case {i}: {err}");
    }
    let huge = vec![b' '; fid_io_reg::detect::MAX_REGISTRY_JSON_BYTES + 1];
    assert_eq!(Registry::from_json(&huge).unwrap_err().code(), "malformed");
    assert!(Registry::from_json(b"{\"formats\": []}").unwrap().is_empty());
}

#[test]
fn hostile_prefixes_never_panic() {
    // A small deterministic sweep (seeded LCG) over short and long prefixes and odd lengths.
    let reg = registry();
    let mut s: u64 = 1;
    let mut next = || {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (s >> 33) as u8
    };
    for len in [0usize, 1, 5, 79, 80, 83, 84, 85, 134, 1023, 1024, 1025, 70_000] {
        let data: Vec<u8> = (0..len).map(|_| next()).collect();
        for total in [len as u64, len as u64 + 1, u64::MAX] {
            let _ = reg.detect(&data, total, Some("x.stl"), None);
        }
    }
}
