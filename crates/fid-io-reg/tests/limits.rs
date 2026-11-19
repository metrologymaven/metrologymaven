//! Acceptance criteria of spec node IO.REG.LIMITS (AC-1 .. AC-6) and the requirements
//! behind them. Expected values come from `fixtures/io/reg/limits/expected.txt`, written
//! by the independent reference script `make_fixtures.py` next to it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_reg::toy::{self, TOY_MAX_RECURSION};
use fid_io_reg::{
    BoundedSource, Budget, Clock, IoError, LimitKind, LimitRequest, Limits, Position, Principal, UntrustedText, CODES,
    DEFAULT_ALLOC, DEFAULT_BYTES, DEFAULT_DEPTH, DEFAULT_ELEMENTS, DEFAULT_WALL_MS, WALL_CHECK_EVERY,
};
use fid_types::Determinism;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/reg/limits").join(name)
}

fn expected() -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(fixture("expected.txt")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (k, v) = l.split_once(' ').unwrap();
            (k.to_string(), v.trim().to_string())
        })
        .collect()
}

fn num(e: &BTreeMap<String, String>, key: &str) -> u64 {
    e[key].parse().unwrap()
}

fn budget(principal: Principal, request: LimitRequest) -> Budget {
    Budget::new(principal, &request).unwrap()
}

// ------------------------------------------------------------------ REQ-001 defaults

#[test]
fn req001_documented_defaults() {
    assert_eq!(DEFAULT_BYTES, 64 * (1u64 << 30));
    assert_eq!(DEFAULT_ELEMENTS, 1u64 << 32);
    assert_eq!(DEFAULT_DEPTH, 64);
    assert_eq!(DEFAULT_ALLOC, 8 * (1u64 << 30));
    assert_eq!(DEFAULT_WALL_MS, 600 * 1000);
    let d = Limits::DEFAULT;
    assert_eq!(d, Limits::default());
    assert_eq!(
        LimitKind::ALL.iter().map(|k| d.value(*k)).collect::<Vec<_>>(),
        [DEFAULT_BYTES, DEFAULT_ELEMENTS, 64, DEFAULT_ALLOC, DEFAULT_WALL_MS]
    );
    let names: Vec<_> = LimitKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(names, ["bytes", "elements", "depth", "alloc", "wall"]);
}

// ------------------------------------------------------------------ AC-1

/// AC-1: deep_nest.json (depth 10,000) through the toy recursive-descent reader with
/// default limits fails `limit_exceeded{depth, 64}` without stack overflow. Run on a
/// small stack to show the recursion stays bounded.
#[test]
fn ac1_deep_nesting_fails_closed() {
    let e = expected();
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let file = File::open(fixture("deep_nest.json")).unwrap();
            let mut src = BoundedSource::new(file, budget(Principal::Agent, LimitRequest::default()));
            let r = toy::read(&mut src);
            (r, src.budget().depth())
        })
        .unwrap();
    let (result, depth_after) = handle.join().unwrap();
    assert_eq!(std::fs::metadata(fixture("deep_nest.json")).unwrap().len(), num(&e, "deep_nest.size"));
    assert_eq!(e["ac1.limit"], "depth");
    let err = result.unwrap_err();
    assert_eq!(
        err,
        IoError::LimitExceeded {
            limit: LimitKind::Depth,
            value: num(&e, "ac1.value"),
            at: Position::line_col(num(&e, "ac1.byte"), num(&e, "ac1.line") as u32, num(&e, "ac1.col") as u32),
        }
    );
    assert_eq!(err.code(), "limit_exceeded");
    assert_eq!(err.determinism(), Determinism::D0);
    // Every depth guard was released on the way out.
    assert_eq!(depth_after, 0);
}

/// A human may raise the depth limit, but the toy reader's own recursion cap still
/// keeps the stack bounded.
#[test]
fn ac1_human_raised_depth_still_bounded() {
    let deep = std::fs::read(fixture("deep_nest.json")).unwrap();
    let handle = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let b = budget(Principal::Human, LimitRequest::default().depth(1_000_000));
            toy::read_slice(&deep, b)
        })
        .unwrap();
    let err = handle.join().unwrap().unwrap_err();
    match err {
        IoError::LimitExceeded { limit: LimitKind::Depth, value, at } => {
            assert_eq!(value, u64::from(TOY_MAX_RECURSION));
            assert_eq!(at.byte, u64::from(TOY_MAX_RECURSION));
        }
        other => panic!("unexpected {other:?}"),
    }
}

