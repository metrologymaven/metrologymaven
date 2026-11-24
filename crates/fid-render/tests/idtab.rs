//! Acceptance criteria AC-1 .. AC-5 of spec leaf VIZ.RENDER.SCENE.IDTAB, plus the error and
//! boundary rules pinned in its Design. Expected values come from `fixtures/viz/scene-ids`
//! (written by its independent Python script, seed 21) or are hand-labelled from the spec's
//! rules; none is produced by running this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_render::scene::idtab::{
    next_render_id, DerivedId, EntityId, IdError, IdTable, Identity, RenderId, Slot, ViewOp, MAX_IDS_PER_ENTITY,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/viz/scene-ids").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn uuid(text: &str) -> EntityId {
    EntityId::parse(text).unwrap_or_else(|_| panic!("bad uuid {text}"))
}

fn ent(n: u128) -> EntityId {
    EntityId::from_u128(n)
}

fn rid(n: u32) -> RenderId {
    RenderId::new(n).unwrap()
}

fn derived(source: EntityId, op: ViewOp) -> Identity {
    Identity::Derived(DerivedId { source, op })
}

/// Parses `drawables.txt`: (expected render id, identity) in registration order.
fn drawables() -> Vec<(u32, Identity)> {
    let mut out = Vec::new();
    for line in fixture("drawables.txt").lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split(' ').collect();
        let id: u32 = f[0].parse().unwrap();
        let identity = match (f[1], f.len()) {
            ("entity", 3) => Identity::Entity(uuid(f[2])),
            ("derived", 4) => derived(uuid(f[2]), ViewOp::from_name(f[3]).unwrap()),
            _ => panic!("bad fixture line {line}"),
        };
        out.push((id, identity));
    }
    out
}

/// Parses `by_entity.txt`: entity -> expected ascending render IDs.
fn by_entity() -> BTreeMap<EntityId, Vec<u32>> {
    let mut out = BTreeMap::new();
    for line in fixture("by_entity.txt").lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.split(' ');
        let e = uuid(f.next().unwrap());
        let ids: Vec<u32> = f.map(|x| x.parse().unwrap()).collect();
        assert!(out.insert(e, ids).is_none(), "entity listed twice");
    }
    out
}

fn ids(list: &[RenderId]) -> Vec<u32> {
    list.iter().map(|r| r.get()).collect()
}

#[test]
fn ac1_fixture_gets_ids_1_to_800_in_order_and_lookups_round_trip() {
    let cases = drawables();
    assert_eq!(cases.len(), 800);
    let entities = cases.iter().filter(|(_, i)| matches!(i, Identity::Entity(_))).count();
    assert_eq!((entities, cases.len() - entities), (500, 300));

    let mut t = IdTable::new();
    for (expected, identity) in &cases {
        let got = t.register(Some(*identity)).unwrap();
        assert_eq!(got.get(), *expected);
    }
    assert_eq!(ids(&cases.iter().map(|(n, _)| rid(*n)).collect::<Vec<_>>()), (1..=800).collect::<Vec<u32>>());
    assert_eq!(t.allocated(), 800);
    assert_eq!(t.live_count(), 800);

    for (n, identity) in &cases {
        assert_eq!(t.identity(rid(*n)), Ok(Slot::Live(*identity)));
        assert_eq!(t.resolve(*n), Ok(Some(Slot::Live(*identity))));
        assert_eq!(t.render_id_of(*identity), Some(rid(*n)));
    }
    let expected = by_entity();
    assert_eq!(expected.len(), 500);
    for (e, want) in &expected {
        assert_eq!(&ids(t.render_ids(*e)), want, "entity {e}");
    }
}

#[test]
fn ac2_missing_identity_is_refused_without_consuming_an_id() {
    let mut t = IdTable::new();
    assert_eq!(t.register(Some(Identity::Entity(ent(1)))), Ok(rid(1)));
    assert_eq!(t.register(Some(Identity::Entity(ent(2)))), Ok(rid(2)));
    assert_eq!(t.register(None), Err(IdError::MissingIdentity));
    assert_eq!(t.register(None).unwrap_err().code(), "MissingIdentity");
    assert_eq!(t.allocated(), 2);
    assert_eq!(t.resolve(3), Err(IdError::UnknownRenderId(3)));
    // The next successful registration receives the ID that would have been next.
    assert_eq!(t.register(Some(derived(ent(1), ViewOp::Label))), Ok(rid(3)));
}

