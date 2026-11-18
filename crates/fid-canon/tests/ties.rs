//! ECMA-262 Number::toString ties-to-even (Note 2) for RFC 8785 number output.
//! Expected strings come from `fixtures/canon/make_tie_vectors.py` (CPython repr()).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{format_number, parse, to_canonical_bytes, CValue};

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/canon/tie_vectors.json");

#[test]
fn review_examples_round_ties_to_even() {
    assert_eq!(format_number(277575414244990.125).unwrap(), "277575414244990.12");
    assert_eq!(format_number(-277575414244990.125).unwrap(), "-277575414244990.12");
    assert_eq!(format_number(64.044097900390625).unwrap(), "64.04409790039062");
    assert_eq!(format_number(0.00048923492431640625).unwrap(), "0.0004892349243164062");
    assert_eq!(canonical("[277575414244990.125]"), "[277575414244990.12]");
}

#[test]
fn python_repr_vectors() {
    let text = std::fs::read(VECTORS).expect("read tie_vectors.json");
    let CValue::Arr(rows) = parse(&text).expect("parse tie_vectors.json") else {
        panic!("tie_vectors.json must be an array");
    };
    assert!(rows.len() >= 8000, "expected at least 8000 vectors, have {}", rows.len());
    let mut failures = Vec::new();
    for row in &rows {
        let CValue::Arr(pair) = row else { panic!("row must be an array") };
        let (CValue::Str(hex), CValue::Str(want)) = (&pair[0], &pair[1]) else { panic!("row must be [str, str]") };
        let v = f64::from_bits(u64::from_str_radix(hex.strip_prefix("0x").unwrap(), 16).unwrap());
        let got = format_number(v).unwrap();
        if got != *want {
            failures.push(format!("{hex}: got {got} want {want}"));
            continue;
        }
        // The canonical writer uses the same text, and it reads back to the same double.
        assert_eq!(to_canonical_bytes(&CValue::Num(v)).unwrap(), want.as_bytes());
        assert_eq!(got.parse::<f64>().unwrap().to_bits(), v.to_bits(), "{hex} does not round-trip");
    }
    assert!(failures.is_empty(), "{} of {} vectors failed:\n{}", failures.len(), rows.len(), failures.join("\n"));
}

fn canonical(json: &str) -> String {
    String::from_utf8(fid_canon::canonicalize(json.as_bytes()).unwrap()).unwrap()
}
