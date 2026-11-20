//! Acceptance criteria of spec node KRN.MAN.KCI (AC-1 .. AC-5) and REQ-KRN.MAN.CARD-005.
//! Expected values come from `fixtures/man/make_kci_fixtures.py` (Python hashlib, json,
//! tomllib) and from the independent `sha2` crate via fid-testkit, never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kci::kci::json::{to_canonical, Json, JsonError};
use fid_kci::kci::{sha256, sha256_digest, Digest, Sha256};
use fid_kci::{
    evaluate, parse_card, AlgorithmFailure, AlgorithmId, AlgorithmTable, CardError, CardRegistry, InputArity,
    KernelAlgorithm, KernelCall, KernelError,
};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(fixtures().join(name)).unwrap()
}

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len() % 2 == 0, "odd hex {s:?}");
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').filter(|l| !l.is_empty() && !l.starts_with('#'))
}

// ---------------------------------------------------------------- AC-1: SHA-256 vectors

fn check_rsp(name: &str, expected_count: usize) {
    let text = read(name);
    let (mut len, mut msg) = (None, None);
    let mut n = 0;
    for line in text.lines().map(str::trim) {
        if let Some(v) = line.strip_prefix("Len = ") {
            len = Some(v.parse::<usize>().unwrap());
        } else if let Some(v) = line.strip_prefix("Msg = ") {
            msg = Some(unhex(v));
        } else if let Some(v) = line.strip_prefix("MD = ") {
            let bits = len.take().unwrap();
            assert_eq!(bits % 8, 0, "byte-oriented vectors only");
            let m = msg.take().unwrap();
            let m = &m[..bits / 8]; // Len = 0 is written with Msg = 00
            assert_eq!(hex(&sha256(m)), v, "{name}: Len = {bits}");
            // Incremental hashing in uneven pieces gives the same digest.
            let mut h = Sha256::new();
            for piece in m.chunks(37) {
                h.update(piece);
            }
            assert_eq!(hex(&h.finish()), v, "{name}: Len = {bits} (incremental)");
            n += 1;
        }
    }
    assert_eq!(n, expected_count, "{name}: vector count");
}

#[test]
fn ac1_short_messages() {
    check_rsp("sha256-cavp-v1/short-msg.rsp", 65);
}

#[test]
fn ac1_long_messages() {
    check_rsp("sha256-cavp-v1/long-msg.rsp", 64);
}

/// FIPS 180-4 examples ("abc", 448-bit, 896-bit, one million 'a') and NIST CAVP samples.
#[test]
fn ac1_fips180_examples() {
    let text = read("sha256-cavp-v1/fips180-examples.txt");
    let mut names = Vec::new();
    for line in lines(&text) {
        let f: Vec<&str> = line.split('\t').collect();
        let msg = match f[1].strip_prefix("repeat:") {
            Some(spec) => {
                let (byte, count) = spec.split_once(':').unwrap();
                vec![unhex(byte)[0]; count.parse().unwrap()]
            }
            None => unhex(f[1]),
        };
        assert_eq!(hex(&sha256(&msg)), f[2], "{}", f[0]);
        names.push(f[0].to_string());
    }
    for n in ["fips-abc", "fips-448", "fips-896", "million-a"] {
        assert!(names.iter().any(|x| x == n), "missing {n}");
    }
}

