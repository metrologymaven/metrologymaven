//! Acceptance tests for `RPT.FORMS.HDR` (`AC-1`..`AC-8`, `REQ-001`..`REQ-008`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::dprof::CharId;
use fid_project::{uuid_v7, Bits, IdError, ObservationSource};
use fid_replay::ReplayOutcome;
use fid_report_forms::hdr::{
    cleanroom_phrase_digest, command_defs, replay_exact, AddFaiItem, FaiError, FaiEvent, FaiStore, FaiType, FieldId,
    Form2Kind, PartCharLookup, PartRevisionId, SetHeaderParams, HEADER_FIELDS,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;

struct Counter(u64);

impl ObservationSource for Counter {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        Ok(1_760_000_000_000 + self.0)
    }

    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        let b = Bits::new(n, u128::from(self.0) + 1)?;
        self.0 += 1;
        Ok(b)
    }
}

/// Fixture `fixtures/rpt/forms/part_bracket`: part revision B with 12 characteristics `C1`..`C12`.
struct BracketChars {
    part_rev: PartRevisionId,
    chars: BTreeSet<String>,
}

impl PartCharLookup for BracketChars {
    fn has_characteristic(&self, part_rev: PartRevisionId, char_id: &CharId) -> bool {
        part_rev == self.part_rev && self.chars.contains(char_id.as_str())
    }
}

fn make_part_rev(obs: &mut Counter) -> PartRevisionId {
    let (ms, r) = obs.draw_v7().unwrap();
    PartRevisionId::new(uuid_v7(ms, &r).unwrap()).unwrap()
}

fn c(name: &str) -> CharId {
    CharId::parse(name).unwrap()
}

