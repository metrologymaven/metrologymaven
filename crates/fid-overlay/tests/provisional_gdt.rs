//! AC-6 of spec node VIZ.OVERLAY.STATUS (D-045): a GD&T result whose stored record bytes make
//! `fid_gdt::requires_provisional_label_bytes` return true renders the provisional label and
//! cue; a signed-off record renders the signed-off line and no cue. The label constants are the
//! ones of `fid_gdt::chars::interp`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::interp::{PROVISIONAL_LABEL_KEY, PROVISIONAL_LABEL_TEXT, SIGNED_OFF_LINE};
use fid_gdt::requires_provisional_label_bytes;
use fid_overlay::status::{DisplayState, GdtResultItem, Interpretation, Palette, Provenance, Theme, PROVISIONAL_CUE};
use std::collections::BTreeMap;

/// Canonical bytes of a minimal `fid.gdt-result/2` interpretation block: the label and the
/// statuses of its rules.
fn record(label: &str, rules: &[&str]) -> Vec<u8> {
    let mut m = BTreeMap::new();
    m.insert("schema".to_string(), CValue::Str("fid.gdt-result/2".to_string()));
    m.insert("interpretation".to_string(), CValue::Str(label.to_string()));
    let rules = rules
        .iter()
        .map(|s| {
            let mut r = BTreeMap::new();
            r.insert("status".to_string(), CValue::Str((*s).to_string()));
            CValue::Obj(r)
        })
        .collect();
    m.insert("interpretation_rules".to_string(), CValue::Arr(rules));
    fid_canon::to_canonical_bytes(&CValue::Obj(m)).unwrap()
}

fn rendered(bytes: &[u8]) -> CValue {
    let i = Interpretation::from_requires_label(requires_provisional_label_bytes(bytes));
    let item = GdtResultItem::builder(DisplayState::Nonconforms, i)
        .number(0.042)
        .provenance(Provenance::Computed)
        .build()
        .unwrap();
    item.render(&Palette::locked(), Theme::Light).to_cvalue()
}

fn interp(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => match m.get("interpretation") {
            Some(CValue::Obj(i)) => i,
            other => panic!("{other:?}"),
        },
        _ => panic!(),
    }
}

#[test]
fn label_constants_are_fid_gdts() {
    assert_eq!(fid_overlay::status::PROVISIONAL_LABEL_KEY, PROVISIONAL_LABEL_KEY);
    assert_eq!(fid_overlay::status::PROVISIONAL_LABEL_TEXT, PROVISIONAL_LABEL_TEXT);
    assert_eq!(fid_overlay::status::SIGNED_OFF_LINE, SIGNED_OFF_LINE);
}

#[test]
fn ac6_provisional_records_show_the_label_and_cue() {
    let provisional = [
        record("provisional", &["provisional", "signed_off"]),
        record("signed_off", &["signed_off", "provisional"]),
        record("signed_off", &[]),
        b"not json".to_vec(),
        b"{ \"interpretation\": \"signed_off\" }".to_vec(),
    ];
    for bytes in &provisional {
        assert!(requires_provisional_label_bytes(bytes));
        let v = rendered(bytes);
        let i = interp(&v);
        assert_eq!(i.get("provisional"), Some(&CValue::Bool(true)));
        assert_eq!(i.get("label_key"), Some(&CValue::Str(PROVISIONAL_LABEL_KEY.to_string())));
        assert_eq!(i.get("text"), Some(&CValue::Str(PROVISIONAL_LABEL_TEXT.to_string())));
        assert_eq!(i.get("cue"), Some(&CValue::Str(PROVISIONAL_CUE.to_string())));
    }
    let signed = record("signed_off", &["signed_off", "signed_off"]);
    assert!(!requires_provisional_label_bytes(&signed));
    let v = rendered(&signed);
    let i = interp(&v);
    assert_eq!(i.get("provisional"), Some(&CValue::Bool(false)));
    assert_eq!(i.get("text"), Some(&CValue::Str(SIGNED_OFF_LINE.to_string())));
    assert_eq!(i.get("cue"), Some(&CValue::Null));
}
