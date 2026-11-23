//! Acceptance tests for `GEO.SEL.QUERY.GRAMMAR` (`spec/geo/sel/query/grammar/NODE.md`):
//! AC-1 (60 valid vectors), AC-2 (40 equivalent + 20 non-equivalent pairs), AC-3 (80 invalid
//! inputs with exact JSON pointers), AC-4 (10 000-deep nested `not`, 65 KiB input, > 1 024 AST
//! nodes, > 10 000 IDs), AC-5 (seed-37 mutation fuzz without panic).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_sel::query::{
    parse_and_canonicalize, parse_opaque_query, parse_query, to_canonical_bytes, to_opaque_query, QueryError,
    MAX_AST_NODES, MAX_IDS_PER_LIST, MAX_QUERY_BYTES,
};

const VALID_JSON: &str = include_str!("../../../fixtures/geo/sel/query/valid_v1.json");
const PAIRS_JSON: &str = include_str!("../../../fixtures/geo/sel/query/equivalent_pairs.json");
const INVALID_JSON: &str = include_str!("../../../fixtures/geo/sel/query/invalid_v1.json");

// ------------------------------------------------------------------ minimal fixture JSON reader
// (fid-geo-sel is a KNP-1 kernel crate with zero third-party or model-layer dev-dependencies.)

#[derive(Debug, Clone)]
enum FVal {
    Raw(String), // raw JSON slice for forwarding verbatim to parse_query
    Str(String),
    Arr(Vec<FVal>),
    Obj(Vec<(String, FVal)>),
}

impl FVal {
    fn field(&self, key: &str) -> Option<&FVal> {
        let FVal::Obj(entries) = self else { return None };
        entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn as_str(&self) -> &str {
        match self {
            FVal::Str(s) => s,
            other => panic!("expected string, got {other:?}"),
        }
    }

    fn as_arr(&self) -> &[FVal] {
        match self {
            FVal::Arr(a) => a,
            other => panic!("expected array, got {other:?}"),
        }
    }

    fn as_raw(&self) -> &str {
        match self {
            FVal::Raw(r) => r,
            other => panic!("expected raw slice, got {other:?}"),
        }
    }
}

struct Reader<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn ws(&mut self) {
        while let Some(&b) = self.src.get(self.pos) {
            if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn bump(&mut self) -> u8 {
        let b = self.src[self.pos];
        self.pos += 1;
        b
    }

    fn read_str(&mut self) -> String {
        assert_eq!(self.bump(), b'"');
        let mut out = String::new();
        loop {
            let b = self.bump();
            match b {
                b'"' => return out,
                b'\\' => match self.bump() {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let hex = std::str::from_utf8(&self.src[self.pos..self.pos + 4]).unwrap();
                        self.pos += 4;
                        let cp = u32::from_str_radix(hex, 16).unwrap();
                        out.push(char::from_u32(cp).unwrap());
                    }
                    other => panic!("bad escape {}", other as char),
                },
                c if c < 0x80 => out.push(c as char),
                _ => {
                    let start = self.pos - 1;
                    while self.pos < self.src.len() && (self.src[self.pos] & 0xC0) == 0x80 {
                        self.pos += 1;
                    }
                    out.push_str(std::str::from_utf8(&self.src[start..self.pos]).unwrap());
                }
            }
        }
    }

    fn skip_any(&mut self) -> String {
        self.ws();
        let start = self.pos;
        match self.peek().unwrap() {
            b'"' => {
                let _ = self.read_str();
            }
            b'{' => {
                self.pos += 1;
                self.ws();
                if self.peek() == Some(b'}') {
                    self.pos += 1;
                } else {
                    loop {
                        self.ws();
                        let _ = self.read_str();
                        self.ws();
                        assert_eq!(self.bump(), b':');
                        let _ = self.skip_any();
                        self.ws();
                        match self.bump() {
                            b',' => continue,
                            b'}' => break,
                            other => panic!("unexpected {}", other as char),
                        }
                    }
                }
            }
            b'[' => {
                self.pos += 1;
                self.ws();
                if self.peek() == Some(b']') {
                    self.pos += 1;
                } else {
                    loop {
                        let _ = self.skip_any();
                        self.ws();
                        match self.bump() {
                            b',' => continue,
                            b']' => break,
                            other => panic!("unexpected {}", other as char),
                        }
                    }
                }
            }
            _ => {
                while let Some(b) = self.peek() {
                    if matches!(b, b',' | b']' | b'}' | b' ' | b'\t' | b'\r' | b'\n') {
                        break;
                    }
                    self.pos += 1;
                }
            }
        }
        std::str::from_utf8(&self.src[start..self.pos]).unwrap().to_string()
    }