// ------------------------------------------------------------------ AC-2

/// AC-2: a 1 MiB byte limit over zeros_2mib.bin fails at offset 1,048,576.
#[test]
fn ac2_byte_limit_on_read_to_end() {
    let e = expected();
    assert_eq!(std::fs::metadata(fixture("zeros_2mib.bin")).unwrap().len(), num(&e, "zeros_2mib.size"));
    assert_eq!(e["ac2.limit"], "bytes");
    let want = IoError::LimitExceeded {
        limit: LimitKind::Bytes,
        value: num(&e, "ac2.value"),
        at: Position::byte(num(&e, "ac2.byte")),
    };
    let request = LimitRequest::default().bytes(1 << 20);

    // Through the harness's own read-to-end.
    let file = File::open(fixture("zeros_2mib.bin")).unwrap();
    let mut src = BoundedSource::new(file, budget(Principal::Agent, request));
    let mut out = Vec::new();
    assert_eq!(src.read_to_end_bounded(&mut out).unwrap_err(), want);
    assert_eq!(out.len() as u64, num(&e, "ac2.byte"));
    assert!(out.iter().all(|&b| b == 0));
    assert_eq!(src.offset(), num(&e, "ac2.byte"));

    // Through std::io::Read::read_to_end: the IoError survives the io::Error round trip.
    let file = File::open(fixture("zeros_2mib.bin")).unwrap();
    let mut src = BoundedSource::new(file, budget(Principal::Agent, request));
    let mut out = Vec::new();
    let io_err = src.read_to_end(&mut out).unwrap_err();
    assert_eq!(IoError::from_io(io_err, src.offset()), want);
}

/// An input of exactly the limit reads to a clean end.
#[test]
fn ac2_exactly_at_limit_is_fine() {
    let data = vec![7u8; 1 << 20];
    let mut src = BoundedSource::new(&data[..], budget(Principal::Script, LimitRequest::default().bytes(1 << 20)));
    let mut out = Vec::new();
    assert_eq!(src.read_to_end_bounded(&mut out).unwrap(), 1 << 20);
    assert_eq!(out, data);
}

/// Tracked allocation is charged by read-to-end and fails closed.
#[test]
fn alloc_limit_fails_closed() {
    let data = vec![0u8; 100_000];
    let mut src = BoundedSource::new(&data[..], budget(Principal::Agent, LimitRequest::default().alloc(50_000)));
    let mut out = Vec::new();
    match src.read_to_end_bounded(&mut out).unwrap_err() {
        IoError::LimitExceeded { limit: LimitKind::Alloc, value: 50_000, at } => assert!(at.byte <= 100_000),
        other => panic!("unexpected {other:?}"),
    }
    assert!(src.budget().alloc_used() <= 50_000);
}

/// Every growth of the read-to-end buffer is charged before it happens.
#[test]
fn read_to_end_charges_capacity_growth() {
    let data = vec![1u8; 20_000];
    let mut src = BoundedSource::new(&data[..], budget(Principal::Agent, LimitRequest::default()));
    let mut out = Vec::new();
    assert_eq!(src.read_to_end_bounded(&mut out).unwrap(), 20_000);
    assert!(src.budget().alloc_used() >= 20_000);
    assert!(src.budget().alloc_used() < 2 * 20_000 + 8192);
}

/// Many tiny strings: each kept string costs its slot even when empty, so the
/// allocation limit is reached although the input is small.
#[test]
fn alloc_limit_counts_every_kept_string() {
    let mut doc = String::from("[");
    for i in 0..200 {
        if i > 0 {
            doc.push(',');
        }
        doc.push_str("\"\"");
    }
    doc.push(']');
    assert_eq!(doc.len(), 601);
    let b = budget(Principal::Agent, LimitRequest::default().alloc(2000));
    match toy::read_slice(doc.as_bytes(), b).unwrap_err() {
        IoError::LimitExceeded { limit: LimitKind::Alloc, value: 2000, .. } => {}
        other => panic!("unexpected {other:?}"),
    }
    // The same shape with 10 strings fits.
    let small = format!("[{}]", ["\"\""; 10].join(","));
    let ok = toy::read_slice(small.as_bytes(), budget(Principal::Agent, LimitRequest::default().alloc(2000))).unwrap();
    assert_eq!(ok.strings.len(), 10);
}

