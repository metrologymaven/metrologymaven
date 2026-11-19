//! `parse_with_budget`: the element cap added for bounded readers (TRUST.PKG.READER), and
//! that `parse` and `parse_with_limits` behave as before.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, parse_with_budget, parse_with_limits, CValue, CanonError, MAX_DEPTH, MAX_INPUT_BYTES};

fn budget(input: &[u8], n: usize) -> Result<CValue, CanonError> {
    parse_with_budget(input, MAX_INPUT_BYTES, MAX_DEPTH, n)
}

#[test]
fn every_value_and_key_counts_once() {
    // Scalars: one element each.
    assert!(budget(b"0", 1).is_ok());
    assert_eq!(budget(b"0", 0), Err(CanonError::InputLimit));
    // [0,0,0]: the array plus three numbers.
    assert!(budget(b"[0,0,0]", 4).is_ok());
    assert_eq!(budget(b"[0,0,0]", 3), Err(CanonError::InputLimit));
    // {"a":1,"b":[]}: object, two keys, two values.
    assert!(budget(br#"{"a":1,"b":[]}"#, 5).is_ok());
    assert_eq!(budget(br#"{"a":1,"b":[]}"#, 4), Err(CanonError::InputLimit));
    // Empty containers are one element.
    assert!(budget(b"{}", 1).is_ok());
    assert!(budget(b"[]", 1).is_ok());
}

#[test]
fn many_tiny_elements_stop_early() {
    let mut big = b"[".to_vec();
    for _ in 0..100_000 {
        big.extend_from_slice(b"0,");
    }
    big.extend_from_slice(b"0]");
    assert_eq!(budget(&big, 1000), Err(CanonError::InputLimit));
    let CValue::Arr(items) = budget(&big, 100_002).unwrap() else { panic!("array") };
    assert_eq!(items.len(), 100_001);
}

#[test]
fn same_result_as_unbudgeted_parse() {
    let text = br#"{"x":[1,2,{"y":"z"}],"w":null,"v":true}"#;
    assert_eq!(budget(text, usize::MAX), parse(text));
    assert_eq!(parse_with_limits(text, 1 << 20, 4), parse(text));
    // The other limits still apply first.
    assert_eq!(parse_with_budget(text, 4, MAX_DEPTH, usize::MAX), Err(CanonError::InputLimit));
    assert_eq!(parse_with_budget(b"[[[0]]]", 64, 2, usize::MAX), Err(CanonError::InputLimit));
    // Errors are unchanged for malformed text.
    assert_eq!(budget(br#"{"a":1,"a":2}"#, 100), Err(CanonError::DuplicateKey("a".into())));
    assert!(matches!(budget(b"[0,", 100), Err(CanonError::Syntax(_))));
}