/// Cross-check against the independent `sha2` crate for every length around block borders,
/// and every split point of incremental updates.
#[test]
fn sha256_matches_independent_implementation() {
    let data: Vec<u8> = (0..1000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    for n in 0..=300 {
        assert_eq!(sha256_digest(&data[..n]).as_str(), fid_testkit::sha256_hex(&data[..n]), "len {n}");
    }
    let m = &data[..200];
    let want = sha256(m);
    for split in 0..=m.len() {
        let mut h = Sha256::new();
        h.update(&m[..split]);
        h.update(&[]);
        h.update(&m[split..]);
        assert_eq!(h.finish(), want, "split {split}");
    }
}

// ---------------------------------------------------------- REQ-007: canonical JSON writer

fn parse_tokens(toks: &mut std::slice::Iter<'_, &str>) -> Json {
    let t = *toks.next().unwrap();
    match t {
        "[" => {
            let mut items = Vec::new();
            loop {
                if toks.as_slice().first() == Some(&"]") {
                    toks.next();
                    return Json::Arr(items);
                }
                items.push(parse_tokens(toks));
            }
        }
        "{" => {
            let mut members = Vec::new();
            loop {
                let k = *toks.next().unwrap();
                if k == "}" {
                    return Json::Obj(members);
                }
                let key = String::from_utf8(unhex(k.strip_prefix('k').unwrap())).unwrap();
                members.push((key, parse_tokens(toks)));
            }
        }
        s => Json::Str(String::from_utf8(unhex(s.strip_prefix('s').unwrap())).unwrap()),
    }
}

#[test]
fn canonical_json_writer_vectors() {
    let text = read("canon-json-v1.txt");
    let mut n = 0;
    for line in lines(&text) {
        let f: Vec<&str> = line.split('\t').collect();
        let toks: Vec<&str> = f[1].split(' ').collect();
        let mut it = toks.iter();
        let v = parse_tokens(&mut it);
        assert!(it.next().is_none(), "{}: trailing tokens", f[0]);
        match to_canonical(&v) {
            Ok(bytes) => assert_eq!(hex(&bytes), f[2], "{}", f[0]),
            Err(e) => {
                assert_eq!(f[2], "ERR duplicate_key", "{}: unexpected {e}", f[0]);
                assert!(matches!(e, JsonError::DuplicateKey(_)));
                assert_eq!(e.code(), "duplicate_key");
            }
        }
        n += 1;
    }
    assert_eq!(n, 71);
}

// ------------------------------------------------------------- the test algorithm and table

/// `test.echo.identity@1.0.0`: params must be printable ASCII; output is
/// `params | seed-or-"-" | input0 | input1 ...` (see make_kci_fixtures.py).
struct Echo {
    id: &'static str,
    calls: Rc<Cell<usize>>,
}

impl KernelAlgorithm for Echo {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::parse(self.id).unwrap()
    }
    /// The echo takes any number of inputs (its card says `"0.."`).
    fn input_arity(&self) -> InputArity {
        InputArity::at_least(0)
    }
    fn validate(&self, params: &[u8]) -> Result<(), KernelError> {
        self.calls.set(self.calls.get() + 1);
        if params.iter().all(|b| (0x20..=0x7e).contains(b)) {
            Ok(())
        } else {
            Err(KernelError::InvalidParams { reason: "params_not_printable_ascii" })
        }
    }
    fn run(&self, params: &[u8], seed: Option<u64>, inputs: &[&[u8]]) -> Result<Vec<u8>, KernelError> {
        self.calls.set(self.calls.get() + 1);
        if params == b"fail" {
            return Err(KernelError::Algorithm(AlgorithmFailure { code: "asked_to_fail" }));
        }
        let mut out = params.to_vec();
        out.push(b'|');
        match seed {
            Some(s) => out.extend_from_slice(s.to_string().as_bytes()),
            None => out.push(b'-'),
        }
        for x in inputs {
            out.push(b'|');
            out.extend_from_slice(x);
        }
        Ok(out)
    }
}

const ECHO: &str = "test.echo.identity@1.0.0";
const NOCARD: &str = "test.echo.nocard@1.0.0";

fn echo_card() -> String {
    [
        "id = \"test.echo.identity@1.0.0\"",
        "title = \"Echo test algorithm\"",
        "method = \"Concatenates parameters, seed and inputs.\"",
        "applicability = \"Tests of the kernel call interface only.\"",
        "references = [\"Spec node KRN.MAN.KCI, acceptance criterion AC-2.\"]",
        "determinism = \"D0\"",
        "failure_modes = [\"Non-printable parameter bytes are refused.\"]",
        "validation = \"fixtures/man/kci-calls-v1.txt\"",
        "input_arity = \"0..\"",
    ]
    .join("\n")
}

fn table() -> (AlgorithmTable, Rc<Cell<usize>>, Rc<Cell<usize>>) {
    let (c1, c2) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let algs: Vec<Box<dyn KernelAlgorithm>> =
        vec![Box::new(Echo { id: NOCARD, calls: c2.clone() }), Box::new(Echo { id: ECHO, calls: c1.clone() })];
    let cards = CardRegistry::new(vec![parse_card(&echo_card()).unwrap()]).unwrap();
    (AlgorithmTable::new(algs, cards).unwrap(), c1, c2)
}

