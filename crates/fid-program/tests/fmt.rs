//! Acceptance criteria of spec node AUTO.PROG.FMT. Expected digests and error labels come
//! from `fixtures/program/make_fixtures.py` (independent Python writer, hand-labelled codes).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_program::{
    program_schema, step_fields, Arg, Bound, Program, ProgramError, Step, StepKind, Uuid7, MAX_BYTES, MAX_OBJECTS,
    MAX_STEPS, MAX_TOKENS, STEP_KINDS,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join("fixtures/program").join(rel)).unwrap()
}

fn fixtures(sub: &str) -> Vec<(String, Vec<u8>)> {
    let dir = root().join("fixtures/program").join(sub);
    let mut out: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()))
        .collect();
    out.sort();
    out
}

fn expected() -> BTreeMap<String, CValue> {
    match parse(&read("expected.json")).unwrap() {
        CValue::Obj(m) => m,
        _ => panic!("expected.json is not an object"),
    }
}

fn section(name: &str) -> BTreeMap<String, CValue> {
    match expected().remove(name) {
        Some(CValue::Obj(m)) => m,
        _ => panic!("expected.json has no {name}"),
    }
}

fn str_of(v: Option<&CValue>) -> Option<&str> {
    match v {
        Some(CValue::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// AC-1: every valid fixture (minimal, all-kinds, ...) round-trips byte for byte and its
/// digest equals the digest the Python reference recorded.
#[test]
fn ac1_valid_round_trip_and_digest() {
    let want = section("valid");
    let valid = fixtures("valid");
    assert_eq!(valid.len(), want.len(), "fixture set and expected.json disagree");
    for (name, bytes) in &valid {
        let p = Program::parse(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let again = p.to_canonical().unwrap();
        assert_eq!(String::from_utf8_lossy(&again), String::from_utf8_lossy(bytes), "{name}");
        let digest = p.digest().unwrap().to_string();
        assert_eq!(Some(digest.as_str()), str_of(want.get(name)), "{name}: digest");
    }
    assert!(valid.iter().any(|(n, _)| n == "minimal") && valid.iter().any(|(n, _)| n == "all-kinds"));
}

/// AC-1: `all-kinds` really holds every step kind (and both loop modes).
#[test]
fn ac1_all_kinds_covers_every_kind() {
    fn walk(steps: &[Step], seen: &mut Vec<String>) {
        for s in steps {
            let mut k = s.kind.as_str().to_string();
            if let StepKind::Loop { mode, .. } = &s.kind {
                k = format!(
                    "loop/{}",
                    if matches!(mode, fid_program::LoopMode::Counted { .. }) { "counted" } else { "while" }
                );
                seen.push("loop".into());
            }
            seen.push(k);
            match &s.kind {
                StepKind::If { branches, else_body } => {
                    branches.iter().for_each(|b| walk(&b.body, seen));
                    walk(else_body, seen);
                }
                StepKind::Loop { body, .. } => walk(body, seen),
                _ => {}
            }
        }
    }
    let p = Program::parse(&read("valid/all-kinds.json")).unwrap();
    let mut seen = Vec::new();
    walk(&p.main, &mut seen);
    for sub in &p.subroutines {
        walk(&sub.body, &mut seen);
    }
    for k in STEP_KINDS.iter().map(|k| k.to_string()).chain(["loop/counted".into(), "loop/while".into()]) {
        assert!(seen.contains(&k), "all-kinds lacks {k}");
    }
    assert!(!p.subroutines.is_empty() && !p.params.is_empty() && !p.variables.is_empty());
}

/// AC-2 .. AC-5 and the further hand-labelled error fixtures: each fails with its code and detail.
#[test]
fn ac2_to_ac5_invalid_fixtures_fail_with_labelled_code() {
    let want = section("invalid");
    let invalid = fixtures("invalid");
    assert_eq!(invalid.len(), want.len(), "fixture set and expected.json disagree");
    for (name, bytes) in &invalid {
        let Some(CValue::Obj(label)) = want.get(name) else { panic!("{name}: no label") };
        let err = Program::parse(bytes).err().unwrap_or_else(|| panic!("{name}: parsed but should fail"));
        assert_eq!(Some(err.code()), str_of(label.get("code")), "{name}: {err}");
        match &err {
            ProgramError::DuplicateStepId(id) => assert_eq!(Some(id.to_string().as_str()), str_of(label.get("id"))),
            ProgramError::UnknownField { path, field } => {
                assert_eq!(Some(field.as_str()), str_of(label.get("field")), "{name}");
                assert_eq!(Some(path.as_str()), str_of(label.get("path")), "{name}");
            }
            ProgramError::SchemaInvalid { path, .. } => {
                assert_eq!(Some(path.as_str()), str_of(label.get("path")), "{name}: {err}")
            }
            ProgramError::FormatVersionUnsupported(v) => assert_eq!(Some(v.as_str()), str_of(label.get("value"))),
            ProgramError::ProgramTooLarge(b) => assert_eq!(Some(b.as_str()), str_of(label.get("bound"))),
            ProgramError::InvalidJson(c) => assert_eq!(Some(*c), str_of(label.get("detail"))),
            ProgramError::NonCanonical => {}
        }
    }
}

/// AC-2: the pretty-printed twin of a valid program is refused as non-canonical.
#[test]
fn ac2_whitespace_is_non_canonical() {
    assert_eq!(Program::parse(&read("invalid/whitespace.json")), Err(ProgramError::NonCanonical));
}

/// AC-3: duplicate step IDs are refused, naming the ID.
#[test]
fn ac3_duplicate_step_id_names_the_id() {
    let p = Program::parse(&read("valid/all-kinds.json")).unwrap();
    let dup = p.main[1].step_id;
    let err = Program::parse(&read("invalid/dup-step.json")).unwrap_err();
    assert_eq!(err, ProgramError::DuplicateStepId(dup));
    assert!(err.to_string().contains(&dup.to_string()));
}

/// AC-4: an `assign` step carrying `command` fails with `unknown_field`.
#[test]
fn ac4_assign_with_command_is_unknown_field() {
    let err = Program::parse(&read("invalid/extra-field.json")).unwrap_err();
    assert_eq!(err, ProgramError::UnknownField { path: "/main/2".into(), field: "command".into() });
}

/// AC-5: format version 2 is refused.
#[test]
fn ac5_unknown_format_version() {
    let err = Program::parse(&read("invalid/version-2.json")).unwrap_err();
    assert_eq!(err, ProgramError::FormatVersionUnsupported("2".into()));
}

/// REQ-004: only `invoke` has a command field; no other step kind has any field naming a
/// command, major, effect or side effect.
#[test]
fn req004_only_invoke_names_a_command() {
    for kind in STEP_KINDS {
        let fields = step_fields(kind);
        assert!(fields.contains(&"step_id") && fields.contains(&"kind"), "{kind}");
        let has_command = fields.contains(&"command") || fields.contains(&"major");
        assert_eq!(has_command, kind == "invoke", "{kind}: {fields:?}");
        assert!(!fields.iter().any(|f| f.contains("effect")), "{kind}");
    }
    assert_eq!(step_fields("invoke"), vec!["args", "bind", "command", "kind", "major", "step_id"]);
}

/// REQ-002 for agents and people: an edit (delete one step, add one) keeps every other
/// step ID and reproduces the Python reference's edited bytes exactly.
#[test]
fn req002_step_ids_stable_across_edits() {
    let edit = section("edit");
    let Some(CValue::Obj(add)) = edit.get("append_comment") else { panic!() };
    let new_id = Uuid7::parse(str_of(add.get("step_id")).unwrap()).unwrap();
    let mut p = Program::parse(&read("valid/all-kinds.json")).unwrap();
    let before = p.step_ids();
    let removed = p.main.remove(0);
    p.main.push(Step { step_id: new_id, kind: StepKind::Comment { text: str_of(add.get("text")).unwrap().into() } });
    let bytes = p.to_canonical().unwrap();
    assert_eq!(String::from_utf8_lossy(&bytes), String::from_utf8_lossy(&read("valid/edited.json")));
    let after = Program::parse(&bytes).unwrap().step_ids();
    let kept: Vec<_> = before.iter().filter(|id| **id != removed.step_id).copied().collect();
    assert_eq!(after.iter().filter(|id| **id != new_id).copied().collect::<Vec<_>>(), kept);
}

/// REQ-007: the checked-in schema is generated from the Rust field tables.
#[test]
fn req007_schema_is_generated() {
    let mut want = to_canonical_bytes(&program_schema()).unwrap();
    want.push(b'\n');
    let have = std::fs::read(root().join("schema/program-v1.json")).unwrap_or_default();
    assert!(
        have == want,
        "schema/program-v1.json is out of date. Replace its contents with:\n{}",
        String::from_utf8_lossy(&want)
    );
}

fn header(main: &str) -> String {
    format!(
        "{{\"format\":\"fid-program\",\"format_version\":1,\"main\":[{main}],\"meta\":{{}},\"params\":[],\
         \"program_id\":\"018f0000-0000-7000-8000-000000000000\",\"subroutines\":[],\"variables\":[]}}"
    )
}

fn invoke_with_literal(literal: &str) -> String {
    header(&format!(
        "{{\"args\":{{\"v\":{{\"literal\":{literal}}}}},\"command\":\"a.b\",\"kind\":\"invoke\",\"major\":1,\
         \"step_id\":\"018f0000-0000-7000-8000-000000000001\"}}"
    ))
}

/// REQ-006: every bound fails closed with `program_too_large`.
#[test]
fn req006_bounds() {
    // The header helper itself is a valid program.
    Program::parse(header("").as_bytes()).unwrap();
    Program::parse(invoke_with_literal("[0,{}]").as_bytes()).unwrap();

    let big = vec![b' '; MAX_BYTES + 1];
    assert_eq!(Program::parse(&big), Err(ProgramError::ProgramTooLarge(Bound::Bytes)));

    let objects = vec!["{}"; MAX_OBJECTS + 1].join(",");
    let doc = invoke_with_literal(&format!("[{objects}]"));
    assert_eq!(Program::parse(doc.as_bytes()), Err(ProgramError::ProgramTooLarge(Bound::Objects)));

    let tokens = vec!["0"; MAX_TOKENS + 1].join(",");
    let doc = invoke_with_literal(&format!("[{tokens}]"));
    assert_eq!(Program::parse(doc.as_bytes()), Err(ProgramError::ProgramTooLarge(Bound::Tokens)));

    // Structural characters inside strings are not counted.
    let quoted = format!("\"{}\"", "{".repeat(MAX_OBJECTS + 1));
    Program::parse(invoke_with_literal(&quoted).as_bytes()).unwrap();

    // MAX_STEPS + 1 comment steps; IDs differ in their last 12 hex digits.
    let steps: Vec<String> = (0..=MAX_STEPS)
        .map(|i| format!("{{\"kind\":\"comment\",\"step_id\":\"018f0000-0000-7000-8000-{i:012x}\",\"text\":\"\"}}"))
        .collect();
    let doc = header(&steps.join(","));
    assert!(doc.len() < MAX_BYTES);
    assert_eq!(Program::parse(doc.as_bytes()), Err(ProgramError::ProgramTooLarge(Bound::Steps)));
}

/// Serialising refuses an in-memory program that would not parse back.
#[test]
fn to_canonical_refuses_invalid_programs() {
    let mut p = Program::parse(&read("valid/minimal.json")).unwrap();
    p.main.push(Step {
        step_id: Uuid7::from_parts(1, [0; 10]),
        kind: StepKind::Assign { var: "not an identifier".into(), expr: "1".into() },
    });
    assert_eq!(p.to_canonical().unwrap_err().code(), "schema_invalid");

    let mut p = Program::parse(&read("valid/minimal.json")).unwrap();
    let mut args = BTreeMap::new();
    args.insert("x".to_string(), Arg::Literal(CValue::Num(f64::NAN)));
    p.main.push(Step {
        step_id: Uuid7::from_parts(1, [0; 10]),
        kind: StepKind::Invoke { command: "a.b".into(), major: 1, args, bind: None },
    });
    assert_eq!(p.to_canonical().unwrap_err(), ProgramError::InvalidJson("non_finite_number"));

    let mut p = Program::parse(&read("valid/minimal.json")).unwrap();
    let id = Uuid7::from_parts(1, [0; 10]);
    p.main.push(Step { step_id: id, kind: StepKind::Label { name: "a".into() } });
    p.main.push(Step { step_id: id, kind: StepKind::Label { name: "b".into() } });
    assert_eq!(p.to_canonical().unwrap_err(), ProgramError::DuplicateStepId(id));

    // Hostile nesting built in memory is refused, not recursed into without limit.
    let mut p = Program::parse(&read("valid/minimal.json")).unwrap();
    let mut body = Vec::new();
    for i in 0..10_000u64 {
        body = vec![Step {
            step_id: Uuid7::from_parts(i, [1; 10]),
            kind: StepKind::Loop { mode: fid_program::LoopMode::While { cond: "x".into() }, max_iterations: 1, body },
        }];
    }
    p.main = body;
    assert_eq!(p.to_canonical().unwrap_err(), ProgramError::ProgramTooLarge(Bound::Depth));
    // Dropping a deep tree is the caller's business; leak it so the test does not depend on stack size.
    std::mem::forget(p);
}

#[test]
fn uuid7_text_form() {
    let id = Uuid7::from_parts(0x0123_4567_89ab, [0xff; 10]);
    let text = id.to_string();
    assert_eq!(text, "01234567-89ab-7fff-bfff-ffffffffffff");
    assert_eq!(Uuid7::parse(&text), Some(id));
    assert_eq!(id.unix_ms(), 0x0123_4567_89ab);
    for bad in [
        "01234567-89AB-7fff-bfff-ffffffffffff", // uppercase
        "01234567-89ab-4fff-bfff-ffffffffffff", // version 4
        "01234567-89ab-7fff-7fff-ffffffffffff", // variant 0
        "0123456789ab7fffbfffffffffffffff",     // no hyphens
        "{01234567-89ab-7fff-bfff-ffffffffff}",
        "01234567-89ab-7fff-bfff-fffffffffffg",
        "",
    ] {
        assert_eq!(Uuid7::parse(bad), None, "{bad}");
    }
}

/// AC-6 (in-tree part): deterministic mutations of every fixture never panic. The
/// `cargo fuzz` target `program_parse` in `crates/fid-program/fuzz` covers the long run.
#[test]
fn ac6_mutated_fixtures_never_panic() {
    let mut seeds: Vec<Vec<u8>> = fixtures("valid").into_iter().map(|(_, b)| b).collect();
    seeds.extend(fixtures("invalid").into_iter().map(|(_, b)| b));
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let tokens: [&[u8]; 8] = [b"{", b"}", b"[", b"]", b"\"", b",", b":", b"\\u0000"];
    for seed in &seeds {
        for _ in 0..300 {
            let mut b = seed.clone();
            for _ in 0..(1 + next() % 4) {
                if b.is_empty() {
                    break;
                }
                let at = (next() % b.len() as u64) as usize;
                match next() % 4 {
                    0 => b[at] = next() as u8,
                    1 => b.truncate(at),
                    2 => {
                        let t = tokens[(next() % tokens.len() as u64) as usize];
                        b.splice(at..at, t.iter().copied());
                    }
                    _ => {
                        b.remove(at);
                    }
                }
            }
            let _ = Program::parse(&b);
        }
    }
}
