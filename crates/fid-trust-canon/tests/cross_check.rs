//! Cross-check against CORE's canonicaliser `fid-canon` (a dev-dependency only):
//! REQ-TRUST.CHAIN.CANON-005, AC-2 (partially: the golden journals do not exist
//! yet, so the entry-header vectors stand in) and AC-5 (an in-test, seeded
//! differential fuzz instead of a 10-minute CI fuzz job).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_trust_canon::{canonicalize, entry_hash, parse, serialize, Value};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/canon")
}

/// Known finding against `fid-canon` (reported in PR #78): for a double exactly
/// halfway between two shortest digit strings, `fid-canon` picks the upper one,
/// while ECMA-262's Number::toString guideline (Note 2), V8 and the Python
/// reference pick the even one (`277575414244990.125` gives `...990.12`, not
/// `...990.13`). Vectors named here are compared number by number with only that
/// difference allowed. Once `fid-canon` is fixed they match byte for byte again.
const KNOWN_TIE_VECTORS: [&str; 1] = ["numbers_exact_ties"];

fn collect_numbers(v: &Value, out: &mut Vec<f64>) {
    match v {
        Value::Number(x) => out.push(*x),
        Value::Array(items) => items.iter().for_each(|i| collect_numbers(i, out)),
        Value::Object(members) => members.iter().for_each(|(_, i)| collect_numbers(i, out)),
        _ => {}
    }
}

/// True when `ours` and `core` both denote `x`, differ only in the last mantissa
/// digit by one, and `ours` holds the even digit (the tie-to-even pick).
fn is_tie_pick(ours: &str, core: &str, x: f64) -> bool {
    let round_trip = matches!((ours.parse::<f64>(), core.parse::<f64>()), (Ok(p), Ok(q)) if p == x && q == x);
    let (om, oe) = ours.split_once('e').unwrap_or((ours, ""));
    let (cm, ce) = core.split_once('e').unwrap_or((core, ""));
    let (ob, cb) = (om.as_bytes(), cm.as_bytes());
    if !round_trip || oe != ce || ob.len() != cb.len() || ob.is_empty() {
        return false;
    }
    let n = ob.len() - 1;
    ob[..n] == cb[..n] && ob[n].is_ascii_digit() && cb[n] == ob[n] + 1 && (ob[n] - b'0') % 2 == 0
}

/// Compares every number of `input` formatted by both crates. Panics on any
/// difference other than the known tie pick; returns how many ties differed.
fn tie_differences(input: &[u8]) -> usize {
    let mut xs = Vec::new();
    collect_numbers(&parse(input).unwrap(), &mut xs);
    let mut ties = 0;
    for x in xs {
        let ours = fid_trust_canon::format_number(x).unwrap();
        let core = fid_canon::format_number(x).unwrap();
        if ours != core {
            assert!(is_tie_pick(&ours, &core, x), "{x:e}: ours {ours}, fid-canon {core}");
            ties += 1;
        }
    }
    ties
}

/// Every golden vector gives identical bytes from both implementations and the
/// committed reference (except the known tie finding above).
#[test]
fn golden_vectors_identical_in_fid_canon() {
    let mut dir: Vec<PathBuf> =
        std::fs::read_dir(fixtures().join("vectors")).unwrap().map(|e| e.unwrap().path()).collect();
    dir.sort();
    let mut n = 0;
    for input in dir.iter().filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json")) {
        let text = std::fs::read(input).unwrap();
        let want = std::fs::read(input.with_extension("canon")).unwrap();
        let ours = canonicalize(&text).unwrap();
        assert_eq!(ours, want, "{}", input.display());
        let core = fid_canon::canonicalize(&text).unwrap_or_else(|e| panic!("fid-canon {}: {e}", input.display()));
        let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if core != want && KNOWN_TIE_VECTORS.contains(&stem) {
            assert!(tie_differences(&text) > 0, "{stem}");
            assert_eq!(parse(&core).unwrap(), parse(&want).unwrap(), "{stem}: values differ");
        } else {
            assert_eq!(String::from_utf8_lossy(&core), String::from_utf8_lossy(&want), "fid-canon {}", input.display());
        }
        n += 1;
    }
    assert!(n >= 30);
}