#[test]
fn ac3_retired_id_resolves_to_retired_and_is_never_reused() {
    let mut t = IdTable::new();
    for n in 1..=20u128 {
        t.register(Some(Identity::Entity(ent(n)))).unwrap();
    }
    assert_eq!(t.retire(rid(17)), Ok(Identity::Entity(ent(17))));
    assert_eq!(t.identity(rid(17)), Ok(Slot::Retired(Identity::Entity(ent(17)))));
    assert_eq!(t.resolve(17), Ok(Some(Slot::Retired(Identity::Entity(ent(17))))));
    assert_eq!(t.render_ids(ent(17)), &[] as &[RenderId]);
    assert_eq!(t.live_count(), 19);

    // Re-registering the same entity, and a fresh one, both get new IDs.
    assert_eq!(t.register(Some(Identity::Entity(ent(17)))), Ok(rid(21)));
    assert_eq!(t.register(Some(Identity::Entity(ent(99)))), Ok(rid(22)));
    // The tombstone still names the old binding, never the new one.
    assert_eq!(t.identity(rid(17)), Ok(Slot::Retired(Identity::Entity(ent(17)))));
    assert_eq!(ids(t.render_ids(ent(17))), [21]);

    assert_eq!(t.retire(rid(17)), Err(IdError::AlreadyRetired(rid(17))));
    assert_eq!(t.retire(rid(23)), Err(IdError::UnknownRenderId(23)));
}

#[test]
fn ac4_entity_lookup_returns_own_and_three_derived_ascending() {
    let mut t = IdTable::new();
    let a = ent(0xA);
    let b = ent(0xB);
    // Interleave another entity's drawables so the answer is not just a contiguous range.
    t.register(Some(derived(a, ViewOp::Label))).unwrap(); // 1
    t.register(Some(Identity::Entity(b))).unwrap(); // 2
    t.register(Some(Identity::Entity(a))).unwrap(); // 3
    t.register(Some(derived(b, ViewOp::Label))).unwrap(); // 4
    t.register(Some(derived(a, ViewOp::Whisker))).unwrap(); // 5
    t.register(Some(derived(b, ViewOp::Ghost))).unwrap(); // 6
    t.register(Some(derived(a, ViewOp::Leader))).unwrap(); // 7
    assert_eq!(ids(t.render_ids(a)), [1, 3, 5, 7]);
    assert_eq!(ids(t.render_ids(b)), [2, 4, 6]);
    assert_eq!(t.render_ids(ent(0xC)), &[] as &[RenderId]);

    // The same property on the fixture: every entity with exactly three derived drawables.
    let cases = drawables();
    let mut t = IdTable::new();
    for (_, identity) in &cases {
        t.register(Some(*identity)).unwrap();
    }
    let mut checked = 0;
    for (e, want) in by_entity() {
        let derived_count = cases.iter().filter(|(_, i)| matches!(i, Identity::Derived(d) if d.source == e)).count();
        if derived_count == 3 {
            assert_eq!(ids(t.render_ids(e)), want);
            assert_eq!(want.len(), 4);
            checked += 1;
        }
    }
    assert!(checked >= 1);
}