fn call(alg: &str, params: &[u8], seed: Option<u64>, inputs: &[&[u8]]) -> KernelCall {
    KernelCall {
        algorithm: AlgorithmId::parse(alg).unwrap(),
        params: params.to_vec(),
        seed,
        inputs: inputs.iter().map(|x| (x.to_vec(), sha256_digest(x))).collect(),
    }
}

// ---------------------------------------------------------------- AC-2: golden results

/// AC-2: every result's canonical bytes equal the golden bytes from Python. CI runs this on
/// x86-64 (Linux, Windows) and ARM64 (macOS), so equal goldens mean equal bytes across targets.
#[test]
fn ac2_golden_calls() {
    let (t, counter, _) = table();
    let text = read("kci-calls-v1.txt");
    let mut n = 0;
    for line in lines(&text) {
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 5, "{line}");
        let params = unhex(f[1]);
        let seed = if f[2] == "-" { None } else { Some(f[2].parse::<u64>().unwrap()) };
        let inputs: Vec<Vec<u8>> = if f[3] == "-" { vec![] } else { f[3].split(',').map(unhex).collect() };
        let refs: Vec<&[u8]> = inputs.iter().map(Vec::as_slice).collect();
        let c = call(ECHO, &params, seed, &refs);
        // The claimed digests also agree with the independent sha2 crate.
        for (x, d) in &c.inputs {
            assert_eq!(d.as_str(), fid_testkit::sha256_hex(x));
        }
        let r = evaluate(&t, &c).unwrap_or_else(|e| panic!("{}: {e}", f[0]));
        assert_eq!(String::from_utf8(r.canonical_json()).unwrap(), f[4], "{}", f[0]);
        assert_eq!(r.seed(), seed);
        assert_eq!(r.algorithm().to_string(), ECHO);
        assert_eq!(r.output_digest().as_str(), fid_testkit::sha256_hex(r.output()));
        assert_eq!(r.params_digest().as_str(), fid_testkit::sha256_hex(&params));
        assert_eq!(r.input_digests().len(), inputs.len());
        // Same call, same bytes.
        assert_eq!(evaluate(&t, &c).unwrap(), r);
        n += 1;
    }
    assert_eq!(n, 20);
    assert_eq!(counter.get(), 20 * 2 * 2, "validate + run per evaluation");
}

// --------------------------------------------------------------- AC-3: digest mismatch

fn flip(d: Digest, byte: usize, mask: u8) -> Digest {
    let mut b = d.to_bytes();
    b[byte] ^= mask;
    Digest::from_bytes(b)
}

#[test]
fn ac3_flipped_bit_in_second_input() {
    let (t, counter, _) = table();
    let mut c = call(ECHO, b"p", Some(1), &[b"first", b"second", b"third"]);
    c.inputs[1].1 = flip(c.inputs[1].1, 0, 0x01);
    let err = evaluate(&t, &c).unwrap_err();
    assert_eq!(err, KernelError::DigestMismatch { input_index: 1 });
    assert_eq!(err.code(), "digest_mismatch");
    assert_eq!(counter.get(), 0, "no algorithm code may run");
    // The last bit of the first input, with invalid params as well: still a mismatch, still no call.
    let mut c = call(ECHO, b"\x00bad", None, &[b"a", b"b"]);
    c.inputs[0].1 = flip(c.inputs[0].1, 31, 0x80);
    assert_eq!(evaluate(&t, &c), Err(KernelError::DigestMismatch { input_index: 0 }));
    // An empty input claiming the digest of a non-empty one.
    let mut c = call(ECHO, b"p", None, &[b""]);
    c.inputs[0].1 = sha256_digest(b"x");
    assert_eq!(evaluate(&t, &c), Err(KernelError::DigestMismatch { input_index: 0 }));
    assert_eq!(counter.get(), 0);
}

// ------------------------------------------------- AC-4: unknown ID, missing card, params