/// Invalid inputs: CORE's canonicaliser must not accept any of them either.
#[test]
fn bad_inputs_rejected_by_fid_canon() {
    for e in std::fs::read_dir(fixtures().join("bad")).unwrap() {
        let p = e.unwrap().path();
        let text = std::fs::read(&p).unwrap();
        assert!(fid_canon::canonicalize(&text).is_err(), "fid-canon accepted {}", p.display());
    }
}

/// Entry hashes: ours versus fid-canon's canonical form and SHA-256 of the same
/// header without its `hash` member.
#[test]
fn entry_hashes_identical_in_fid_canon() {
    let text = std::fs::read(fixtures().join("entry/vectors.json")).unwrap();
    let Value::Array(vectors) = parse(&text).unwrap() else { panic!("not an array") };
    let mut n = 0;
    for v in &vectors {
        let Value::Object(m) = v else { panic!("vector") };
        let Some((_, Value::String(header))) = m.iter().find(|(k, _)| k == "header") else { panic!("header") };
        let Ok(ours) = entry_hash(header.as_bytes()) else { continue };
        let Ok(Value::Object(members)) = parse(header.as_bytes()) else { panic!("header object") };
        let stripped = Value::Object(members.into_iter().filter(|(k, _)| k != "hash").collect());
        let core_bytes = fid_canon::canonicalize(&serialize(&stripped).unwrap()).unwrap();
        let core = fid_canon::digest_bytes(&core_bytes);
        assert_eq!(ours.to_bytes(), core.0, "{header}");
        n += 1;
    }
    assert!(n >= 5);
}

/// xorshift64*: a fixed, platform-independent sequence.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const CHARS: [char; 22] = [
    'a',
    'Z',
    '0',
    ' ',
    '"',
    '\\',
    '/',
    '\u{0}',
    '\u{1f}',
    '\n',
    '\u{7f}',
    '\u{e9}',
    '\u{20ac}',
    '\u{2028}',
    '\u{e000}',
    '\u{fb01}',
    '\u{ff61}',
    '\u{ffff}',
    '\u{10000}',
    '\u{1f600}',
    '\u{10ffff}',
    '~',
];

fn gen_string(r: &mut Rng, out: &mut String) {
    out.push('"');
    for _ in 0..r.below(6) {
        let c = CHARS[r.below(CHARS.len() as u64) as usize];
        let escape = (c as u32) < 0x20 || c == '"' || c == '\\' || r.below(3) == 0;
        if !escape {
            out.push(c);
            continue;
        }
        match c {
            '"' if r.below(2) == 0 => out.push_str("\\\""),
            '\\' if r.below(2) == 0 => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\n' => out.push_str("\\n"),
            _ => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    if r.below(2) == 0 {
                        out.push_str(&format!("\\u{u:04x}"));
                    } else {
                        out.push_str(&format!("\\u{u:04X}"));
                    }
                }
            }
        }
    }
    out.push('"');
}

fn gen_number(r: &mut Rng, out: &mut String) {
    match r.below(6) {
        0 => {
            let x = f64::from_bits(r.next());
            if x.is_finite() {
                out.push_str(&format!("{x:e}"));
            } else {
                out.push('0');
            }
        }
        1 => {
            // Plain decimal form; bounded so the text stays short.
            let x = f64::from_bits(r.next());
            if x.is_finite() && x.abs() < 1e30 && (x == 0.0 || x.abs() > 1e-30) {
                out.push_str(&format!("{x}"));
            } else {
                out.push_str("-0");
            }
        }
        2 => out.push_str(&r.next().to_string()),
        3 => out.push_str(&format!("-{}.{}e{}", r.below(1000), r.below(100_000), r.below(60) as i64 - 30)),
        4 => out.push_str(&format!("{}", r.below(1 << 54))),
        _ => out.push_str(&format!("{}E+{}", r.below(10), r.below(25))),
    }
}

