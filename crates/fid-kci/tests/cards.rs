//! Acceptance criteria of spec node KRN.MAN.CARD (AC-1 .. AC-4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kci::{check, parse_card, AlgorithmId, CardError};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man")
}

/// AC-1: 50 valid and 50 invalid IDs are classified correctly.
#[test]
fn ac1_id_classification() {
    let text = std::fs::read_to_string(fixtures().join("card-ids-v1.txt")).unwrap();
    let (mut valid, mut invalid) = (0, 0);
    for line in text.split('\n').filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (kind, id) = line.split_once('\t').unwrap();
        let parsed = AlgorithmId::parse(id);
        match kind {
            "valid" => {
                valid += 1;
                let a = parsed.unwrap_or_else(|e| panic!("{id:?} should be valid: {e}"));
                assert_eq!(a.to_string(), id, "display must round-trip");
            }
            "invalid" => {
                invalid += 1;
                assert!(matches!(parsed, Err(CardError::BadId(_))), "{id:?} should be invalid");
            }
            other => panic!("bad fixture line kind {other}"),
        }
    }
    assert_eq!((valid, invalid), (50, 50));
}

/// AC-2: valid cards load; each invalid card names its missing field.
#[test]
fn ac2_cards() {
    let dir = fixtures().join("cards-v1");
    let read = |name: &str| std::fs::read_to_string(dir.join(format!("{name}.toml"))).unwrap();
    for name in ["valid-fit-cylinder-ls", "valid-gdt-flatness-mz", "valid-unc-mc-sample"] {
        let card = parse_card(&read(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!card.references.is_empty() && !card.failure_modes.is_empty(), "{name}");
    }
    for field in ["applicability", "references", "validation"] {
        let err = parse_card(&read(&format!("missing-{field}"))).unwrap_err();
        assert_eq!(err, CardError::MissingField(field), "missing-{field}");
    }
    let c = parse_card(&read("valid-fit-cylinder-ls")).unwrap();
    assert_eq!(c.id, AlgorithmId::parse("fit.cylinder.ls@1.0.0").unwrap());
    assert_eq!(c.determinism, "D0");
}

/// AC-3: algorithms without cards and cards without algorithms are both reported.
#[test]
fn ac3_check() {
    let id = |s| AlgorithmId::parse(s).unwrap();
    let (a, b, c) = (id("fit.plane.ls@1.0.0"), id("fit.circle.ls@1.0.0"), id("fit.sphere.ls@1.0.0"));
    let report = check(&[a.clone(), b.clone()], &[b.clone(), c.clone()]);
    assert_eq!(report.missing_cards, vec![a]);
    assert_eq!(report.orphan_cards, vec![c]);
    assert!(!report.is_clean());
    assert!(check(std::slice::from_ref(&b), std::slice::from_ref(&b)).is_clean());
}

/// AC-4: a 40-word quoted passage next to a standard reference is refused.
#[test]
fn ac4_standards_text() {
    let text = std::fs::read_to_string(fixtures().join("cards-v1/standards-text-quote.toml")).unwrap();
    let err = parse_card(&text).unwrap_err();
    assert_eq!(err.code(), "standards_text_suspected");
    assert_eq!(err, CardError::StandardsTextSuspected { field: "references[0]".into() });
}

#[test]
fn ordering_is_total_and_by_version_last() {
    let mut ids: Vec<AlgorithmId> = ["fit.plane.ls@1.10.0", "fit.plane.ls@1.2.0", "fit.circle.ls@9.0.0"]
        .iter()
        .map(|s| AlgorithmId::parse(s).unwrap())
        .collect();
    ids.sort();
    let shown: Vec<String> = ids.iter().map(ToString::to_string).collect();
    assert_eq!(shown, ["fit.circle.ls@9.0.0", "fit.plane.ls@1.2.0", "fit.plane.ls@1.10.0"]);
}

#[test]
fn card_syntax_errors() {
    let bad = [
        ("id = fit.cylinder.ls@1.0.0\n", "syntax"),
        ("id = \"x\"\nid = \"y\"\n", "syntax"),
        ("colour = \"red\"\n", "unknown_field"),
        ("references = [\"a\",\n", "syntax"),
        ("title = \"unterminated\n", "syntax"),
    ];
    for (text, code) in bad {
        assert_eq!(parse_card(text).unwrap_err().code(), code, "{text:?}");
    }
}

#[test]
fn too_long_field() {
    let dir = fixtures().join("cards-v1");
    let base = std::fs::read_to_string(dir.join("valid-gdt-flatness-mz.toml")).unwrap();
    let long = "x".repeat(2001);
    let text = base.replace("Any planar point set with at least 4 non-collinear points.", &long);
    assert_eq!(parse_card(&text).unwrap_err(), CardError::TooLong { field: "applicability".into() });
}