/// SplitMix64 (Steele, Lea, Flood 2014): the seeded generator of the property test.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[test]
fn ac5_property_no_render_id_is_ever_bound_to_two_identities() {
    let mut rng = SplitMix64(21);
    for _ in 0..10_000 {
        let mut t = IdTable::with_limit(48);
        // The model: identity bound at allocation, and whether the ID is still live.
        let mut bound: Vec<(Identity, bool)> = Vec::new();
        let steps = 1 + rng.below(64);
        for _ in 0..steps {
            let roll = rng.below(10);
            if roll < 6 {
                let e = ent(u128::from(rng.below(4)));
                let identity = match rng.below(8) {
                    0 => None,
                    1 => Some(Identity::Entity(e)),
                    k => Some(derived(e, ViewOp::from_code(u8::try_from(k - 1).unwrap()).unwrap())),
                };
                let live_dup = identity.and_then(|i| bound.iter().position(|&(b, live)| live && b == i));
                match (identity, t.register(identity)) {
                    (None, r) => assert_eq!(r, Err(IdError::MissingIdentity)),
                    (Some(_), Err(IdError::DuplicateIdentity(id))) => {
                        assert_eq!(live_dup.map(|p| p + 1), Some(id.get() as usize));
                    }
                    (Some(_), Err(IdError::IdSpaceExhausted)) => {
                        assert!(live_dup.is_none());
                        assert_eq!(bound.len(), 48);
                    }
                    (Some(i), Ok(id)) => {
                        assert!(live_dup.is_none());
                        // Dense and monotonic: the next unused value.
                        assert_eq!(id.get() as usize, bound.len() + 1);
                        bound.push((i, true));
                    }
                    (_, r) => panic!("unexpected {r:?}"),
                }
            } else if !bound.is_empty() {
                // Retire any allocated ID, live or not, and sometimes one past the end.
                let n = 1 + rng.below(bound.len() as u64 + 1);
                let id = rid(u32::try_from(n).unwrap());
                let r = t.retire(id);
                match bound.get_mut(n as usize - 1) {
                    None => assert_eq!(r, Err(IdError::UnknownRenderId(id.get()))),
                    Some((i, live)) if *live => {
                        assert_eq!(r, Ok(*i));
                        *live = false;
                    }
                    Some(_) => assert_eq!(r, Err(IdError::AlreadyRetired(id))),
                }
            }
            // Invariant after every step: each allocated ID resolves to the one identity it was
            // bound to (live or tombstone), and the reverse map agrees with the model.
            for (k, (i, live)) in bound.iter().enumerate() {
                let want = if *live { Slot::Live(*i) } else { Slot::Retired(*i) };
                assert_eq!(t.resolve(k as u32 + 1), Ok(Some(want)));
            }
            for e in 0..4u128 {
                let want: Vec<u32> = bound
                    .iter()
                    .enumerate()
                    .filter(|(_, (i, live))| *live && i.entity() == ent(e))
                    .map(|(k, _)| k as u32 + 1)
                    .collect();
                let got = ids(t.render_ids(ent(e)));
                assert_eq!(got, want);
                assert!(got.len() <= MAX_IDS_PER_ENTITY);
            }
        }
    }
}

#[test]
fn render_id_value_rules() {
    assert_eq!(RenderId::MAX, 4_294_967_294); // 2^32 - 2
    assert_eq!(RenderId::new(0), None);
    assert_eq!(RenderId::new(u32::MAX), None);
    assert_eq!(RenderId::new(1).map(RenderId::get), Some(1));
    assert_eq!(RenderId::new(4_294_967_294).map(RenderId::get), Some(4_294_967_294));
    assert_eq!(rid(7).to_string(), "7");
}

