//! Acceptance criteria of spec node CORE.PRJ.CONT.ID (AC-1 .. AC-4), with fixtures from
//! `fixtures/core/prj/cont/id/make_fixtures.py` (independent Python reference).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_project::{
    live_source, uuid_v5, uuid_v7, Bits, IdError, IdService, Observation, ObservationSource, Recording, ReplaySource,
    Uuid, V7_RANDOM_BITS,
};
use std::collections::{BTreeMap, BTreeSet};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/prj/cont/id/");

fn load(name: &str) -> CValue {
    let bytes = std::fs::read(format!("{DIR}{name}")).unwrap();
    parse(&bytes).unwrap()
}
fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}
fn arr(v: &CValue) -> &Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        other => panic!("expected array, got {other:?}"),
    }
}
fn st<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: expected string, got {other:?}"),
    }
}

/// Fake clock and CSPRNG with fixed values; counts every call.
struct Fake {
    steps: Vec<(u64, u128)>,
    clock_calls: usize,
    rng_calls: usize,
}

impl ObservationSource for Fake {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        let t = self.steps[self.clock_calls].0;
        self.clock_calls += 1;
        Ok(t)
    }
    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        assert_eq!(n, 74);
        let r = self.steps[self.rng_calls].1;
        self.rng_calls += 1;
        Bits::new(n, r)
    }
}

/// (now_ms, random74, expected) from v7.json.
fn v7_steps() -> Vec<(u64, u128, String)> {
    let doc = load("v7.json");
    arr(&obj(&doc)["steps"])
        .iter()
        .map(|s| {
            let s = obj(s);
            let r = st(s, "random74").strip_prefix("0x").unwrap();
            (st(s, "now_ms").parse().unwrap(), u128::from_str_radix(r, 16).unwrap(), st(s, "expected").to_string())
        })
        .collect()
}

fn ns() -> Uuid {
    Uuid::parse("0193a6f2-7c3e-7b2a-9d41-5e2f8c1a0b77").unwrap()
}

/// Runs AC-1 and returns the recorded observations and the IDs.
fn run_ac1() -> (Vec<Observation>, Vec<Uuid>, usize, usize) {
    let steps = v7_steps();
    assert!(steps.len() >= 8);
    let mut src =
        Recording::new(Fake { steps: steps.iter().map(|(t, r, _)| (*t, *r)).collect(), clock_calls: 0, rng_calls: 0 });
    let existing = BTreeSet::new();
    let mut ids = Vec::new();
    {
        let mut svc = IdService::new(ns(), &mut src, &existing);
        for (_, _, expected) in &steps {
            let id = svc.new_v7().unwrap();
            assert_eq!(id.to_string(), *expected);
            assert_eq!(id, Uuid::parse(expected).unwrap());
            assert_eq!(id.version(), 7);
            assert_eq!(id.variant_bits(), 0b10);
            ids.push(id);
        }
    }
    let obs = src.observations().to_vec();
    assert_eq!(obs.len(), 2 * steps.len());
    for (i, (t, r, _)) in steps.iter().enumerate() {
        assert_eq!(obs[2 * i], Observation::NowMs(*t));
        assert_eq!(obs[2 * i + 1], Observation::RandomBits(Bits::new(V7_RANDOM_BITS, *r).unwrap()));
    }
    (obs, ids, src.inner().clock_calls, src.inner().rng_calls)
}

#[test]
fn ac1_new_v7_matches_fixture_and_records_observations() {
    let (_, ids, clock_calls, rng_calls) = run_ac1();
    assert_eq!(clock_calls, ids.len());
    assert_eq!(rng_calls, ids.len());
}

