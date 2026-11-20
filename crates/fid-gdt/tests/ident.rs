//! Acceptance criteria of spec node GDT.CHAR.IDENT (AC-1 .. AC-5) plus the error paths and QIF
//! mapping rules of the leaf's Design. The QIF fixtures, the imported QPId text and the
//! expected UUIDv7 values come from the independent stdlib-Python reference
//! `fixtures/gdt/ident/make_fixtures.py` (`expected.json`); error cases are hand-labelled
//! from the leaf's rules.
//!
//! QIF reading and writing belong to `IO.QIF` and the characteristic types to
//! `GDT.CHAR.MODEL`, neither built yet. The tests therefore read the identity fields of one
//! element with the project's XML reader (`fid-xml`), write them back with a small test-only
//! serialiser, and edit a test-only stand-in characteristic.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::ident::{
    assign_identity, assign_identity_from, check_edit, check_unchanged, identity_on_import, read_identity,
    write_identity, CharIdentity, Clock, CounterRng, FixedClock, IdSource, IdentError, QifIdentityView,
    QifStrAttribute, Qpid, QpidSlot, FX_QPID, MAX_UNIX_MS,
};
use fid_io_reg::{Budget, LimitRequest, Principal};
use fid_project::{uuid_v7, Bits, Observation, ReplaySource, Uuid};
use fid_xml::{read_slice, EventKind};
use std::path::{Path, PathBuf};

const ITEM: &str = "FlatnessCharacteristicItem";
const NOMINAL: &str = "FlatnessCharacteristicNominal";

fn path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/ident").join(name)
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(path(name)).unwrap()
}

fn expected() -> CValue {
    fid_canon::parse(&read("expected.json")).unwrap()
}

fn get<'a>(v: &'a CValue, key: &str) -> &'a CValue {
    let CValue::Obj(o) = v else { panic!("object for {key}") };
    o.get(key).unwrap_or_else(|| panic!("missing {key}"))
}

fn s<'a>(v: &'a CValue, key: &str) -> &'a str {
    let CValue::Str(t) = get(v, key) else { panic!("string {key}") };
    t
}

fn n(v: &CValue, key: &str) -> u64 {
    s(v, key).parse().unwrap()
}

fn budget() -> Budget {
    Budget::new(Principal::Agent, &LimitRequest::default()).unwrap()
}

/// The identity fields of the first element named `local`: the text of its direct `QPId`
/// child and the `AttributeStr` entries of its direct `Attributes` child.
fn view_of(xml: &[u8], local: &str) -> QifIdentityView {
    let events = read_slice(xml, budget()).unwrap();
    let mut stack: Vec<String> = Vec::new();
    let mut inside: Option<usize> = None;
    let mut found = false;
    let mut view = QifIdentityView::default();
    for ev in events {
        match ev.kind {
            EventKind::Start(e) => {
                let name = e.name.local.clone();
                if inside.is_none() && !found && name == local {
                    inside = Some(stack.len());
                    found = true;
                }
                if let Some(base) = inside {
                    let rel: Vec<&str> = stack[base..].iter().map(String::as_str).collect();
                    if name == "AttributeStr" && rel == [local, "Attributes"] {
                        let attr = |k: &str| {
                            e.attrs.iter().find(|a| a.name.local == k).map(|a| a.value.raw().to_owned()).unwrap()
                        };
                        view.attributes.push(QifStrAttribute { name: attr("name"), value: attr("val") });
                    }
                }
                stack.push(name);
            }
            EventKind::End { .. } => {
                stack.pop();
                if inside == Some(stack.len()) {
                    inside = None;
                }
            }
            EventKind::Text(t) => {
                if let Some(base) = inside {
                    let rel: Vec<&str> = stack[base..].iter().map(String::as_str).collect();
                    if rel == [local, "QPId"] {
                        view.native_qpid = Some(view.native_qpid.take().unwrap_or_default() + t.raw());
                    }
                }
            }
            _ => {}
        }
    }
    assert!(found, "no element {local}");
    view
}

fn esc(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Test-only export of one element's identity fields (QIF writing is IO.QIF's job).
fn to_xml(local: &str, view: &QifIdentityView) -> String {
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str(&format!("<{local} xmlns=\"http://qifstandards.org/xsd/qif3\" id=\"9\">"));
    if let Some(q) = &view.native_qpid {
        x.push_str(&format!("<QPId>{}</QPId>", esc(q)));
    }
    if !view.attributes.is_empty() {
        x.push_str(&format!("<Attributes n=\"{}\">", view.attributes.len()));
        for a in &view.attributes {
            x.push_str(&format!("<AttributeStr name=\"{}\" val=\"{}\"/>", esc(&a.name), esc(&a.value)));
        }
        x.push_str("</Attributes>");
    }
    x.push_str(&format!("</{local}>\n"));
    x
}