#[test]
fn id_space_boundary_is_refused_not_wrapped() {
    // The pure allocation step at the real 2^32 - 2 boundary.
    assert_eq!(next_render_id(0, RenderId::MAX), Ok(rid(1)));
    assert_eq!(next_render_id(4_294_967_293, RenderId::MAX), Ok(rid(4_294_967_294)));
    assert_eq!(next_render_id(4_294_967_294, RenderId::MAX), Err(IdError::IdSpaceExhausted));
    assert_eq!(next_render_id(u32::MAX, RenderId::MAX), Err(IdError::IdSpaceExhausted));
    // A limit above 2^32 - 2 is clamped: u32::MAX is never allocated.
    assert_eq!(next_render_id(4_294_967_294, u32::MAX), Err(IdError::IdSpaceExhausted));
    assert_eq!(IdTable::with_limit(u32::MAX).limit(), RenderId::MAX);
    assert_eq!(IdTable::new().limit(), RenderId::MAX);

    // The table stops at its limit, allocates nothing more, and stays usable for lookups.
    let mut t = IdTable::with_limit(3);
    for n in 1..=3u128 {
        t.register(Some(Identity::Entity(ent(n)))).unwrap();
    }
    assert_eq!(t.register(Some(Identity::Entity(ent(4)))), Err(IdError::IdSpaceExhausted));
    assert_eq!(t.register(Some(Identity::Entity(ent(4)))).unwrap_err().code(), "IdSpaceExhausted");
    t.retire(rid(2)).unwrap();
    // Retiring does not free space: IDs are never reused.
    assert_eq!(t.register(Some(Identity::Entity(ent(2)))), Err(IdError::IdSpaceExhausted));
    assert_eq!(t.allocated(), 3);
    assert_eq!(t.render_ids(ent(4)), &[] as &[RenderId]);
    assert_eq!(t.identity(rid(3)), Ok(Slot::Live(Identity::Entity(ent(3)))));

    let mut empty = IdTable::with_limit(0);
    assert_eq!(empty.register(Some(Identity::Entity(ent(1)))), Err(IdError::IdSpaceExhausted));
    // MissingIdentity is reported before exhaustion.
    assert_eq!(empty.register(None), Err(IdError::MissingIdentity));
}

#[test]
fn duplicate_live_identity_is_refused() {
    let mut t = IdTable::new();
    let e = ent(5);
    assert_eq!(t.register(Some(Identity::Entity(e))), Ok(rid(1)));
    assert_eq!(t.register(Some(derived(e, ViewOp::Ghost))), Ok(rid(2)));
    assert_eq!(t.register(Some(Identity::Entity(e))), Err(IdError::DuplicateIdentity(rid(1))));
    assert_eq!(t.register(Some(derived(e, ViewOp::Ghost))), Err(IdError::DuplicateIdentity(rid(2))));
    assert_eq!(t.allocated(), 2);
    // A different op, or another source, is a different identity.
    assert_eq!(t.register(Some(derived(e, ViewOp::ClipCap))), Ok(rid(3)));
    assert_eq!(t.register(Some(derived(ent(6), ViewOp::Ghost))), Ok(rid(4)));
    // After retiring, the identity may be registered again, under a new ID.
    t.retire(rid(2)).unwrap();
    assert_eq!(t.register(Some(derived(e, ViewOp::Ghost))), Ok(rid(5)));
    assert_eq!(ids(t.render_ids(e)), [1, 3, 5]);
    assert_eq!(t.render_id_of(derived(e, ViewOp::Ghost)), Some(rid(5)));
    assert_eq!(t.render_id_of(derived(e, ViewOp::Proxy)), None);
}

#[test]
fn entity_with_every_view_op_hits_the_bound() {
    let mut t = IdTable::new();
    let e = ent(9);
    t.register(Some(Identity::Entity(e))).unwrap();
    for op in ViewOp::ALL {
        t.register(Some(derived(e, op))).unwrap();
    }
    assert_eq!(t.render_ids(e).len(), MAX_IDS_PER_ENTITY);
    assert_eq!(MAX_IDS_PER_ENTITY, 7);
    for op in ViewOp::ALL {
        assert!(matches!(t.register(Some(derived(e, op))), Err(IdError::DuplicateIdentity(_))));
    }
}

#[test]
fn resolve_of_raw_buffer_values() {
    let mut t = IdTable::new();
    t.register(Some(Identity::Entity(ent(1)))).unwrap();
    assert_eq!(t.resolve(0), Ok(None));
    assert_eq!(t.resolve(1), Ok(Some(Slot::Live(Identity::Entity(ent(1))))));
    assert_eq!(t.resolve(2), Err(IdError::UnknownRenderId(2)));
    assert_eq!(t.resolve(u32::MAX), Err(IdError::UnknownRenderId(u32::MAX)));
    assert_eq!(t.identity(rid(2)), Err(IdError::UnknownRenderId(2)));
    assert_eq!(IdTable::new().resolve(1), Err(IdError::UnknownRenderId(1)));
}