#[test]
fn ac4_unknown_algorithm_and_missing_card() {
    let (t, c1, c2) = table();
    let err = evaluate(&t, &call("fit.nothing.ls@1.0.0", b"", None, &[])).unwrap_err();
    assert_eq!(err, KernelError::UnknownAlgorithm(AlgorithmId::parse("fit.nothing.ls@1.0.0").unwrap()));
    assert_eq!(err.code(), "unknown_algorithm");
    // Exact semver: another version of a registered algorithm is unknown.
    let err = evaluate(&t, &call("test.echo.identity@1.0.1", b"", None, &[])).unwrap_err();
    assert_eq!(err.code(), "unknown_algorithm");
    let err = evaluate(&t, &call(NOCARD, b"", None, &[b"x"])).unwrap_err();
    assert_eq!(err, KernelError::MissingSpecCard(AlgorithmId::parse(NOCARD).unwrap()));
    assert_eq!(err.code(), "missing_spec_card");
    assert_eq!((c1.get(), c2.get()), (0, 0));
    // The card check of KRN.MAN.CARD sees the same gap.
    let report = fid_kci::check(&t.ids(), &t.cards().ids());
    assert_eq!(report.missing_cards, vec![AlgorithmId::parse(NOCARD).unwrap()]);
}

#[test]
fn ac4_invalid_params_compute_nothing() {
    let (t, counter, _) = table();
    let err = evaluate(&t, &call(ECHO, b"tab\there", None, &[b"x"])).unwrap_err();
    assert_eq!(err, KernelError::InvalidParams { reason: "params_not_printable_ascii" });
    assert_eq!(err.code(), "invalid_params");
    assert_eq!(counter.get(), 1, "validate ran, run did not");
}

#[test]
fn algorithm_errors_and_duplicates_are_typed() {
    let (t, _, _) = table();
    let err = evaluate(&t, &call(ECHO, b"fail", None, &[])).unwrap_err();
    assert_eq!(err, KernelError::Algorithm(AlgorithmFailure { code: "asked_to_fail" }));
    assert_eq!(err.code(), "algorithm_failed");
    let dup: Vec<Box<dyn KernelAlgorithm>> = vec![
        Box::new(Echo { id: ECHO, calls: Rc::new(Cell::new(0)) }),
        Box::new(Echo { id: ECHO, calls: Rc::new(Cell::new(0)) }),
    ];
    let err = AlgorithmTable::new(dup, CardRegistry::default()).unwrap_err();
    assert_eq!(err.code(), "duplicate_algorithm");
    let card = parse_card(&echo_card()).unwrap();
    let err = CardRegistry::new(vec![card.clone(), card]).unwrap_err();
    assert_eq!(err.code(), "duplicate_card");
}

/// REQ-KRN.MAN.KCI-006: the FP probe runs first; on CI hosts the environment is IEEE
/// round-to-nearest-even without flush-to-zero, so a valid call succeeds. (The failing branch
/// cannot be provoked without `unsafe` FP-control access; fid-num tests its probe.)
#[test]
fn fp_probe_passes_on_conforming_hosts() {
    assert!(fid_num::probe_fp_env().is_ok());
    let (t, _, _) = table();
    assert!(evaluate(&t, &call(ECHO, b"", None, &[])).is_ok());
}

// ------------------------------------------------- REQ-KRN.MAN.CARD-005: card digests

#[test]
fn card_canonical_bytes_and_digests() {
    let text = read("card-digests-v1.txt");
    let mut cards = Vec::new();
    let mut expected = Vec::new();
    for line in lines(&text) {
        let f: Vec<&str> = line.split('\t').collect();
        let card = parse_card(&read(&format!("cards-v1/{}", f[0]))).unwrap();
        assert_eq!(String::from_utf8(card.canonical_bytes()).unwrap(), f[1], "{}", f[0]);
        expected.push((card.id.clone(), f[1].to_string(), f[2].to_string()));
        cards.push(card);
    }
    assert_eq!(cards.len(), 3);
    let reg = CardRegistry::new(cards).unwrap();
    for (id, json, digest) in expected {
        assert_eq!(reg.canonical_bytes(&id).unwrap(), json.as_bytes());
        assert_eq!(reg.digest(&id).unwrap().hex(), digest);
        assert_eq!(reg.get(&id).unwrap().id, id);
    }
    let mut ids = reg.ids();
    let sorted = ids.clone();
    ids.sort();
    assert_eq!(ids, sorted);
}

/// An algorithm returning a kernel-owned error variant cannot impersonate a kernel check:
/// the error comes back as `Algorithm(IMPERSONATION_CODE)`.
struct Impostor {
    in_validate: bool,
}