fn ws(r: &mut Rng, out: &mut String) {
    for _ in 0..r.below(3) {
        out.push([' ', '\t', '\n', '\r'][r.below(4) as usize]);
    }
}

fn gen_value(r: &mut Rng, depth: usize, out: &mut String) {
    let pick = if depth >= 6 { r.below(5) } else { r.below(7) };
    match pick {
        0 => out.push_str(["null", "true", "false"][r.below(3) as usize]),
        1 | 2 => gen_number(r, out),
        3 | 4 => gen_string(r, out),
        5 => {
            out.push('[');
            for i in 0..r.below(5) {
                if i > 0 {
                    out.push(',');
                }
                ws(r, out);
                gen_value(r, depth + 1, out);
                ws(r, out);
            }
            out.push(']');
        }
        _ => {
            out.push('{');
            for i in 0..r.below(6) {
                if i > 0 {
                    out.push(',');
                }
                ws(r, out);
                gen_string(r, out);
                out.push(':');
                gen_value(r, depth + 1, out);
            }
            out.push('}');
        }
    }
}

fn compare(input: &[u8]) -> bool {
    let ours = canonicalize(input);
    let core = fid_canon::canonicalize(input);
    match (&ours, &core) {
        (Ok(a), Ok(b)) if a == b => {}
        (Ok(a), Ok(b)) => {
            // Only the known tie finding may differ, and the values must be equal.
            assert!(tie_differences(input) > 0, "input {:?}", String::from_utf8_lossy(input));
            assert_eq!(parse(a).unwrap(), parse(b).unwrap(), "input {:?}", String::from_utf8_lossy(input));
        }
        (Err(_), Err(_)) => {}
        _ => panic!(
            "disagreement on {:?}: ours {:?}, fid-canon {:?}",
            String::from_utf8_lossy(input),
            ours.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()),
            core.as_ref().map(|b| String::from_utf8_lossy(b).into_owned())
        ),
    }
    ours.is_ok()
}

/// AC-5 (in-test form): seeded random documents and byte mutations of the golden
/// vectors; both implementations must agree on acceptance and on output bytes.
#[test]
fn differential_fuzz_against_fid_canon() {
    let mut r = Rng(0x8785_0000_c0de_f00d);
    let mut accepted = 0;
    for _ in 0..4000 {
        let mut doc = String::new();
        ws(&mut r, &mut doc);
        gen_value(&mut r, 0, &mut doc);
        ws(&mut r, &mut doc);
        if compare(doc.as_bytes()) {
            accepted += 1;
        }
    }
    assert!(accepted > 1000, "generator produced too few valid documents: {accepted}");

    // Doubles straight from random bit patterns, many per document.
    for _ in 0..200 {
        let mut doc = String::from("[");
        for i in 0..50 {
            if i > 0 {
                doc.push(',');
            }
            let x = f64::from_bits(r.next());
            doc.push_str(&if x.is_finite() { format!("{x:e}") } else { "1".to_string() });
        }
        doc.push(']');
        assert!(compare(doc.as_bytes()));
    }

    // Single-byte mutations of the golden inputs.
    let mut seeds: Vec<Vec<u8>> = Vec::new();
    for e in std::fs::read_dir(fixtures().join("vectors")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|e| e.to_str()) == Some("json") {
            seeds.push(std::fs::read(p).unwrap());
        }
    }
    seeds.sort();
    for _ in 0..3000 {
        let mut doc = seeds[r.below(seeds.len() as u64) as usize].clone();
        if doc.is_empty() {
            continue;
        }
        let at = r.below(doc.len() as u64) as usize;
        match r.below(3) {
            0 => doc[at] = r.next() as u8,
            1 => {
                doc.remove(at);
            }
            _ => doc.insert(at, b"{}[]\",:0-e.\\u"[r.below(14) as usize]),
        }
        compare(&doc);
    }
}
