//! TRUST.REPLAY.COMPARE AC-H2: the comparator's value-field outcome and `MARGINAL` annotation
//! agree with the `fid-rep` classifier (`rdl.rep.classify@1.0.0`) and with the labels of
//! `fixtures/rdl/rep/cases.csv`, using table `rep-1@1.0.0`.
//!
//! Rows whose label is an error (`ERR:nan_input`, `ERR:kind_unknown`) are checked separately:
//! the comparator never errors on package data; it reports `DIVERGENT` with `NAN_INPUT`, and a
//! kind `fid-rep` does not know with `NO_TOLERANCE` (or `IDENTICAL` when bit-equal).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::kdec::Decimal;
use fid_rep::{classify, Annotation, Ctx, FieldKind};
use fid_vreplay::compare::{compare_result, Determinism, Rep1, ReplayContext, ResultFields, ResultPair};

const CASES: &str = include_str!("../../../fixtures/rdl/rep/cases.csv");

fn value(s: &str) -> Option<f64> {
    (s != "absent").then(|| f64::from_bits(u64::from_str_radix(s, 16).unwrap()))
}

fn fields(v: Option<f64>) -> ResultFields {
    ResultFields { value: v, uec: 0, expanded_uncertainty: None, decision: None, status: "MEASURED".into() }
}

#[test]
fn ac_h2_value_field_agrees_with_fid_rep_on_every_row() {
    let rep1 = Rep1::embedded().unwrap();
    let mut lines = CASES.lines();
    assert!(lines.next().unwrap().starts_with("name,kind,recorded,replayed"));
    let (mut checked, mut errors) = (0, 0);
    for line in lines {
        let f: Vec<&str> = line.split(',').collect();
        let det = Determinism::parse(f[5]).unwrap();
        let limit = (!f[7].is_empty()).then(|| Decimal::parse(f[7]).unwrap());
        let (rec, rep) = (value(f[2]), value(f[3]));
        let rc = ReplayContext { same_manifest: f[4] == "1", same_platform_class: f[6] == "1" };
        let pair = ResultPair {
            record_id: f[0].into(),
            determinism: det,
            quantity_kind: f[1].into(),
            limit,
            packaged: fields(rec),
            recomputed: fields(rep),
        };
        let got = compare_result(&rep1, &rc, &pair);
        if let Some(code) = f[8].strip_prefix("ERR:") {
            errors += 1;
            let reason = got.reason.as_ref().map(|r| r.to_wire());
            match code {
                "nan_input" => assert_eq!(reason.as_deref(), Some("NAN_INPUT"), "{}", f[0]),
                "kind_unknown" => assert!(
                    got.outcome.as_str() == "IDENTICAL" || reason.as_deref() == Some("NO_TOLERANCE"),
                    "{}: {got:?}",
                    f[0]
                ),
                other => panic!("{other}"),
            }
            continue;
        }
        let ctx = Ctx {
            same_manifest: rc.same_manifest,
            determinism: det,
            same_platform_class: rc.same_platform_class,
            limit,
        };
        let want = classify(FieldKind::parse(f[1]).unwrap(), rec, rep, &ctx).unwrap();
        assert_eq!(got.outcome, want.outcome, "{}", f[0]);
        assert_eq!(got.outcome.as_str(), f[8], "{}", f[0]);
        assert_eq!(got.marginal, want.annotations.contains(&Annotation::Marginal), "{}", f[0]);
        assert_eq!(got.marginal, f[9] == "MARGINAL", "{}", f[0]);
        checked += 1;
    }
    assert!(checked >= 100 && errors > 0, "{checked} rows, {errors} error rows");
}