impl KernelAlgorithm for Impostor {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::parse(ECHO).unwrap()
    }
    fn input_arity(&self) -> InputArity {
        InputArity::at_least(0)
    }
    fn validate(&self, _: &[u8]) -> Result<(), KernelError> {
        if self.in_validate {
            Err(KernelError::DigestMismatch { input_index: 0 })
        } else {
            Ok(())
        }
    }
    fn run(&self, _: &[u8], _: Option<u64>, _: &[&[u8]]) -> Result<Vec<u8>, KernelError> {
        Err(KernelError::MissingSpecCard(AlgorithmId::parse(ECHO).unwrap()))
    }
}

#[test]
fn algorithms_cannot_impersonate_kernel_errors() {
    let confined = KernelError::Algorithm(AlgorithmFailure { code: fid_kci::kci::IMPERSONATION_CODE });
    for in_validate in [true, false] {
        let cards = CardRegistry::new(vec![parse_card(&echo_card()).unwrap()]).unwrap();
        let t = AlgorithmTable::new(vec![Box::new(Impostor { in_validate })], cards).unwrap();
        assert_eq!(evaluate(&t, &call(ECHO, b"", None, &[b"x"])), Err(confined.clone()), "validate={in_validate}");
    }
    // The permitted variants pass through unchanged (see also ac4_invalid_params_compute_nothing).
    let (t, _, _) = table();
    assert_eq!(
        evaluate(&t, &call(ECHO, b"fail", None, &[])),
        Err(KernelError::Algorithm(AlgorithmFailure { code: "asked_to_fail" }))
    );
    // An algorithm cannot fake the kernel's arity refusal either.
    let fake = KernelError::InputArityMismatch { expected: InputArity::exactly(1), got: 0 };
    struct FakeArity(KernelError);
    impl KernelAlgorithm for FakeArity {
        fn id(&self) -> AlgorithmId {
            AlgorithmId::parse(ECHO).unwrap()
        }
        fn input_arity(&self) -> InputArity {
            InputArity::at_least(0)
        }
        fn validate(&self, _: &[u8]) -> Result<(), KernelError> {
            Ok(())
        }
        fn run(&self, _: &[u8], _: Option<u64>, _: &[&[u8]]) -> Result<Vec<u8>, KernelError> {
            Err(self.0.clone())
        }
    }
    let cards = CardRegistry::new(vec![parse_card(&echo_card()).unwrap()]).unwrap();
    let t = AlgorithmTable::new(vec![Box::new(FakeArity(fake))], cards).unwrap();
    assert_eq!(evaluate(&t, &call(ECHO, b"", None, &[])), Err(confined));
}

// ------------------------------------- REQ-KRN.MAN.KCI-008, -009: declared input arity

/// A test algorithm whose `input_arity` is set per instance; `run` counts its calls and
/// returns the inputs joined by `|`.
struct Declared {
    id: &'static str,
    arity: InputArity,
    calls: Rc<Cell<usize>>,
}

impl KernelAlgorithm for Declared {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::parse(self.id).unwrap()
    }
    fn input_arity(&self) -> InputArity {
        self.arity
    }
    fn validate(&self, _: &[u8]) -> Result<(), KernelError> {
        self.calls.set(self.calls.get() + 1);
        Ok(())
    }
    fn run(&self, _: &[u8], _: Option<u64>, inputs: &[&[u8]]) -> Result<Vec<u8>, KernelError> {
        self.calls.set(self.calls.get() + 1);
        Ok(inputs.join(&b'|'))
    }
}

const PAIR: &str = "test.echo.pair@1.0.0";
const RANGE: &str = "test.echo.range@1.0.0";

/// `echo_card` with another ID and `input_arity`.
fn card_with(id: &str, arity: &str) -> String {
    echo_card().replace(ECHO, id).replace("input_arity = \"0..\"", &format!("input_arity = \"{arity}\""))
}

fn arity_table() -> (AlgorithmTable, Rc<Cell<usize>>, Rc<Cell<usize>>) {
    let (pair_calls, range_calls) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let algs: Vec<Box<dyn KernelAlgorithm>> = vec![
        Box::new(Declared { id: PAIR, arity: InputArity::exactly(2), calls: pair_calls.clone() }),
        Box::new(Declared { id: RANGE, arity: InputArity::between(1, 3).unwrap(), calls: range_calls.clone() }),
    ];
    let cards = CardRegistry::new(vec![
        parse_card(&card_with(PAIR, "2")).unwrap(),
        parse_card(&card_with(RANGE, "1..=3")).unwrap(),
    ])
    .unwrap();
    (AlgorithmTable::new(algs, cards).unwrap(), pair_calls, range_calls)
}