#[test]
fn ac1_to_ac8_full_fai_header_and_accountability_lifecycle() {
    let mut obs = Counter(1);
    let part_rev_b = make_part_rev(&mut obs);
    let part_rev_sub = make_part_rev(&mut obs);
    let chars = BracketChars { part_rev: part_rev_b, chars: (1..=12).map(|i| format!("C{i}")).collect() };
    let mut store = FaiStore::new();

    // AC-1: create_fai -> UUIDv7 fai_id, FULL, empty header fields, 1 journal event.
    let s_a = store.create_fai(part_rev_b, &mut obs).expect("AC-1 create FAI A");
    assert_eq!(s_a.fai_type, FaiType::Full);
    assert_eq!(s_a.part_rev, part_rev_b);
    assert_eq!(store.journal().len(), 1);
    let rec_a = store.show_fai(s_a.fai_id).unwrap();
    assert!(rec_a.header.fields.is_empty());

    // AC-2: set_header_fai PARTIAL with reason and scope [C3, C7].
    let mut fields = BTreeMap::new();
    fields.insert(FieldId::parse("part_number").unwrap(), Some("BRK-4200".into()));
    fields.insert(FieldId::parse("organisation").unwrap(), Some("Acme Aero".into()));
    let hdr = store
        .set_header_fai(
            s_a.fai_id,
            &SetHeaderParams {
                fields,
                fai_type: Some(FaiType::Partial),
                reason: Some(Some("Drawing change ECN 41".into())),
                scope: Some(vec![c("C3"), c("C7")]),
            },
            &chars,
        )
        .expect("AC-2 set partial header");
    assert_eq!(hdr.fai_type, FaiType::Partial);
    assert_eq!(hdr.reason.as_ref().unwrap().untrusted_value().as_str(), "Drawing change ECN 41");
    assert_eq!(hdr.scope, vec![c("C3"), c("C7")]);
    // Untrusted JSON shape confirmed.
    let show_val = store.show_fai(s_a.fai_id).unwrap().to_value();
    let CValue::Obj(show_map) = &show_val else { panic!("show must be object") };
    let CValue::Obj(hdr_map) = show_map.get("header").unwrap() else { panic!("header object") };
    let CValue::Obj(reason_map) = hdr_map.get("reason").unwrap() else { panic!("reason untrusted object") };
    assert_eq!(reason_map.get("untrusted"), Some(&CValue::Bool(true)));

    // AC-3: PARTIAL without reason or with unknown C99 fails with report.fai_partial_incomplete and journals nothing.
    let s_b = store.create_fai(part_rev_sub, &mut obs).expect("create FAI B");
    let jlen = store.journal().len();
    let e_noreason = store
        .set_header_fai(
            s_b.fai_id,
            &SetHeaderParams {
                fields: BTreeMap::new(),
                fai_type: Some(FaiType::Partial),
                reason: None,
                scope: Some(vec![c("C3")]),
            },
            &chars,
        )
        .unwrap_err();
    assert_eq!(e_noreason.code(), "report.fai_partial_incomplete");

    let e_badchar = store
        .set_header_fai(
            s_a.fai_id,
            &SetHeaderParams {
                fields: BTreeMap::new(),
                fai_type: Some(FaiType::Partial),
                reason: Some(Some("Change".into())),
                scope: Some(vec![c("C99")]),
            },
            &chars,
        )
        .unwrap_err();
    assert_eq!(e_badchar.code(), "report.fai_partial_incomplete");
    assert_eq!(store.journal().len(), jlen);

    // AC-4: Add 2 material + 1 special_process Form 2 items and 1 Form 1 item referencing B -> insertion order, distinct UUIDv7 IDs, removed ID retired forever.
    let m1 = store
        .add_item_fai(
            s_a.fai_id,
            AddFaiItem::Form2 {
                kind: Form2Kind::Material,
                spec_ref: "AMS 4027".into(),
                supplier: "AluCorp".into(),
                cert_ref: "MTR-1001".into(),
                note: None,
            },
            None,
            &mut obs,
        )
        .expect("add material 1");
    let m2 = store
        .add_item_fai(
            s_a.fai_id,
            AddFaiItem::Form2 {
                kind: Form2Kind::Material,
                spec_ref: "AMS 5643".into(),
                supplier: "SteelCo".into(),
                cert_ref: "MTR-1002".into(),
                note: Some("lot 42".into()),
            },
            None,
            &mut obs,
        )
        .expect("add material 2");
    let sp1 = store
        .add_item_fai(
            s_a.fai_id,
            AddFaiItem::Form2 {
                kind: Form2Kind::SpecialProcess,
                spec_ref: "MIL-A-8625".into(),
                supplier: "Anodize Ltd".into(),
                cert_ref: "NADCAP-77".into(),
                note: None,
            },
            None,
            &mut obs,
        )
        .expect("add special process");
    let f1 = store
        .add_item_fai(
            s_a.fai_id,
            AddFaiItem::Form1 { fai_ref: s_b.fai_id, note: Some("sub-bracket".into()) },
            None,
            &mut obs,
        )
        .expect("add form1 ref A -> B");

    let shown = store.show_fai(s_a.fai_id).unwrap();
    let f2_ids: Vec<_> = shown.form2.iter().map(|e| e.id).collect();
    assert_eq!(f2_ids, vec![m1, m2, sp1]);
    assert_eq!(shown.form1.len(), 1);
    assert_eq!(shown.form1[0].id, f1);
    assert_eq!(shown.form1[0].fai_ref, s_b.fai_id);

    // Remove m2 and add another item -> m2 retired and never reused.
    store.remove_item_fai(s_a.fai_id, m2).expect("remove m2");
    let ft1 = store
        .add_item_fai(
            s_a.fai_id,
            AddFaiItem::Form2 {
                kind: Form2Kind::FunctionalTest,
                spec_ref: "ATP-09".into(),
                supplier: "In-House Lab".into(),
                cert_ref: "FT-2026-09".into(),
                note: None,
            },
            None,
            &mut obs,
        )
        .expect("add functional test");
    assert_ne!(ft1, m2);
    let shown2 = store.show_fai(s_a.fai_id).unwrap();
    assert!(shown2.retired_ids.contains(&m2.uuid()));
    assert_eq!(shown2.form2.iter().map(|e| e.id).collect::<Vec<_>>(), vec![m1, sp1, ft1]);

    // AC-5: A references B; B adding reference to A fails with report.fai_ref_invalid; unknown ref also fails.
    let jlen_cyc = store.journal().len();
    let e_cyc = store
        .add_item_fai(s_b.fai_id, AddFaiItem::Form1 { fai_ref: s_a.fai_id, note: None }, None, &mut obs)
        .unwrap_err();
    assert_eq!(e_cyc.code(), "report.fai_ref_invalid");
    assert!(matches!(e_cyc, FaiError::RefInvalid(_)));
    assert_eq!(store.journal().len(), jlen_cyc);

    // AC-6: Hostile strings (<= 512 bytes accepted as untrusted: true; > 512 bytes or NUL rejected with report.text_invalid).
    let hostile_ok = "<b>Acme</b>\u{202E}=cmd|' /C calc'!A0\n\t\"quote\"";
    let h_id = store
        .add_item_fai(
            s_b.fai_id,
            AddFaiItem::Form2 {
                kind: Form2Kind::Material,
                spec_ref: "SPEC".into(),
                supplier: hostile_ok.into(),
                cert_ref: "CERT".into(),
                note: None,
            },
            None,
            &mut obs,
        )
        .expect("hostile <= 512 bytes accepted");
    let rec_b = store.show_fai(s_b.fai_id).unwrap();
    let h_entry = rec_b.form2.iter().find(|e| e.id == h_id).unwrap();
    let CValue::Obj(sup_obj) = h_entry.supplier.to_value() else { panic!("untrusted obj") };
    assert_eq!(sup_obj.get("untrusted"), Some(&CValue::Bool(true)));

    let too_long = "x".repeat(513);
    let e_long = store
        .add_item_fai(
            s_b.fai_id,
            AddFaiItem::Form2 {
                kind: Form2Kind::Material,
                spec_ref: "SPEC".into(),
                supplier: too_long,
                cert_ref: "CERT".into(),
                note: None,
            },
            None,
            &mut obs,
        )
        .unwrap_err();
    assert_eq!(e_long.code(), "report.text_invalid");

    let e_nul = store
        .add_item_fai(
            s_b.fai_id,
            AddFaiItem::Form2 {
                kind: Form2Kind::Material,
                spec_ref: "SPEC".into(),
                supplier: "bad\0supplier".into(),
                cert_ref: "CERT".into(),
                note: None,
            },
            None,
            &mut obs,
        )
        .unwrap_err();
    assert_eq!(e_nul.code(), "report.text_invalid");

    // AC-7: Unique header field IDs, own-words labels, required flags, and cleanroom_phrases digest check.
    let fix_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/forms/cleanroom_phrases.json");
    let fix_bytes = std::fs::read(&fix_path).expect("read cleanroom_phrases.json");
    let fix_val = fid_canon::parse_with_limits(&fix_bytes, 64 * 1024, fid_canon::MAX_DEPTH).unwrap();
    let CValue::Obj(fm) = fix_val else { panic!("fixture object") };
    let CValue::Arr(forb) = fm.get("forbidden_digests").unwrap() else { panic!("forbidden array") };
    let forbidden: BTreeSet<String> =
        forb.iter().filter_map(|v| if let CValue::Str(s) = v { Some(s.clone()) } else { None }).collect();
    let mut seen_ids = BTreeSet::new();
    for f in HEADER_FIELDS {
        assert!(seen_ids.insert(f.id), "duplicate field id {}", f.id);
        assert!(!f.label.is_empty());
        let d = cleanroom_phrase_digest(f.label);
        assert!(!forbidden.contains(d.as_str()), "label {:?} matched forbidden digest", f.label);
    }

    // AC-8: Canonical event roundtrip and exact replay -> IDENTICAL with exact same IDs!
    let roundtripped: Vec<FaiEvent> =
        store.journal().iter().map(|e| FaiEvent::from_value(&e.to_value()).expect("event roundtrip")).collect();
    assert_eq!(roundtripped, store.journal());
    let run = replay_exact(&roundtripped);
    assert_eq!(run.outcome, ReplayOutcome::Identical, "mismatch at {:?}", run.first_mismatch);
    assert_eq!(run.store.show_fai(s_a.fai_id).unwrap(), store.show_fai(s_a.fai_id).unwrap());
    assert_eq!(run.store.show_fai(s_b.fai_id).unwrap(), store.show_fai(s_b.fai_id).unwrap());

    // Validated CommandDefs.
    let defs = command_defs().expect("valid CommandDefs");
    assert_eq!(defs.len(), 5);

    // Performance budget: record with 1000 items.
    let s_perf = store.create_fai(part_rev_b, &mut obs).unwrap();
    for i in 0..1_000u32 {
        store
            .add_item_fai(
                s_perf.fai_id,
                AddFaiItem::Form2 {
                    kind: Form2Kind::Material,
                    spec_ref: format!("SPEC-{i}"),
                    supplier: "Sup".into(),
                    cert_ref: format!("CERT-{i}"),
                    note: None,
                },
                None,
                &mut obs,
            )
            .unwrap();
    }
    let t0 = Instant::now();
    let _ = store.show_fai(s_perf.fai_id).unwrap();
    assert!(t0.elapsed().as_millis() < 50);
}