#[test]
fn ac2_replay_returns_same_ids_without_drawing() {
    let (obs, ids, _, _) = run_ac1();
    // Replay mode holds only the recorded values: it has no clock or CSPRNG to call.
    let mut replay = ReplaySource::new(obs);
    let existing = BTreeSet::new();
    {
        let mut svc = IdService::new(ns(), &mut replay, &existing);
        let again: Vec<Uuid> = ids.iter().map(|_| svc.new_v7().unwrap()).collect();
        assert_eq!(again, ids);
        // Nothing more was recorded: a further draw is an error, not a fresh value.
        assert_eq!(svc.new_v7(), Err(IdError::ReplayExhausted));
    }
    assert_eq!(replay.remaining(), 0);
}

#[test]
fn ac2_replay_mismatch_is_typed() {
    let bits = Bits::new(V7_RANDOM_BITS, 5).unwrap();
    let mut wrong_order = ReplaySource::new(vec![Observation::RandomBits(bits), Observation::NowMs(1)]);
    assert_eq!(wrong_order.now_ms(), Err(IdError::ReplayMismatch));
    let mut wrong_width = ReplaySource::new(vec![Observation::RandomBits(bits)]);
    assert_eq!(wrong_width.random_bits(64), Err(IdError::ReplayMismatch));
    assert_eq!(wrong_width.random_bits(74), Ok(bits));
}

#[test]
fn ac3_derive_v5_matches_reference() {
    let doc = load("v5.json");
    let cases = arr(&obj(&doc)["cases"]);
    assert!(cases.len() >= 10);
    let existing: Vec<Uuid> = Vec::new();
    for c in cases {
        let c = obj(c);
        let name = st(c, "name");
        let canonical = to_canonical_bytes(&c["inputs"]).unwrap();
        assert_eq!(String::from_utf8(canonical).unwrap(), st(c, "canonical"), "{name}: canonical bytes");
        let mut src = ReplaySource::new(Vec::new());
        let svc = IdService::new(Uuid::parse(st(c, "namespace")).unwrap(), &mut src, &existing);
        let id = svc.derive_v5(&c["inputs"]).unwrap();
        assert_eq!(id.to_string(), st(c, "expected"), "{name}");
        assert_eq!(id.version(), 5);
        assert_eq!(id.variant_bits(), 0b10);
    }
    for r in arr(&obj(&doc)["raw"]) {
        let r = obj(r);
        let id = uuid_v5(&Uuid::parse(st(r, "namespace")).unwrap(), st(r, "name").as_bytes());
        assert_eq!(id.to_string(), st(r, "expected"));
    }
}

#[test]
fn derive_v5_rejects_inputs_without_canonical_form() {
    let mut src = ReplaySource::new(Vec::new());
    let existing = BTreeSet::new();
    let svc = IdService::new(ns(), &mut src, &existing);
    let e = svc.derive_v5(&CValue::Num(f64::NAN)).unwrap_err();
    assert_eq!(e.code(), "bad_input");
}

#[test]
fn ac4_import_of_existing_id_is_id_collision() {
    let taken = Uuid::parse("017f22e2-79b0-7cc3-98c4-dc0c0c07398f").unwrap();
    let existing: BTreeSet<Uuid> = [taken].into_iter().collect();
    let mut src = ReplaySource::new(Vec::new());
    let svc = IdService::new(ns(), &mut src, &existing);
    let e = svc.import("017f22e2-79b0-7cc3-98c4-dc0c0c07398f").unwrap_err();
    assert_eq!(e, IdError::IdCollision(taken));
    assert_eq!(e.code(), "id_collision");
    // Same ID in upper case is the same ID.
    assert_eq!(svc.import("017F22E2-79B0-7CC3-98C4-DC0C0C07398F"), Err(IdError::IdCollision(taken)));
    assert_eq!(svc.import_uuid(taken), Err(IdError::IdCollision(taken)));
    // A new ID (here a v4, as QIF files often carry) comes back unchanged.
    let fresh = svc.import("6f1c2a3b-4d5e-4f60-8a1b-2c3d4e5f6071").unwrap();
    assert_eq!(fresh.to_string(), "6f1c2a3b-4d5e-4f60-8a1b-2c3d4e5f6071");
    assert_eq!(fresh.version(), 4);
}