/// REQ-KRN.MAN.KCI-008: a call whose input count is outside the card's `input_arity` is
/// refused with `InputArityMismatch` before any algorithm code runs (validate and run
/// counters stay 0), and before inputs are hashed (a bad digest is not reported).
#[test]
fn wrong_input_count_is_refused_before_the_algorithm_runs() {
    let (t, pair_calls, range_calls) = arity_table();
    let inputs: [&[u8]; 5] = [b"a", b"b", b"c", b"d", b"e"];
    for n in [0, 1, 3, 4, 5] {
        let err = evaluate(&t, &call(PAIR, b"", None, &inputs[..n])).unwrap_err();
        assert_eq!(err, KernelError::InputArityMismatch { expected: InputArity::exactly(2), got: n }, "n={n}");
        assert_eq!(err.code(), "input_arity_mismatch");
    }
    for n in [0, 4, 5] {
        let err = evaluate(&t, &call(RANGE, b"", None, &inputs[..n])).unwrap_err();
        assert_eq!(err.code(), "input_arity_mismatch", "n={n}");
    }
    // Arity is checked before digests: a wrong count with a corrupt digest is an arity error.
    let mut c = call(PAIR, b"", None, &inputs[..3]);
    c.inputs[0].1 = sha256_digest(b"not a");
    assert_eq!(evaluate(&t, &c).unwrap_err().code(), "input_arity_mismatch");
    assert_eq!((pair_calls.get(), range_calls.get()), (0, 0), "no algorithm code may run");
    // Accepted counts reach the algorithm (validate + run = 2 calls each).
    assert_eq!(evaluate(&t, &call(PAIR, b"", None, &inputs[..2])).unwrap().output(), b"a|b");
    assert_eq!(pair_calls.get(), 2);
    for n in 1..=3 {
        assert!(evaluate(&t, &call(RANGE, b"", None, &inputs[..n])).is_ok(), "n={n}");
    }
    assert_eq!(range_calls.get(), 6);
}

/// REQ-KRN.MAN.KCI-009: every algorithm's own `input_arity` must equal its card's; a drifted
/// pair makes `AlgorithmTable::new` fail, so no table with drift can evaluate anything.
#[test]
fn card_and_implementation_arity_must_agree() {
    // Every algorithm in this file's tables agrees with its card.
    for (t, _, _) in [table(), arity_table()] {
        for id in t.cards().ids() {
            assert!(t.ids().contains(&id));
        }
    }
    let drift = |card: &str, imp: InputArity| {
        let cards = CardRegistry::new(vec![parse_card(&card_with(PAIR, card)).unwrap()]).unwrap();
        let alg = Declared { id: PAIR, arity: imp, calls: Rc::new(Cell::new(0)) };
        AlgorithmTable::new(vec![Box::new(alg)], cards)
    };
    assert!(drift("2", InputArity::exactly(2)).is_ok());
    for (card, imp) in [
        ("2", InputArity::exactly(1)),
        ("2", InputArity::exactly(3)),
        ("2", InputArity::at_least(2)),
        ("2..", InputArity::exactly(2)),
        ("1..=3", InputArity::between(1, 4).unwrap()),
        ("0..", InputArity::at_least(1)),
    ] {
        let err = drift(card, imp).unwrap_err();
        assert_eq!(err.code(), "card_arity_drift", "{card} vs {imp}");
        assert_eq!(
            err,
            KernelError::CardArityDrift {
                id: AlgorithmId::parse(PAIR).unwrap(),
                card: InputArity::parse(card).unwrap(),
                implementation: imp,
            }
        );
    }
}