    fn read_fixture_val(&mut self, raw_keys: &[&str]) -> FVal {
        self.ws();
        match self.peek().unwrap() {
            b'"' => FVal::Str(self.read_str()),
            b'[' => {
                self.pos += 1;
                let mut items = Vec::new();
                self.ws();
                if self.peek() == Some(b']') {
                    self.pos += 1;
                    return FVal::Arr(items);
                }
                loop {
                    items.push(self.read_fixture_val(raw_keys));
                    self.ws();
                    match self.bump() {
                        b',' => continue,
                        b']' => return FVal::Arr(items),
                        other => panic!("unexpected {}", other as char),
                    }
                }
            }
            b'{' => {
                self.pos += 1;
                let mut entries = Vec::new();
                self.ws();
                if self.peek() == Some(b'}') {
                    self.pos += 1;
                    return FVal::Obj(entries);
                }
                loop {
                    self.ws();
                    let k = self.read_str();
                    self.ws();
                    assert_eq!(self.bump(), b':');
                    let v = if raw_keys.contains(&k.as_str()) {
                        FVal::Raw(self.skip_any())
                    } else {
                        self.read_fixture_val(raw_keys)
                    };
                    entries.push((k, v));
                    self.ws();
                    match self.bump() {
                        b',' => continue,
                        b'}' => return FVal::Obj(entries),
                        other => panic!("unexpected {}", other as char),
                    }
                }
            }
            _ => FVal::Raw(self.skip_any()),
        }
    }
}

fn parse_fixture(json: &str, raw_keys: &[&str]) -> FVal {
    let mut r = Reader { src: json.as_bytes(), pos: 0 };
    r.read_fixture_val(raw_keys)
}

// ------------------------------------------------------------------ AC-1

/// AC-1: 60 valid queries covering every predicate and combinator match committed canonical
/// byte strings and SHA-256 digests, and round-trip idempotently through opaque SelectionQuery.
#[test]
fn ac1_valid_v1_vectors_match_committed_canonical_bytes_and_digests() {
    let doc = parse_fixture(VALID_JSON, &["query"]);
    let cases = doc.field("cases").unwrap().as_arr();
    assert_eq!(cases.len(), 60);

    for case in cases {
        let name = case.field("name").unwrap().as_str();
        let query_raw = case.field("query").unwrap().as_raw();
        let expected_canon = case.field("canonical").unwrap().as_str();
        let expected_digest = case.field("digest").unwrap().as_str();

        let (canon_ast, bytes, digest) =
            parse_and_canonicalize(query_raw.as_bytes()).unwrap_or_else(|e| panic!("case {name} failed to parse: {e}"));
        let got_canon = std::str::from_utf8(&bytes).unwrap();
        assert_eq!(got_canon, expected_canon, "canonical mismatch on case {name}");
        assert_eq!(digest.as_str(), expected_digest, "digest mismatch on case {name}");

        // Canonical form round-trips through parse_query and opaque SelectionQuery idempotently.
        let opaque = to_opaque_query(&canon_ast).unwrap();
        let reparsed = parse_opaque_query(&opaque).unwrap();
        let re_bytes = to_canonical_bytes(&reparsed).unwrap();
        assert_eq!(re_bytes, bytes, "round-trip mismatch on case {name}");
    }
}

// ------------------------------------------------------------------ AC-2

/// AC-2: 40 equivalent pairs produce equal digests; 20 semantically distinct pairs produce
/// different digests (including REQ-005 authored-text changes with identical SI value).
#[test]
fn ac2_equivalent_and_non_equivalent_pairs() {
    let doc = parse_fixture(PAIRS_JSON, &["a", "b"]);
    let eq_pairs = doc.field("equivalent").unwrap().as_arr();
    let neq_pairs = doc.field("non_equivalent").unwrap().as_arr();
    assert_eq!(eq_pairs.len(), 40);
    assert_eq!(neq_pairs.len(), 20);

    for pair in eq_pairs {
        let name = pair.field("name").unwrap().as_str();
        let a_src = if let Some(raw) = pair.field("a_raw") { raw.as_str() } else { pair.field("a").unwrap().as_raw() };
        let b_src = if let Some(raw) = pair.field("b_raw") { raw.as_str() } else { pair.field("b").unwrap().as_raw() };

        let (_, ab, ad) = parse_and_canonicalize(a_src.as_bytes()).unwrap_or_else(|e| panic!("eq {name} a: {e}"));
        let (_, bb, bd) = parse_and_canonicalize(b_src.as_bytes()).unwrap_or_else(|e| panic!("eq {name} b: {e}"));
        assert_eq!(ab, bb, "equivalent pair {name} canonical bytes differ");
        assert_eq!(ad, bd, "equivalent pair {name} digests differ");
    }

    for pair in neq_pairs {
        let name = pair.field("name").unwrap().as_str();
        let a_src = pair.field("a").unwrap().as_raw();
        let b_src = pair.field("b").unwrap().as_raw();

        let (_, ab, ad) = parse_and_canonicalize(a_src.as_bytes()).unwrap_or_else(|e| panic!("neq {name} a: {e}"));
        let (_, bb, bd) = parse_and_canonicalize(b_src.as_bytes()).unwrap_or_else(|e| panic!("neq {name} b: {e}"));
        assert_ne!(ab, bb, "non-equivalent pair {name} canonical bytes unexpectedly equal");
        assert_ne!(ad, bd, "non-equivalent pair {name} digests unexpectedly equal");
    }
}

// ------------------------------------------------------------------ AC-3

/// AC-3 (error path): 80 malformed queries fail with the expected error code and JSON pointer.
#[test]
fn ac3_invalid_v1_rejections_and_json_pointers() {
    let doc = parse_fixture(INVALID_JSON, &["query"]);
    let cases = doc.field("cases").unwrap().as_arr();
    assert_eq!(cases.len(), 80);

    for case in cases {
        let name = case.field("name").unwrap().as_str();
        let expected_code = case.field("expected_code").unwrap().as_str();
        let expected_ptr = case.field("expected_pointer").unwrap().as_str();
        let src = if let Some(raw) = case.field("raw") { raw.as_str() } else { case.field("query").unwrap().as_raw() };

        let err = parse_query(src.as_bytes()).expect_err(&format!("invalid case {name} unexpectedly parsed"));
        assert_eq!(err.code(), expected_code, "wrong error code on {name}: got {err}");
        assert_eq!(err.pointer(), Some(expected_ptr), "wrong JSON pointer on {name}: got {err}");
    }
}

// ------------------------------------------------------------------ AC-4

/// AC-4 (error path): 10 000-deep nested `not`, 65 KiB input, > 1 024 AST nodes, and > 10 000
/// IDs all fail closed with `query_limit` without stack overflow.
#[test]
fn ac4_limits_without_stack_overflow() {
    // 1. 10 000-deep nested `not`: must fail with `query_limit`, never overflow the stack.
    let depth = 10_000usize;
    let mut deep = String::with_capacity(depth * 9 + 64);
    deep.push_str(r#"{"v":1,"domain":"points","where":"#);
    for _ in 0..depth {
        deep.push_str(r#"{"not":"#);
    }
    deep.push_str(r#"{"all":{}}"#);
    for _ in 0..depth {
        deep.push('}');
    }
    deep.push('}');
    // Even if > 64 KiB, also test a depth-40 nested `not` (< 64 KiB) to exercise depth limit directly.
    let err_deep = parse_query(deep.as_bytes()).expect_err("10 000-deep must fail");
    assert!(matches!(err_deep, QueryError::QueryLimit { .. }), "got {err_deep}");

    let depth40 = 40usize;
    let mut deep40 = String::new();
    deep40.push_str(r#"{"v":1,"domain":"points","where":"#);
    for _ in 0..depth40 {
        deep40.push_str(r#"{"not":"#);
    }
    deep40.push_str(r#"{"all":{}}"#);
    for _ in 0..depth40 {
        deep40.push('}');
    }
    deep40.push('}');
    assert!(deep40.len() < MAX_QUERY_BYTES);
    let err_40 = parse_query(deep40.as_bytes()).expect_err("depth 40 must fail");
    assert_eq!(err_40.code(), "query_limit");

    // 2. 65 KiB input -> query_limit.
    let mut big = String::from(r#"{"v":1,"domain":"points","where":{"all":{}}}"#);
    big.push_str(&" ".repeat(65 * 1024));
    assert!(big.len() > MAX_QUERY_BYTES);
    let err_big = parse_query(big.as_bytes()).expect_err("65 KiB input must fail");
    assert_eq!(err_big.code(), "query_limit");

    // 3. > 1 024 AST nodes (under 64 KiB, depth <= 32) -> query_limit.
    let mut many_nodes = String::from(r#"{"v":1,"domain":"points","where":{"and":["#);
    for i in 0..=MAX_AST_NODES {
        if i > 0 {
            many_nodes.push(',');
        }
        many_nodes.push_str(r#"{"all":{}}"#);
    }
    many_nodes.push_str("]}}");
    assert!(many_nodes.len() <= MAX_QUERY_BYTES);
    let err_nodes = parse_query(many_nodes.as_bytes()).expect_err("1 025 AST nodes must fail");
    assert_eq!(err_nodes.code(), "query_limit");

    // 4. > 10 000 IDs in one `ids` list (under 64 KiB) -> query_limit.
    let mut many_ids = String::from(r#"{"v":1,"domain":"points","where":{"ids":{"ids":["#);
    for i in 0..=MAX_IDS_PER_LIST {
        if i > 0 {
            many_ids.push(',');
        }
        many_ids.push('0');
    }
    many_ids.push_str("]}}}");
    assert!(many_ids.len() <= MAX_QUERY_BYTES);
    let err_ids = parse_query(many_ids.as_bytes()).expect_err("10 001 IDs must fail");
    assert_eq!(err_ids.code(), "query_limit");
}

// ------------------------------------------------------------------ AC-5

/// Deterministic splitmix64 PRNG for AC-5 fuzzing (seed 37).
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn idx(&mut self, len: usize) -> usize {
        (self.next_u64() as usize) % len
    }
}

/// AC-5: 10^6 mutated inputs (seed 37) never panic and every result is a typed query or typed
/// error; any accepted query canonicalises cleanly without error.
#[test]
fn ac5_fuzz_one_million_mutated_inputs_seed_37_never_panics() {
    let seeds: [&[u8]; 8] = [
        br#"{"v":1,"domain":"points","where":{"all":{}}}"#,
        br#"{"v":1,"domain":"triangles","where":{"ids":{"ids":[1,2,3]}}}"#,
        br#"{"v":1,"domain":"faces","where":{"and":[{"in_body":{"body":"00000000-0000-7000-8000-000000000001"}},{"not":{"all":{}}}]}}"#,
        br#"{"v":1,"domain":"points","where":{"in_sphere":{"centre":[{"text":"0","unit":"mm"},{"text":"0","unit":"mm"},{"text":"0","unit":"mm"}],"radius":{"text":"5","unit":"mm"}}}}"#,
        br#"{"v":1,"domain":"points","where":{"attr_range":{"channel":"intensity","min":0,"max":1}}}"#,
        br#"{"v":1,"domain":"edges","where":{"near":{"of":{"face":{"body":"00000000-0000-7000-8000-000000000001","id":"F1"}},"distance":{"text":"1","unit":"mm"}}}}"#,
        br#"{"v":1,"domain":"faces","where":{"tangent_grow":{"seed":{"all":{}},"angle":{"text":"5","unit":"deg"}}}}"#,
        br#"{"v":1,"domain":"points","where":{"normal_cone":{"axis":[0,0,1],"half_angle":{"text":"10","unit":"deg"}}}}"#,
    ];

    let mut rng = Rng(37);
    let mut buf = Vec::with_capacity(256);
    let mut ok_count = 0usize;
    let mut err_count = 0usize;

    for _ in 0..1_000_000usize {
        buf.clear();
        let base = seeds[rng.idx(seeds.len())];
        buf.extend_from_slice(base);
        let ops = 1 + (rng.next_u64() as usize % 4);
        for _ in 0..ops {
            match rng.next_u64() % 5 {
                0 if !buf.is_empty() => {
                    // Byte flip
                    let p = rng.idx(buf.len());
                    buf[p] = (rng.next_u64() & 0xff) as u8;
                }
                1 if !buf.is_empty() => {
                    // Delete byte
                    let p = rng.idx(buf.len());
                    buf.remove(p);
                }
                2 if buf.len() < 240 => {
                    // Insert syntax byte
                    const SYM: &[u8] = b"{}[]\":,019-e.\\u/n\\";
                    let p = rng.idx(buf.len() + 1);
                    let c = SYM[rng.idx(SYM.len())];
                    buf.insert(p, c);
                }
                3 if buf.len() > 2 => {
                    // Truncate
                    let new_len = rng.idx(buf.len());
                    buf.truncate(new_len);
                }
                _ if buf.len() >= 2 => {
                    // Swap two bytes
                    let a = rng.idx(buf.len());
                    let b = rng.idx(buf.len());
                    buf.swap(a, b);
                }
                _ => {}
            }
        }
        match parse_and_canonicalize(&buf) {
            Ok((_q, _bytes, _d)) => ok_count += 1,
            Err(_e) => err_count += 1,
        }
    }
    assert!(ok_count > 0 && err_count > 0, "expected both ok ({ok_count}) and err ({err_count})");
}