/// Invalid UTF-8 grows on conversion (one byte becomes the 3-byte U+FFFD); the
/// converted length is what is charged.
#[test]
fn alloc_limit_counts_converted_string_length() {
    let mut doc = vec![b'"'];
    doc.extend_from_slice(&[0xFF; 300]);
    doc.push(b'"');
    // Buffer 302 + converted 900 + one slot > 1000 (raw counting would give ~618).
    let b = budget(Principal::Agent, LimitRequest::default().alloc(1000));
    match toy::read_slice(&doc, b).unwrap_err() {
        IoError::LimitExceeded { limit: LimitKind::Alloc, value: 1000, .. } => {}
        other => panic!("unexpected {other:?}"),
    }
    let ok = toy::read_slice(&doc, budget(Principal::Agent, LimitRequest::default().alloc(1300))).unwrap();
    assert_eq!(ok.strings[0].len(), 900);
}

// ------------------------------------------------------------------ AC-3

/// AC-3: an element limit of 1,000, one element per line of lines_5000.txt, fails at
/// line 1,001.
#[test]
fn ac3_element_limit_per_line() {
    let e = expected();
    assert_eq!(std::fs::metadata(fixture("lines_5000.txt")).unwrap().len(), num(&e, "lines_5000.size"));
    assert_eq!(e["ac3.limit"], "elements");
    let file = File::open(fixture("lines_5000.txt")).unwrap();
    let src = BoundedSource::new(file, budget(Principal::Agent, LimitRequest::default().elements(1000)));
    let mut reader = BufReader::new(src);
    let mut line = 0u32;
    let mut offset = 0u64;
    let mut buf = Vec::new();
    let err = loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf).unwrap();
        if n == 0 {
            panic!("read to the end without hitting the element limit");
        }
        line += 1;
        let at = Position::line_col(offset, line, 1);
        if let Err(err) = reader.get_mut().budget_mut().charge_elements(1, at) {
            break err;
        }
        offset += n as u64;
    };
    assert_eq!(
        err,
        IoError::LimitExceeded {
            limit: LimitKind::Elements,
            value: num(&e, "ac3.value"),
            at: Position::line_col(num(&e, "ac3.byte"), num(&e, "ac3.line") as u32, 1),
        }
    );
    assert_eq!(reader.get_ref().budget().elements_used(), 1000);
}

// ------------------------------------------------------------------ AC-4

/// AC-4: an agent asking for a 128 GiB byte limit fails `limit_raise_forbidden`.
#[test]
fn ac4_agent_cannot_raise_limits() {
    let request = LimitRequest::default().bytes(128 * (1u64 << 30));
    let err = Budget::new(Principal::Agent, &request).unwrap_err();
    assert_eq!(
        err,
        IoError::LimitRaiseForbidden { limit: LimitKind::Bytes, requested: 128 * (1u64 << 30), default: DEFAULT_BYTES }
    );
    assert_eq!(err.code(), "limit_raise_forbidden");
    assert!(Budget::new(Principal::Script, &request).is_err());
    let human = Budget::new(Principal::Human, &request).unwrap();
    assert_eq!(human.limits().bytes(), 128 * (1u64 << 30));
}