/// REQ-KRN.MAN.CARD-006: the `input_arity` spellings, their round trip and their refusals.
#[test]
fn input_arity_grammar() {
    let ok = [
        ("0", 0, Some(0)),
        ("1", 1, Some(1)),
        ("4294967295", u32::MAX, Some(u32::MAX)),
        ("0..", 0, None),
        ("2..", 2, None),
        ("1..=3", 1, Some(3)),
        ("0..=4294967295", 0, Some(u32::MAX)),
    ];
    for (s, min, max) in ok {
        let a = InputArity::parse(s).unwrap();
        assert_eq!((a.min(), a.max()), (min, max), "{s}");
        assert_eq!(a.to_string(), s, "one spelling per value");
    }
    for s in
        ["", "01", "-1", "1.5", "4294967296", "1..=1", "3..=2", "..3", "..", "1..2", "1..=", "1...", " 1", "1 ", "x"]
    {
        assert_eq!(InputArity::parse(s), Err(CardError::BadArity(s.to_string())), "{s:?}");
    }
    let a = InputArity::between(1, 3).unwrap();
    assert_eq!([0, 1, 2, 3, 4].map(|n| a.accepts(n)), [false, true, true, true, false]);
    assert!(InputArity::at_least(2).accepts(usize::MAX) && !InputArity::exactly(2).accepts(usize::MAX));
    assert_eq!(InputArity::between(3, 2), None);
    // A card without the field, or with a bad one, is refused.
    let text = echo_card().replace("input_arity = \"0..\"", "");
    assert_eq!(parse_card(&text), Err(CardError::MissingField("input_arity")));
    let text = echo_card().replace("\"0..\"", "\"1..=1\"");
    assert_eq!(parse_card(&text).unwrap_err().code(), "bad_arity");
}

#[test]
fn canonical_json_depth_limit() {
    use fid_kci::kci::json::MAX_DEPTH;
    let nest = |n: usize| (0..n).fold(Json::Str("x".into()), |v, _| Json::Arr(vec![v]));
    let ok = to_canonical(&nest(MAX_DEPTH)).unwrap();
    assert_eq!(ok.len(), 2 * MAX_DEPTH + 3);
    assert_eq!(to_canonical(&nest(MAX_DEPTH + 1)), Err(JsonError::TooDeep));
    let obj = (0..MAX_DEPTH + 1).fold(Json::Str("x".into()), |v, _| Json::Obj(vec![("k".into(), v)]));
    assert_eq!(to_canonical(&obj).map_err(|e| e.code()), Err("too_deep"));
}

// ------------------------------------------------------------------ AC-5: KNP-1 source scan

/// AC-5: the crate source uses no I/O, clock, environment, global state, `unsafe`, hash maps,
/// FMA or float min/max. `unsafe_code = "forbid"` is also set workspace-wide.
///
/// Interim check: `fid-knp1-lint` (KRN.NUM.LINT) has only its findings model on main, no
/// detectors yet. When its detectors land, this test should be replaced by running that lint
/// on the kernel closure.
///
/// Float `min`/`max`: a text scan cannot tell a float receiver from an integer one, so every
/// `.min(` / `.max(` / `.clamp(` method call is banned in this crate (it uses no floats at all;
/// integer code writes the comparison out). `f64::min` / `f64::max` paths are banned too.
#[test]
fn ac5_knp1_source_scan() {
    let banned = [
        "unsafe",
        "std::fs",
        "std::io",
        "std::env",
        "std::net",
        "std::process",
        "std::time",
        "SystemTime",
        "Instant",
        "thread_local",
        "static mut",
        "OnceLock",
        "OnceCell",
        "lazy_static",
        "Mutex",
        "RwLock",
        "Atomic",
        "HashMap",
        "HashSet",
        "mul_add",
        "f64::min",
        "f64::max",
        "f32::",
        "println!",
        "eprintln!",
        "print!",
        "dbg!",
        "include_bytes!",
        "env!",
        "asm!",
        "core::arch",
        "std::arch",
        "extern ",
        ".min(",
        ".max(",
        ".clamp(",
    ];
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut dirs = vec![src];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                files.push(p);
            }
        }
    }
    assert!(files.len() >= 5);
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        for b in banned {
            assert!(!text.contains(b), "{}: KNP-1 finding {b:?}", f.display());
        }
    }
    let manifest = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let deps = manifest.split("[dependencies]").nth(1).unwrap().split("\n[").next().unwrap();
    let names: Vec<&str> =
        deps.lines().filter(|l| l.contains('=')).map(|l| l.split('=').next().unwrap().trim()).collect();
    assert_eq!(names, vec!["fid-num", "fid-types"], "kernel dependencies (both in layer num)");
}