/// A stand-in characteristic: an identity plus one editable field.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Characteristic {
    id: CharIdentity,
    tolerance: String,
}

struct Edit<'a> {
    identity: Option<&'a str>,
    tolerance: &'a str,
}

/// Applies an edit the way the edit command must: identity check first, then the change.
fn apply(c: &mut Characteristic, e: &Edit) -> Result<(), IdentError> {
    check_edit(&c.id, e.identity)?;
    c.tolerance = e.tolerance.to_owned();
    Ok(())
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_imported_qpid_survives_edit_and_export_byte_for_byte() {
    let exp = expected();
    let want = s(&exp, "imported_qpid");
    let src = read("imported_qpid.qif");
    assert!(String::from_utf8(src.clone()).unwrap().contains(&format!("<QPId>{want}</QPId>")));

    let view = view_of(&src, ITEM);
    let mut rng = CounterRng::new(1);
    let id = identity_on_import(QpidSlot::Native, &view, &FixedClock(1), &mut rng).unwrap();
    assert_eq!(id.source(), IdSource::Imported);
    assert_eq!(id.as_str(), want);
    assert_eq!(rng.counter(), 0, "an imported QPId draws nothing");

    let mut c = Characteristic { id, tolerance: "0.05".into() };
    apply(&mut c, &Edit { identity: None, tolerance: "0.10" }).unwrap();
    assert_eq!(c.tolerance, "0.10");

    let mut out = view.clone();
    write_identity(QpidSlot::Native, &c.id, &mut out);
    let xml = to_xml(ITEM, &out);
    assert!(xml.contains(&format!("<QPId>{want}</QPId>")), "{xml}");
    let back = view_of(xml.as_bytes(), ITEM);
    assert_eq!(back.native_qpid.as_deref().unwrap().as_bytes(), want.as_bytes());
    let again = identity_on_import(QpidSlot::Native, &back, &FixedClock(1), &mut rng).unwrap();
    assert_eq!(again, c.id);
}

// ------------------------------------------------------------------ AC-2

fn ac2_round(slot: QpidSlot) {
    let clock = FixedClock(1_763_500_000_123);
    let mut rng = CounterRng::new(99);
    let id = assign_identity(None, &clock, &mut rng).unwrap();
    assert_eq!(id.source(), IdSource::Assigned);
    assert_eq!(id.uuid().version(), 7);
    assert_eq!(id.uuid().variant_bits(), 0b10);
    assert_eq!(rng.counter(), 2);
    let first = id.clone();

    let mut c = Characteristic { id, tolerance: "0.05".into() };
    let same = first.as_str().to_uppercase();
    let edits = [
        Edit { identity: None, tolerance: "0.06" },
        Edit { identity: Some(first.as_str()), tolerance: "0.07" },
        Edit { identity: Some(same.as_str()), tolerance: "0.08" },
    ];
    for e in &edits {
        apply(&mut c, e).unwrap();
        assert_eq!(c.id, first, "edit changed the identity");
    }
    assert_eq!(c.tolerance, "0.08");
    assert_eq!(rng.counter(), 2, "edits draw nothing");

    let mut out = QifIdentityView::default();
    write_identity(slot, &c.id, &mut out);
    let xml = to_xml(ITEM, &out);
    let back = view_of(xml.as_bytes(), ITEM);
    let mut other_rng = CounterRng::new(12345);
    let re = identity_on_import(slot, &back, &FixedClock(5), &mut other_rng).unwrap();
    assert_eq!(re.source(), IdSource::Imported);
    assert!(re.same_identity(&first));
    assert_eq!(re.as_str(), first.as_str());
    assert_eq!(re.uuid().version(), 7);
    assert_eq!(other_rng.counter(), 0);
    check_unchanged(&first, &re).unwrap();
}

#[test]
fn ac2_assigned_uuidv7_survives_three_edits_and_round_trip_native() {
    ac2_round(QpidSlot::Native);
}

#[test]
fn ac2_assigned_uuidv7_survives_three_edits_and_round_trip_fx() {
    ac2_round(QpidSlot::FxAttribute);
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_no_slot_element_gets_fx_qpid_and_round_trips() {
    let exp = expected();
    let ns = get(&exp, "no_slot");
    let kept = get(ns, "kept_attribute");
    let view = view_of(&read("no_qpid_slot.qif"), NOMINAL);
    assert_eq!(view.native_qpid, None);
    assert_eq!(view.attributes, vec![QifStrAttribute { name: s(kept, "name").into(), value: s(kept, "value").into() }]);
    assert_eq!(read_identity(QpidSlot::FxAttribute, &view).unwrap(), None);

    let mut rng = CounterRng::new(n(ns, "seed"));
    let id = identity_on_import(QpidSlot::FxAttribute, &view, &FixedClock(n(ns, "clock_ms")), &mut rng).unwrap();
    assert_eq!(id.source(), IdSource::Assigned);
    assert_eq!(id.as_str(), s(ns, "identity"));

    let mut out = view.clone();
    write_identity(QpidSlot::FxAttribute, &id, &mut out);
    assert_eq!(out.native_qpid, None);
    assert_eq!(out.attributes.len(), 2);
    assert_eq!(out.attributes[0], view.attributes[0], "other attributes kept, in order");
    assert_eq!(out.attributes[1], QifStrAttribute { name: FX_QPID.into(), value: s(ns, "identity").into() });

    let xml = to_xml(NOMINAL, &out);
    assert!(xml.contains(&format!("<AttributeStr name=\"fx:qpid\" val=\"{}\"/>", s(ns, "identity"))), "{xml}");
    let back = view_of(xml.as_bytes(), NOMINAL);
    let mut fresh = CounterRng::new(0);
    let re = identity_on_import(QpidSlot::FxAttribute, &back, &FixedClock(0), &mut fresh).unwrap();
    assert!(re.same_identity(&id));
    assert_eq!(re.as_str(), id.as_str());
    assert_eq!(fresh.counter(), 0);

    // A second export of the re-imported identity replaces the attribute, never duplicates it.
    let mut twice = back.clone();
    write_identity(QpidSlot::FxAttribute, &re, &mut twice);
    assert_eq!(twice, back);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_edit_with_other_identity_fails_and_leaves_characteristic_unchanged() {
    let mut rng = CounterRng::new(3);
    let id = assign_identity(None, &FixedClock(1_700_000_000_000), &mut rng).unwrap();
    let mut c = Characteristic { id, tolerance: "0.05".into() };
    let before = c.clone();

    let other = assign_identity(None, &FixedClock(1_700_000_000_000), &mut rng).unwrap();
    let err = apply(&mut c, &Edit { identity: Some(other.as_str()), tolerance: "0.50" }).unwrap_err();
    assert_eq!(err.code(), "identity_immutable");
    assert_eq!(err, IdentError::IdentityImmutable { current: before.id.uuid(), attempted: other.uuid() });
    assert_eq!(c, before);

    let err = apply(&mut c, &Edit { identity: Some("not-a-uuid"), tolerance: "0.50" }).unwrap_err();
    assert_eq!(err.code(), "malformed_qpid");
    assert_eq!(c, before);

    let err = check_unchanged(&before.id, &other).unwrap_err();
    assert_eq!(err.code(), "identity_immutable");
    check_unchanged(&before.id, &before.id).unwrap();
}

// ------------------------------------------------------------------ AC-5

fn draw(ms: u64, seed: u64, k: usize) -> Vec<String> {
    let mut rng = CounterRng::new(seed);
    (0..k).map(|_| assign_identity(None, &FixedClock(ms), &mut rng).unwrap().as_str().to_owned()).collect()
}

#[test]
fn ac5_same_clock_and_seed_give_the_reference_uuids() {
    let exp = expected();
    let CValue::Arr(cases) = get(&exp, "v7_cases") else { panic!("v7_cases") };
    assert!(cases.len() >= 5);
    for case in cases {
        let CValue::Arr(want) = get(case, "uuids") else { panic!("uuids") };
        let want: Vec<String> = want
            .iter()
            .map(|u| match u {
                CValue::Str(t) => t.clone(),
                _ => panic!("uuid"),
            })
            .collect();
        let (ms, seed) = (n(case, "clock_ms"), n(case, "seed"));
        let got = draw(ms, seed, want.len());
        assert_eq!(got, want, "case {}", s(case, "name"));
        assert_eq!(draw(ms, seed, want.len()), got, "repeat of case {}", s(case, "name"));
    }
}

#[test]
fn counter_rng_matches_splitmix64_reference_value() {
    // Published SplitMix64 reference: seed 1234567, first output.
    assert_eq!(CounterRng::new(1_234_567).next_u64(), 6_457_827_717_110_365_317);
}

// ------------------------------------------------------------------ errors and rules

#[test]
fn clock_past_48_bits_fails_before_drawing() {
    let mut rng = CounterRng::new(1);
    assert_eq!(assign_identity(None, &FixedClock(MAX_UNIX_MS + 1), &mut rng), Err(IdentError::BadObservation));
    assert_eq!(rng.counter(), 0);
    assert!(assign_identity(None, &FixedClock(MAX_UNIX_MS), &mut rng).is_ok());
    assert_eq!(IdentError::BadObservation.code(), "bad_observation");
}

#[test]
fn qpid_parse_rules() {
    let q = Qpid::parse(" \t4F2A9C3E-1B7D-4E8A-9C21-7D3B5E6F8A10\r\n").unwrap();
    assert_eq!(q.as_str(), "4F2A9C3E-1B7D-4E8A-9C21-7D3B5E6F8A10");
    assert_eq!(q.uuid(), Uuid::parse("4f2a9c3e-1b7d-4e8a-9c21-7d3b5e6f8a10").unwrap());
    let reason = |t: &str| match Qpid::parse(t) {
        Err(IdentError::MalformedQpid { reason }) => reason,
        other => panic!("{t:?}: {other:?}"),
    };
    assert_eq!(reason(""), "not_uuid");
    assert_eq!(reason("4f2a9c3e1b7d4e8a9c217d3b5e6f8a10"), "not_uuid");
    assert_eq!(reason("4f2a9c3e-1b7d-4e8a-9c21-7d3b5e6f8a1g"), "not_uuid");
    assert_eq!(reason("4f2a9c3e-1b7d-4e8a-9c21-7d3b5e6f8a10x"), "not_uuid");
    assert_eq!(reason("4f2a9c3e-1b7d-4e8a-9c21 7d3b5e6f8a10"), "not_uuid");
    assert_eq!(reason("\u{a0}4f2a9c3e-1b7d-4e8a-9c21-7d3b5e6f8a10"), "not_uuid");
    assert_eq!(reason("00000000-0000-0000-0000-000000000000"), "reserved_uuid");
    assert_eq!(reason("FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF"), "reserved_uuid");
    assert_eq!(Qpid::parse("x").unwrap_err().code(), "malformed_qpid");
}

fn view(native: Option<&str>, attrs: &[(&str, &str)]) -> QifIdentityView {
    QifIdentityView {
        native_qpid: native.map(str::to_owned),
        attributes: attrs.iter().map(|(n, v)| QifStrAttribute { name: (*n).into(), value: (*v).into() }).collect(),
    }
}

const U1: &str = "018bcfe5-6800-7a54-a8ef-e333b266f103";
const U2: &str = "018bcfe5-6800-7d49-981c-e1ff0e4ae394";

#[test]
fn read_identity_rules() {
    let reason = |slot, v: &QifIdentityView| match read_identity(slot, v) {
        Err(IdentError::MalformedQpid { reason }) => reason,
        other => panic!("{other:?}"),
    };
    // Native slot.
    assert_eq!(read_identity(QpidSlot::Native, &view(None, &[])).unwrap(), None);
    assert_eq!(read_identity(QpidSlot::Native, &view(Some(U1), &[])).unwrap().unwrap().as_str(), U1);
    assert_eq!(read_identity(QpidSlot::Native, &view(None, &[(FX_QPID, U1)])).unwrap().unwrap().as_str(), U1);
    let upper = U1.to_uppercase();
    assert_eq!(
        read_identity(QpidSlot::Native, &view(Some(&upper), &[(FX_QPID, U1)])).unwrap().unwrap().as_str(),
        upper
    );
    assert_eq!(reason(QpidSlot::Native, &view(Some(U1), &[(FX_QPID, U2)])), "conflicting_qpid");
    assert_eq!(reason(QpidSlot::Native, &view(Some("bad"), &[])), "not_uuid");
    // No native slot.
    assert_eq!(read_identity(QpidSlot::FxAttribute, &view(None, &[("fx:note", "x")])).unwrap(), None);
    assert_eq!(
        read_identity(QpidSlot::FxAttribute, &view(None, &[("fx:note", "x"), (FX_QPID, U2)]))
            .unwrap()
            .unwrap()
            .as_str(),
        U2
    );
    assert_eq!(reason(QpidSlot::FxAttribute, &view(Some(U1), &[])), "qpid_without_slot");
    assert_eq!(reason(QpidSlot::FxAttribute, &view(None, &[(FX_QPID, U1), (FX_QPID, U1)])), "duplicate_fx_qpid");
    assert_eq!(reason(QpidSlot::Native, &view(Some(U1), &[(FX_QPID, U1), (FX_QPID, U1)])), "duplicate_fx_qpid");
    assert_eq!(reason(QpidSlot::FxAttribute, &view(None, &[(FX_QPID, "nope")])), "not_uuid");
    // Attribute names are matched exactly.
    assert_eq!(read_identity(QpidSlot::FxAttribute, &view(None, &[("FX:QPID", U1), ("qpid", U1)])).unwrap(), None);
}

#[test]
fn write_identity_rules() {
    let id =
        identity_on_import(QpidSlot::Native, &view(Some(U1), &[]), &FixedClock(0), &mut CounterRng::new(0)).unwrap();
    // Native: sets the slot and drops fx:qpid attributes, keeping the rest in order.
    let mut v = view(Some(U2), &[("a", "1"), (FX_QPID, U2), ("b", "2")]);
    write_identity(QpidSlot::Native, &id, &mut v);
    assert_eq!(v, view(Some(U1), &[("a", "1"), ("b", "2")]));
    // FxAttribute: replaces the first fx:qpid in place, drops the others, clears the slot.
    let mut v = view(Some(U2), &[("a", "1"), (FX_QPID, U2), ("b", "2"), (FX_QPID, U2)]);
    write_identity(QpidSlot::FxAttribute, &id, &mut v);
    assert_eq!(v, view(None, &[("a", "1"), (FX_QPID, U1), ("b", "2")]));
    // FxAttribute without one: appended at the end.
    let mut v = view(None, &[("a", "1")]);
    write_identity(QpidSlot::FxAttribute, &id, &mut v);
    assert_eq!(v, view(None, &[("a", "1"), (FX_QPID, U1)]));
}

#[test]
fn observation_source_assignment_matches_uuid_v7() {
    let bits = Bits::new(74, (0x2a5u128 << 64) | 0x0123_4567_89ab_cdef).unwrap();
    let mut src = ReplaySource::new(vec![Observation::NowMs(1_700_000_000_000), Observation::RandomBits(bits)]);
    let id = assign_identity_from(None, &mut src).unwrap();
    assert_eq!(id.uuid(), uuid_v7(1_700_000_000_000, &bits).unwrap());
    assert_eq!(id.source(), IdSource::Assigned);
    assert_eq!(src.remaining(), 0);
    // Exhausted source: typed error, no identity.
    assert_eq!(assign_identity_from(None, &mut src), Err(IdentError::BadObservation));
    // Imported QPId: the source is not touched.
    let mut empty = ReplaySource::new(vec![]);
    let q = Qpid::parse(U1).unwrap();
    assert_eq!(assign_identity_from(Some(q), &mut empty).unwrap().source(), IdSource::Imported);
}

#[test]
fn identity_maps_to_char_id_and_displays_its_text() {
    let mut rng = CounterRng::new(8);
    let id = assign_identity(None, &FixedClock(1), &mut rng).unwrap();
    assert_eq!(id.to_char_id().unwrap().as_str(), id.as_str());
    assert_eq!(id.to_string(), id.as_str());
    assert_eq!(id.as_str(), id.uuid().to_string(), "assigned identities are lowercase");
    let imported = assign_identity(Some(Qpid::parse(&U1.to_uppercase()).unwrap()), &FixedClock(1), &mut rng).unwrap();
    assert_eq!(imported.to_char_id().unwrap().as_str(), U1.to_uppercase());
    assert_eq!(imported.qpid().to_string(), U1.to_uppercase());
}

#[test]
fn clock_trait_is_object_safe_and_injectable() {
    struct Ticking(std::cell::Cell<u64>);
    impl Clock for Ticking {
        fn now_ms(&self) -> u64 {
            let t = self.0.get();
            self.0.set(t + 1);
            t
        }
    }
    let clock = Ticking(std::cell::Cell::new(10));
    let mut rng = CounterRng::new(0);
    let a = assign_identity(None, &clock, &mut rng).unwrap();
    let b = assign_identity(None, &clock, &mut rng).unwrap();
    assert!(!a.same_identity(&b));
    assert!(a.uuid() < b.uuid(), "later millisecond sorts later");
}

#[test]
fn error_display_and_codes_are_stable() {
    let u = Uuid::parse(U1).unwrap();
    let v = Uuid::parse(U2).unwrap();
    let e = IdentError::IdentityImmutable { current: u, attempted: v };
    assert_eq!(e.code(), "identity_immutable");
    assert_eq!(e.to_string(), format!("identity_immutable: {U1} cannot become {U2}"));
    assert_eq!(IdentError::MalformedQpid { reason: "not_uuid" }.to_string(), "malformed_qpid: not_uuid");
}
