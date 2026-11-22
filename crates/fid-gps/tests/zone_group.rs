//! Acceptance criteria tests for spec node `GDT.ZONE.GROUP.IDS` (AC-1..AC-4, REQ-001..REQ-005).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::Path;

use fid_canon::{parse, CValue};
use fid_gps::zone::group::{
    all_or_nothing, attach_group_ref, build_group_ref, group_id, group_id_canonical_bytes, normalize_members,
    pose_canonical_bytes, pose_digest, BadGroupReason, CharId, GroupError, GroupKind,
};
use fid_gps::zone::shape::Pose;
use fid_types::Digest;
use fid_uuid::Uuid;

fn load_fixture() -> CValue {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/zone/group/ids.json");
    let bytes = fs::read(&path).expect("read fixtures/gdt/zone/group/ids.json");
    parse(&bytes).expect("parse fixture JSON")
}

fn as_obj(v: &CValue) -> &std::collections::BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => m,
        other => panic!("expected object, got {other:?}"),
    }
}

fn as_arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("expected array, got {other:?}"),
    }
}

fn as_str(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

fn parse_members(arr: &[CValue]) -> Vec<CharId> {
    arr.iter().map(|v| CharId::parse(as_str(v)).expect("valid CharId UUID")).collect()
}

fn parse_f64_hex(s: &str) -> f64 {
    let hex = s.strip_prefix("0x").expect("0x prefix");
    let bits = u64::from_str_radix(hex, 16).expect("valid 64-bit hex");
    f64::from_bits(bits)
}

fn parse_p3_hex(arr: &[CValue]) -> [f64; 3] {
    assert_eq!(arr.len(), 3);
    [parse_f64_hex(as_str(&arr[0])), parse_f64_hex(as_str(&arr[1])), parse_f64_hex(as_str(&arr[2]))]
}

#[test]
fn ac1_group_id_is_order_independent_and_matches_fixture() {
    let root = load_fixture();
    let obj = as_obj(&root);
    let ns = Uuid::parse(as_str(obj.get("namespace").unwrap())).expect("valid namespace UUID");

    for case in as_arr(obj.get("group_cases").unwrap()) {
        let c = as_obj(case);
        let kind = GroupKind::parse(as_str(c.get("kind").unwrap())).expect("valid group kind");
        let label = as_str(c.get("label").unwrap());
        let order_a = parse_members(as_arr(c.get("order_a").unwrap()));
        let order_b = parse_members(as_arr(c.get("order_b").unwrap()));
        let expected_canon = as_str(c.get("expected_canonical").unwrap());
        let expected_id = Uuid::parse(as_str(c.get("expected_group_id").unwrap())).expect("valid expected UUID");

        let canon_a = String::from_utf8(group_id_canonical_bytes(kind, label, &order_a).unwrap()).unwrap();
        let canon_b = String::from_utf8(group_id_canonical_bytes(kind, label, &order_b).unwrap()).unwrap();
        assert_eq!(canon_a, expected_canon);
        assert_eq!(canon_b, expected_canon);

        let id_a = group_id(&ns, kind, label, &order_a).expect("group_id order A");
        let id_b = group_id(&ns, kind, label, &order_b).expect("group_id order B");
        assert_eq!(id_a, expected_id);
        assert_eq!(id_b, expected_id);
        assert_eq!(id_a.version(), 5, "group ID must be UUIDv5");
    }
}

#[test]
fn ac2_pose_digest_matches_fixture_and_preserves_f64_bits() {
    let root = load_fixture();
    let obj = as_obj(&root);

    let mut seen_digests = Vec::new();
    for case in as_arr(obj.get("pose_cases").unwrap()) {
        let c = as_obj(case);
        let origin = parse_p3_hex(as_arr(c.get("origin_hex").unwrap()));
        let direction = parse_p3_hex(as_arr(c.get("direction_hex").unwrap()));
        let expected_canon = as_str(c.get("expected_canonical").unwrap());
        let expected_digest =
            Digest::parse(as_str(c.get("expected_pose_digest").unwrap())).expect("valid expected digest");

        let pose = Pose::new(origin, direction);
        let canon = String::from_utf8(pose_canonical_bytes(&pose).unwrap()).unwrap();
        assert_eq!(canon, expected_canon);

        let dig = pose_digest(&pose).expect("pose_digest");
        assert_eq!(dig, expected_digest);
        seen_digests.push(dig);
    }

    // Confirm -0.0 and +0.0 produce distinct IEEE-754 bit digests.
    let pos_zero = Pose::new([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    let neg_zero = Pose::new([-0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    assert_ne!(pose_digest(&pos_zero).unwrap(), pose_digest(&neg_zero).unwrap());

    // Non-finite poses are refused.
    assert_eq!(pose_digest(&Pose::new([f64::NAN, 0.0, 0.0], [0.0, 0.0, 1.0])), Err(GroupError::NonFinitePose));
    assert_eq!(pose_digest(&Pose::new([0.0, 0.0, 0.0], [f64::INFINITY, 0.0, 0.0])), Err(GroupError::NonFinitePose));
}

#[test]
fn ac3_all_or_nothing_reports_lowest_failing_member_and_emits_no_values() {
    let root = load_fixture();
    let obj = as_obj(&root);
    let members = parse_members(as_arr(obj.get("members_sorted").unwrap()));
    let [m1, m2, m3, m4] = [members[0], members[1], members[2], members[3]];

    // Supply out of order with both m2 and m4 failing -> must name m2 (lowest failing ID).
    let inputs: Vec<(CharId, Result<f64, &'static str>)> =
        vec![(m4, Err("insufficient_points")), (m3, Ok(0.012)), (m2, Err("missing_datum")), (m1, Ok(0.008))];
    let err = all_or_nothing(inputs).expect_err("incomplete group must fail");
    assert_eq!(err.code(), "group_incomplete");
    assert_eq!(err, GroupError::GroupIncomplete { member: m2, cause: "missing_datum".to_owned() });

    // When all succeed, values are returned in ascending byte order of CharId.
    let ok_inputs: Vec<(CharId, Result<u32, &'static str>)> = vec![(m3, Ok(3)), (m1, Ok(1)), (m4, Ok(4)), (m2, Ok(2))];
    let out = all_or_nothing(ok_inputs).expect("complete group succeeds");
    assert_eq!(out, vec![(m1, 1), (m2, 2), (m3, 3), (m4, 4)]);
}

#[test]
fn ac4_bad_group_rejects_too_few_and_duplicate_members() {
    let root = load_fixture();
    let obj = as_obj(&root);
    let ns = Uuid::parse(as_str(obj.get("namespace").unwrap())).unwrap();
    let members = parse_members(as_arr(obj.get("members_sorted").unwrap()));
    let [m1, m2] = [members[0], members[1]];

    // Zero or one member -> bad_group{too_few}.
    assert_eq!(normalize_members(&[]), Err(GroupError::BadGroup { reason: BadGroupReason::TooFew }));
    assert_eq!(
        group_id(&ns, GroupKind::Pattern, "single", &[m1]),
        Err(GroupError::BadGroup { reason: BadGroupReason::TooFew })
    );
    let one_res: Vec<(CharId, Result<u32, &'static str>)> = vec![(m1, Ok(1))];
    assert_eq!(all_or_nothing(one_res), Err(GroupError::BadGroup { reason: BadGroupReason::TooFew }));

    // Duplicate member -> bad_group{duplicate}.
    assert_eq!(
        group_id(&ns, GroupKind::SimReq, "dup", &[m1, m2, m1]),
        Err(GroupError::BadGroup { reason: BadGroupReason::Duplicate })
    );
    assert_eq!(normalize_members(&[m1, m1]), Err(GroupError::BadGroup { reason: BadGroupReason::Duplicate }));
    let dup_res: Vec<(CharId, Result<u32, &'static str>)> = vec![(m2, Ok(2)), (m2, Ok(2))];
    assert_eq!(all_or_nothing(dup_res), Err(GroupError::BadGroup { reason: BadGroupReason::Duplicate }));
}

#[test]
fn req003_every_member_receives_identical_group_ref() {
    let root = load_fixture();
    let obj = as_obj(&root);
    let ns = Uuid::parse(as_str(obj.get("namespace").unwrap())).unwrap();
    let members = parse_members(as_arr(obj.get("members_sorted").unwrap()));
    let pose = Pose::new([0.0125, -0.025, 0.5], [0.0, 0.0, 1.0]);

    let reversed: Vec<CharId> = members.iter().copied().rev().collect();
    let gref_a = build_group_ref(&ns, GroupKind::Pattern, "4x-bolt-circle-A", &members, &pose).unwrap();
    let gref_b = build_group_ref(&ns, GroupKind::Pattern, "4x-bolt-circle-A", &reversed, &pose).unwrap();
    assert_eq!(gref_a, gref_b);
    assert_eq!(gref_a.digest().unwrap(), gref_b.digest().unwrap());

    let vals: Vec<(CharId, usize)> = reversed.iter().copied().enumerate().map(|(i, id)| (id, i)).collect();
    let attached = attach_group_ref(&gref_a, vals).expect("attach group ref");
    assert_eq!(attached.len(), 4);
    for (idx, (id, _, ref_copy)) in attached.iter().enumerate() {
        assert_eq!(*id, members[idx]);
        assert_eq!(ref_copy, &gref_a);
    }

    // Member mismatch rejected.
    let subset: Vec<(CharId, usize)> = vec![(members[0], 0), (members[1], 1)];
    assert_eq!(attach_group_ref(&gref_a, subset), Err(GroupError::BadGroup { reason: BadGroupReason::MemberMismatch }));
}