#[test]
fn ac4_every_limit_follows_the_raise_rule() {
    let raised = [
        (LimitKind::Bytes, LimitRequest::default().bytes(DEFAULT_BYTES + 1)),
        (LimitKind::Elements, LimitRequest::default().elements(DEFAULT_ELEMENTS + 1)),
        (LimitKind::Depth, LimitRequest::default().depth(DEFAULT_DEPTH + 1)),
        (LimitKind::Alloc, LimitRequest::default().alloc(DEFAULT_ALLOC + 1)),
        (LimitKind::Wall, LimitRequest::default().wall_ms(DEFAULT_WALL_MS + 1)),
    ];
    for (kind, req) in raised {
        for p in [Principal::Agent, Principal::Script] {
            match Budget::new(p, &req).unwrap_err() {
                IoError::LimitRaiseForbidden { limit, .. } => assert_eq!(limit, kind),
                other => panic!("{}: unexpected {other:?}", p.as_str()),
            }
        }
        assert_eq!(Budget::new(Principal::Human, &req).unwrap().limits().value(kind), Limits::DEFAULT.value(kind) + 1);
    }
    // Lowering and keeping the default are allowed for everyone.
    let lower = LimitRequest::default().bytes(10).elements(10).depth(3).alloc(10).wall_ms(10);
    let same = LimitRequest::default().bytes(DEFAULT_BYTES).depth(DEFAULT_DEPTH);
    for p in [Principal::Human, Principal::Agent, Principal::Script] {
        let l = *Budget::new(p, &lower).unwrap().limits();
        assert_eq!((l.bytes(), l.elements(), l.depth(), l.alloc(), l.wall_ms()), (10, 10, 3, 10, 10));
        assert_eq!(*Budget::new(p, &same).unwrap().limits(), Limits::DEFAULT);
    }
}

// ------------------------------------------------------------------ AC-5 (stable-Rust proxy)

struct XorShift(u64);
impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

/// AC-5 proxy on stable Rust: the fuzz target's entry point over seeded random and
/// mutated inputs never panics and always returns a code from the closed set. The real
/// campaign is `cargo fuzz run limits_toy_reader -- -max_total_time=300`.
#[test]
fn ac5_toy_reader_survives_hostile_input() {
    let alphabet = b"[],0123456789\" \n\t\\\x00\x7fab\xff";
    let seeds: Vec<Vec<u8>> = vec![
        std::fs::read(fixture("deep_nest.json")).unwrap(),
        b"[1,[2,\"x\"],[[[]]],\"label\"]".to_vec(),
        b"[".repeat(300),
    ];
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    for case in 0..20_000u32 {
        let data: Vec<u8> = if case % 2 == 0 {
            let len = (rng.next() % 512) as usize;
            (0..len).map(|_| alphabet[(rng.next() % alphabet.len() as u64) as usize]).collect()
        } else {
            let mut d = seeds[(case as usize / 2) % seeds.len()].clone();
            for _ in 0..(rng.next() % 8) {
                if d.is_empty() {
                    break;
                }
                let i = (rng.next() % d.len() as u64) as usize;
                d[i] = alphabet[(rng.next() % alphabet.len() as u64) as usize];
            }
            d
        };
        match toy::read_slice(&data, toy::fuzz_budget().unwrap()) {
            Ok(doc) => assert!(doc.max_depth <= DEFAULT_DEPTH),
            Err(e) => assert!(CODES.contains(&e.code()), "{e:?}"),
        }
    }
}

#[test]
fn toy_reader_reports_positions_and_untrusted_strings() {
    let ok = toy::read_slice("[1, \"a\u{202e}b\", [\"\"]]".as_bytes(), toy::fuzz_budget().unwrap()).unwrap();
    assert_eq!(ok.elements, 5);
    assert_eq!(ok.max_depth, 2);
    assert_eq!(ok.strings, [UntrustedText::new("a\u{202e}b"), UntrustedText::new("")]);
    assert_eq!(ok.strings[0].as_display().to_string(), "a\\u{202e}b");
    let ctl = toy::read_slice(b"\"a\x1bb\"", toy::fuzz_budget().unwrap()).unwrap_err();
    assert_eq!(ctl, IoError::Malformed { at: Position::line_col(2, 1, 3), reason: "control_byte_in_string" });

    let bad = toy::read_slice(b"[1,\n x]", toy::fuzz_budget().unwrap()).unwrap_err();
    assert_eq!(bad, IoError::Malformed { at: Position::line_col(5, 2, 2), reason: "unexpected_byte" });
    let cut = toy::read_slice(b"[1,", toy::fuzz_budget().unwrap()).unwrap_err();
    assert_eq!(cut, IoError::TruncatedFile { at: Position::line_col(3, 1, 4) });
    let trailing = toy::read_slice(b"1 2", toy::fuzz_budget().unwrap()).unwrap_err();
    assert_eq!(trailing, IoError::Malformed { at: Position::line_col(2, 1, 3), reason: "trailing_data" });
}