#[test]
fn import_rejects_malformed_ids() {
    let existing = BTreeSet::new();
    let mut src = ReplaySource::new(Vec::new());
    let svc = IdService::new(ns(), &mut src, &existing);
    for bad in [
        "",
        "017f22e2-79b0-7cc3-98c4-dc0c0c07398",
        "017f22e2-79b0-7cc3-98c4-dc0c0c07398f0",
        "017f22e279b07cc398c4dc0c0c07398f",
        "017f22e2_79b0-7cc3-98c4-dc0c0c07398f",
        "017f22e2-79b0-7cc3-98c4-dc0c0c07398g",
        "{17f22e2-79b0-7cc3-98c4-dc0c0c07398f}",
        "017f22e2-79b0-7cc3-98c4-dc0c0c0739\u{e9}",
        "00000000-0000-0000-0000-000000000000",
        "ffffffff-ffff-ffff-ffff-ffffffffffff",
    ] {
        assert_eq!(svc.import(bad), Err(IdError::BadId), "{bad:?}");
    }
    assert_eq!(IdError::BadId.code(), "bad_id");
}

#[test]
fn v7_rejects_out_of_range_observations() {
    let ok = Bits::new(V7_RANDOM_BITS, (1u128 << 74) - 1).unwrap();
    assert_eq!(uuid_v7(1 << 48, &ok), Err(IdError::BadObservation));
    assert!(uuid_v7((1 << 48) - 1, &ok).is_ok());
    assert_eq!(Bits::new(74, 1u128 << 74), Err(IdError::BadObservation));
    assert_eq!(Bits::new(129, 0), Err(IdError::BadObservation));
    assert!(Bits::new(128, u128::MAX).is_ok());
    let short = Bits::new(64, 1).unwrap();
    assert_eq!(uuid_v7(0, &short), Err(IdError::BadObservation));
}

#[test]
fn live_source_records_and_replays() {
    let mut live = live_source();
    let existing = BTreeSet::new();
    let ids: Vec<Uuid> = {
        let mut svc = IdService::new(ns(), &mut live, &existing);
        (0..4).map(|_| svc.new_v7().unwrap()).collect()
    };
    for id in &ids {
        assert_eq!(id.version(), 7);
        assert_eq!(id.variant_bits(), 0b10);
    }
    assert_ne!(ids[0], ids[1]);
    let obs = live.into_observations();
    assert_eq!(obs.len(), 8);
    let mut replay = ReplaySource::new(obs);
    let mut svc = IdService::new(ns(), &mut replay, &existing);
    let again: Vec<Uuid> = (0..4).map(|_| svc.new_v7().unwrap()).collect();
    assert_eq!(again, ids);
}

#[test]
fn observations_have_a_canonical_journal_form() {
    // Hand-labelled from the documented journal form.
    let t = to_canonical_bytes(&Observation::NowMs(1645557742000).to_cvalue()).unwrap();
    assert_eq!(t, br#"{"kind":"now_ms","value":"1645557742000"}"#);
    let bits = Bits::new(74, (0xCC3u128 << 62) | 0x18C4DC0C0C07398F).unwrap();
    let r = to_canonical_bytes(&Observation::RandomBits(bits).to_cvalue()).unwrap();
    assert_eq!(r, br#"{"kind":"random_bits","value":"0x330d8c4dc0c0c07398f","width":"74"}"#);
}

#[test]
fn uuid_text_round_trip_and_constants() {
    let id = Uuid::parse("2ED6657D-E927-568B-95E1-2665A8AEA6A2").unwrap();
    assert_eq!(id.to_string(), "2ed6657d-e927-568b-95e1-2665a8aea6a2");
    assert_eq!("2ed6657d-e927-568b-95e1-2665a8aea6a2".parse::<Uuid>().unwrap(), id);
    assert_eq!(Uuid::from_u128(id.as_u128()), id);
    assert_eq!(Uuid::from_bytes(*id.as_bytes()), id);
    assert_eq!(Uuid::NIL.to_string(), "00000000-0000-0000-0000-000000000000");
}

/// Clock that works, CSPRNG that fails on its first call only.
struct FlakyRng {
    rng_calls: usize,
}

impl ObservationSource for FlakyRng {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        Ok(1645557742000)
    }
    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        self.rng_calls += 1;
        if self.rng_calls == 1 {
            return Err(IdError::ObservationUnavailable);
        }
        Bits::new(n, (0xCC3u128 << 62) | 0x18C4DC0C0C07398F)
    }
}