#[test]
fn view_op_codes_and_names_are_pinned() {
    let pinned = [
        (ViewOp::Whisker, 1, "whisker"),
        (ViewOp::Label, 2, "label"),
        (ViewOp::Leader, 3, "leader"),
        (ViewOp::Ghost, 4, "ghost"),
        (ViewOp::ClipCap, 5, "clip_cap"),
        (ViewOp::Proxy, 6, "proxy"),
    ];
    assert_eq!(ViewOp::ALL.len(), pinned.len());
    for (op, code, name) in pinned {
        assert_eq!(op.code(), code);
        assert_eq!(op.name(), name);
        assert_eq!(ViewOp::from_code(code), Some(op));
        assert_eq!(ViewOp::from_name(name), Some(op));
    }
    for bad in [0u8, 7, 255] {
        assert_eq!(ViewOp::from_code(bad), None);
    }
    for bad in ["", "Label", "clipcap", "clip-cap", " label"] {
        assert_eq!(ViewOp::from_name(bad), None);
    }
}

#[test]
fn identity_entity_and_error_texts() {
    let e = ent(3);
    assert_eq!(Identity::Entity(e).entity(), e);
    assert_eq!(derived(e, ViewOp::Proxy).entity(), e);
    assert_eq!(Slot::Retired(Identity::Entity(e)).identity(), Identity::Entity(e));
    let all = [
        (IdError::MissingIdentity, "MissingIdentity"),
        (IdError::IdSpaceExhausted, "IdSpaceExhausted"),
        (IdError::UnknownRenderId(4), "UnknownRenderId"),
        (IdError::DuplicateIdentity(rid(1)), "DuplicateIdentity"),
        (IdError::AlreadyRetired(rid(1)), "AlreadyRetired"),
        (IdError::AllocationFailed, "AllocationFailed"),
        (IdError::EntityFull(e), "EntityFull"),
    ];
    for (err, code) in all {
        assert_eq!(err.code(), code);
        assert!(err.to_string().starts_with(&format!("{code}: ")));
    }
}

#[test]
fn same_calls_give_same_table() {
    // D0: two tables fed the same sequence are identical.
    let cases = drawables();
    let build = || {
        let mut t = IdTable::new();
        for (_, i) in &cases {
            t.register(Some(*i)).unwrap();
        }
        for n in (5..=800).step_by(7) {
            t.retire(rid(n)).unwrap();
        }
        t
    };
    assert_eq!(build(), build());
}

#[test]
fn one_million_registrations() {
    // Design budget: 10^6 registrations (timed by the ignored bench below in release).
    let mut t = IdTable::new();
    for n in 0..1_000_000u128 {
        t.register(Some(Identity::Entity(ent(n)))).unwrap();
    }
    assert_eq!(t.allocated(), 1_000_000);
    assert_eq!(t.resolve(1_000_000), Ok(Some(Slot::Live(Identity::Entity(ent(999_999))))));
}

/// Performance budget of the Design: register 10^6 drawables under 200 ms and look up under
/// 1 µs. Wall-clock limits are not asserted in shared CI; run with
/// `cargo test --release -p fid-render -- --ignored --nocapture`.
#[test]
#[ignore = "wall-clock release performance budget (<200ms / <1us); fails in debug and noisy on shared CI"]
fn bench_budget() {
    let start = std::time::Instant::now();
    let mut t = IdTable::new();
    for n in 0..1_000_000u128 {
        t.register(Some(Identity::Entity(ent(n.wrapping_mul(0x9E37_79B9_7F4A_7C15))))).unwrap();
    }
    let reg = start.elapsed();
    let start = std::time::Instant::now();
    let mut hits = 0usize;
    for n in 1..=1_000_000u32 {
        if let Ok(Some(Slot::Live(i))) = t.resolve(n) {
            hits += t.render_ids(i.entity()).len();
        }
    }
    let per = start.elapsed() / 1_000_000;
    println!("register 10^6: {reg:?}; forward+reverse lookup: {per:?} each; {hits}");
    assert!(reg.as_millis() < 200 && per.as_nanos() < 1_000);
}