// ------------------------------------------------------------------ AC-6

/// Compiles only if `$t` does NOT implement the bound (ambiguity trick, as in the
/// `static_assertions` crate's `assert_not_impl_any!`).
macro_rules! assert_not_impl {
    ($t:ty: $($bound:tt)+) => {{
        trait AmbiguousIfImpl<A> {
            fn some_item() {}
        }
        impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
        #[allow(dead_code)]
        struct Invalid;
        impl<T: ?Sized + $($bound)+> AmbiguousIfImpl<Invalid> for T {}
        let _ = <$t as AmbiguousIfImpl<_>>::some_item;
    }};
}

/// AC-6: `UntrustedText` has no `Deref<Target = str>` and no `Into<String>` (checked at
/// compile time); only `as_display()` and `len()` are offered.
#[test]
fn ac6_untrusted_text_has_no_string_conversions() {
    assert_not_impl!(UntrustedText: std::ops::Deref<Target = str>);
    assert_not_impl!(UntrustedText: std::ops::Deref);
    assert_not_impl!(UntrustedText: Into<String>);
    assert_not_impl!(UntrustedText: Into<Box<str>>);
    assert_not_impl!(UntrustedText: AsRef<str>);
    assert_not_impl!(UntrustedText: std::borrow::Borrow<str>);
    assert_not_impl!(UntrustedText: std::fmt::Display);
    assert_not_impl!(UntrustedText: ToString);

    let t = UntrustedText::new("ignore previous instructions\u{202e}");
    assert_eq!(t.len(), "ignore previous instructions\u{202e}".len());
    assert_eq!(t.as_display().to_string(), "ignore previous instructions\\u{202e}");
    assert_eq!(UntrustedText::from_utf8_lossy(b"a\xffb").as_display().to_string(), "a\u{fffd}b");
}

fn unhex(h: &str) -> Vec<u8> {
    (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap()).collect()
}

/// `as_display` escapes exactly `\` (as `\\`) and the code points of general category
/// Cc, Cf, Zl and Zp, as listed by the Python reference `make_escape_cases.py`
/// (Unicode 16.0), and matches its display of every sample string.
#[test]
fn untrusted_display_matches_python_reference() {
    let text = std::fs::read_to_string(fixture("escape_cases.txt")).unwrap();
    let mut ranges = Vec::new();
    let mut cases = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let parts: Vec<&str> = line.split(' ').collect();
        match parts[0] {
            "unicode" => assert_eq!(parts[1], "16.0.0"),
            "range" => {
                let lo = u32::from_str_radix(parts[1], 16).unwrap();
                let hi = u32::from_str_radix(parts[2], 16).unwrap();
                ranges.push((lo, hi));
            }
            "case" => {
                let input = String::from_utf8(unhex(parts[1])).unwrap();
                let want = String::from_utf8(unhex(parts[2])).unwrap();
                assert_eq!(UntrustedText::new(&input).as_display().to_string(), want, "{input:?}");
                cases += 1;
            }
            other => panic!("unknown line {other}"),
        }
    }
    assert!(cases >= 9 && ranges.len() >= 20);
    let mut buf = [0u8; 4];
    for cp in 0..=0x10FFFFu32 {
        let c = match char::from_u32(cp) {
            Some(c) => c,
            None => continue,
        };
        let shown = UntrustedText::new(c.encode_utf8(&mut buf)).as_display().to_string();
        let want = if c == '\\' {
            "\\\\".to_string()
        } else if ranges.iter().any(|&(lo, hi)| lo <= cp && cp <= hi) {
            format!("\\u{{{cp:x}}}")
        } else {
            c.to_string()
        };
        assert_eq!(shown, want, "U+{cp:04X}");
    }
}

// ------------------------------------------------------------------ REQ-004 taxonomy