#[test]
fn failed_draw_records_nothing_and_retry_replays() {
    let mut src = Recording::new(FlakyRng { rng_calls: 0 });
    let existing = BTreeSet::new();
    let id = {
        let mut svc = IdService::new(ns(), &mut src, &existing);
        assert_eq!(svc.new_v7(), Err(IdError::ObservationUnavailable));
        svc.new_v7().unwrap()
    };
    assert_eq!(id.to_string(), "017f22e2-79b0-7cc3-98c4-dc0c0c07398f");
    let obs = src.observations().to_vec();
    assert_eq!(obs.len(), 2);
    assert!(matches!(obs[0], Observation::NowMs(1645557742000)));
    assert!(matches!(obs[1], Observation::RandomBits(_)));
    let mut replay = ReplaySource::new(obs);
    let mut svc = IdService::new(ns(), &mut replay, &existing);
    assert_eq!(svc.new_v7(), Ok(id));
}

#[test]
fn failed_replay_draw_consumes_nothing() {
    let mut replay = ReplaySource::new(vec![Observation::NowMs(1)]);
    assert_eq!(replay.draw_v7(), Err(IdError::ReplayExhausted));
    assert_eq!(replay.remaining(), 1);
    assert_eq!(replay.now_ms(), Ok(1));
}

#[test]
fn observation_journal_form_round_trips() {
    let (obs, _, _, _) = run_ac1();
    for o in &obs {
        let v = o.to_cvalue();
        assert_eq!(Observation::from_cvalue(&v), Ok(*o));
        // Through journal text: 64-bit integers come back as decimal strings.
        let text = to_canonical_bytes(&v).unwrap();
        assert_eq!(Observation::from_cvalue(&parse(&text).unwrap()), Ok(*o), "{o:?}");
    }
}

#[test]
fn observation_from_cvalue_fails_closed() {
    for bad in [
        r#"[]"#,
        r#"{"kind":"now_ms"}"#,
        r#"{"kind":"now_ms","value":"-1"}"#,
        r#"{"kind":"now_ms","value":"01"}"#,
        r#"{"kind":"now_ms","value":""}"#,
        r#"{"kind":"now_ms","value":"18446744073709551616"}"#,
        r#"{"kind":"now_ms","value":5}"#,
        r#"{"kind":"now_ms","value":"5","extra":1}"#,
        r#"{"kind":"clock","value":"5"}"#,
        r#"{"kind":"random_bits","value":"0x1","width":"129"}"#,
        r#"{"kind":"random_bits","value":"0x4","width":"2"}"#,
        r#"{"kind":"random_bits","value":"0xA","width":"74"}"#,
        r#"{"kind":"random_bits","value":"0x01","width":"74"}"#,
        r#"{"kind":"random_bits","value":"0x","width":"74"}"#,
        r#"{"kind":"random_bits","value":"1","width":"74"}"#,
        r#"{"kind":"random_bits","value":"0x1","width":"74","x":null}"#,
    ] {
        let v = parse(bad.as_bytes()).unwrap();
        assert_eq!(Observation::from_cvalue(&v), Err(IdError::BadObservation), "{bad}");
    }
    let ok = parse(br#"{"kind":"random_bits","value":"0x0","width":"74"}"#).unwrap();
    assert_eq!(Observation::from_cvalue(&ok), Ok(Observation::RandomBits(Bits::new(74, 0).unwrap())));
}