#[test]
fn req004_closed_taxonomy_with_positions() {
    let at = Position::line_col(9, 2, 3);
    let all = [
        IoError::LimitExceeded { limit: LimitKind::Bytes, value: 1, at },
        IoError::Malformed { at, reason: "x" },
        IoError::TruncatedFile { at },
        IoError::UnitRequired,
        IoError::UnitConflict,
        IoError::FormatUnknown,
        IoError::FormatAmbiguous,
        IoError::FormatMismatch,
        IoError::FormatDuplicate,
        IoError::RawPathForbidden,
        IoError::PathOutsideRoot,
        IoError::RootNotFound,
        IoError::TargetExists,
        IoError::LimitRaiseForbidden { limit: LimitKind::Depth, requested: 2, default: 1 },
        IoError::ReaderCrashed,
        IoError::Io { kind: std::io::ErrorKind::NotFound, at },
    ];
    let codes: Vec<_> = all.iter().map(|e| e.code()).collect();
    assert_eq!(codes, CODES);
    for c in CODES {
        assert!(c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'), "{c}");
        assert_eq!(CODES.iter().filter(|x| *x == c).count(), 1);
    }
    for e in &all {
        if e.is_malformed_input() {
            assert_eq!(e.position(), Some(at));
        }
        assert!(e.to_string().starts_with(e.code()));
        let back = IoError::from_io(e.clone().into_io(), 0);
        assert_eq!(&back, e);
    }
    assert_eq!(
        IoError::from_io(std::io::Error::from(std::io::ErrorKind::UnexpectedEof), 4),
        IoError::TruncatedFile { at: Position::byte(4) }
    );
}

// ------------------------------------------------------------------ wall time

struct StepClock {
    now: u64,
    step: u64,
    reads: Arc<AtomicU64>,
}
impl Clock for StepClock {
    fn now_ms(&mut self) -> u64 {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let t = self.now;
        self.now += self.step;
        t
    }
}

/// Wall time is read once every 4,096 charges; expiry is `limit_exceeded{wall}` with
/// determinism `N`.
#[test]
fn wall_time_checked_every_4096_charges() {
    let reads = Arc::new(AtomicU64::new(0));
    let clock = StepClock { now: 0, step: 1000, reads: Arc::clone(&reads) };
    let mut b = Budget::with_clock(Principal::Agent, &LimitRequest::default().wall_ms(2500), Box::new(clock)).unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 1); // start time
    let mut failed = None;
    for i in 0..10 * WALL_CHECK_EVERY {
        if let Err(e) = b.charge_elements(1, Position::byte(i)) {
            failed = Some((i, e));
            break;
        }
    }
    let (i, e) = failed.unwrap();
    // Reads at charges 4096 (t=1000), 8192 (t=2000), 12288 (t=3000 > 2500).
    assert_eq!(i + 1, 3 * WALL_CHECK_EVERY);
    assert_eq!(reads.load(Ordering::SeqCst), 4);
    assert_eq!(e, IoError::LimitExceeded { limit: LimitKind::Wall, value: 2500, at: Position::byte(i) });
    assert_eq!(e.determinism(), Determinism::N);
}

#[test]
fn depth_guard_restores_depth() {
    let mut b = budget(Principal::Agent, LimitRequest::default().depth(2));
    {
        let mut g1 = b.enter(Position::byte(0)).unwrap();
        assert_eq!(g1.depth(), 1);
        {
            let mut g2 = g1.enter(Position::byte(1)).unwrap();
            assert_eq!(g2.depth(), 2);
            assert_eq!(
                g2.enter(Position::byte(2)).unwrap_err(),
                IoError::LimitExceeded { limit: LimitKind::Depth, value: 2, at: Position::byte(2) }
            );
            g2.leave();
        }
        assert_eq!(g1.depth(), 1);
    }
    assert_eq!(b.depth(), 0);
    assert_eq!(b.max_depth(), 2);
}

#[test]
fn bounded_source_reports_io_errors_with_offset() {
    struct Failing(u8);
    impl Read for Failing {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.0 == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            self.0 -= 1;
            buf[0] = 1;
            Ok(1)
        }
    }
    let mut src = BoundedSource::new(Failing(3), budget(Principal::Agent, LimitRequest::default()));
    let mut out = Vec::new();
    assert_eq!(
        src.read_to_end_bounded(&mut out).unwrap_err(),
        IoError::Io { kind: std::io::ErrorKind::PermissionDenied, at: Position::byte(3) }
    );
}
