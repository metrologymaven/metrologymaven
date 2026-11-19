//! Acceptance criteria of spec node GDT.CHAR.DPROF (AC-1 .. AC-7) plus the error paths and
//! limits pinned in the leaf's Design. Expected references and canonical forms come from
//! the independent stdlib-Python reference `fixtures/gdt/dprof/make_cases.py`
//! (`expected.json`); error cases are hand-labelled from the leaf's rules. All profile
//! content is synthetic placeholder data.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::dprof::{
    parse_profile, publish_defaults_profile, replay, Branch, CharId, DprofError, DprofEvent, DprofState, Edition,
    KeyVocabulary, ProfileRef, PublishStatus, SemVer,
};
use fid_gdt::dmt_flow::roster::Dialect;
use fid_io_reg::{Principal, RootAccess, RootId, RootSet};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/dprof")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap()
}

fn vocab() -> KeyVocabulary {
    let v = fid_canon::parse(&read("vocabulary.json")).unwrap();
    let CValue::Obj(o) = v else { panic!("vocabulary") };
    let Some(CValue::Arr(keys)) = o.get("keys") else { panic!("keys") };
    let keys: Vec<&str> = keys
        .iter()
        .map(|k| match k {
            CValue::Str(s) => s.as_str(),
            _ => panic!("key"),
        })
        .collect();
    KeyVocabulary::new(keys).unwrap()
}

/// `expected.json` from the Python reference: file name -> (ref, canonical).
fn expected() -> BTreeMap<String, (String, String)> {
    let v = fid_canon::parse(&read("expected.json")).unwrap();
    let CValue::Obj(o) = v else { panic!("expected") };
    let Some(CValue::Arr(cases)) = o.get("cases") else { panic!("cases") };
    cases
        .iter()
        .map(|c| {
            let CValue::Obj(c) = c else { panic!("case") };
            let s = |k: &str| match c.get(k) {
                Some(CValue::Str(s)) => s.clone(),
                _ => panic!("{k}"),
            };
            (s("file"), (s("ref"), s("canonical")))
        })
        .collect()
}

fn exp_ref(file: &str) -> String {
    expected()[file].0.clone()
}

fn roots() -> RootSet {
    let mut r = RootSet::new();
    r.insert(RootId::new("imports").unwrap(), RootAccess::Import, fixtures());
    r
}

fn handle(file: &str) -> String {
    format!("root://imports/{file}")
}

fn publish(state: &mut DprofState, p: Principal, file: &str) -> Result<fid_gdt::chars::dprof::Published, DprofError> {
    publish_defaults_profile(state, &roots(), p, &handle(file), &vocab(), false)
}

fn rf(s: &str) -> ProfileRef {
    ProfileRef::parse(s).unwrap()
}

fn cid(s: &str) -> CharId {
    CharId::parse(s).unwrap()
}

const A100: &str = "asme_placeholder_1.0.0.json";
const A100_MOD: &str = "asme_placeholder_1.0.0_modified.json";
const A110: &str = "asme_placeholder_1.1.0.json";
const I100: &str = "iso_placeholder_1.0.0.json";

// ------------------------------------------------------------------ reference oracle

#[test]
fn canonical_form_and_digest_match_the_python_reference() {
    for (file, (r, canonical)) in expected() {
        let doc = parse_profile(&read(&file), &vocab()).unwrap();
        assert_eq!(String::from_utf8(doc.canonical().to_vec()).unwrap(), canonical, "{file}");
        assert_eq!(doc.reference().to_string(), r, "{file}");
        assert_eq!(rf(&r), doc.reference());
    }
}

// ------------------------------------------------------------------ AC-1 .. AC-3

#[test]
fn ac1_publish_returns_the_content_addressed_id_and_one_event() {
    let mut st = DprofState::new();
    let mut journal = Vec::new();
    let p = publish(&mut st, Principal::Human, A100).unwrap();
    journal.extend(p.event.clone());
    assert_eq!(p.reference.to_string(), exp_ref(A100));
    assert!(p.reference.to_string().starts_with("dp:asme-y14.5-2018@1.0.0#sha256:"));
    assert_eq!(p.status, PublishStatus::Created);
    assert_eq!(p.branch, Branch::Working);
    assert_eq!(journal.len(), 1);
    let DprofEvent::Publish(e) = &journal[0] else { panic!("event") };
    assert_eq!(e.file, handle(A100));
    assert_eq!(e.reference.to_string(), exp_ref(A100));
    // --json result.
    let json = String::from_utf8(fid_canon::to_canonical_bytes(&p.to_cvalue()).unwrap()).unwrap();
    assert_eq!(json, format!(r#"{{"branch":"working","dry_run":false,"ref":"{}","status":"created"}}"#, exp_ref(A100)));
}

#[test]
fn ac2_same_version_different_digest_is_profile_immutable_and_journals_nothing() {
    let mut st = DprofState::new();
    let mut journal: Vec<DprofEvent> = Vec::new();
    journal.extend(publish(&mut st, Principal::Human, A100).unwrap().event);
    let before = st.clone();
    let err = publish(&mut st, Principal::Human, A100_MOD).unwrap_err();
    assert_eq!(err.code(), "profile_immutable");
    assert_eq!(err.to_string(), "profile_immutable{dp:asme-y14.5-2018@1.0.0}");
    assert_eq!(journal.len(), 1);
    assert_eq!(st, before);
    // Uniqueness is per branch (C5): an agent may publish the modified 1.0.0 on its own
    // branch without touching the working branch, but not twice with different content.
    let agent = publish(&mut st, Principal::Agent, A100_MOD).unwrap();
    assert_eq!((agent.status, agent.branch), (PublishStatus::Created, Branch::Agent));
    assert_eq!(st.get(Branch::Working, agent.reference.id()).unwrap().reference().to_string(), exp_ref(A100));
    assert_eq!(publish(&mut st, Principal::Agent, A100).unwrap_err().code(), "profile_immutable");
    // Its adoption conflicts with the working branch's 1.0.0 and changes nothing.
    let before = st.clone();
    let err = st.adopt(Principal::Human, &agent.reference).unwrap_err();
    assert_eq!(err.code(), "adopt_conflict");
    assert_eq!(err.to_string(), "adopt_conflict{dp:asme-y14.5-2018@1.0.0}");
    assert_eq!(st, before);
    assert_eq!(st.resolve(Branch::Working, &agent.reference).unwrap_err().code(), "defaults_profile_unknown");
}

#[test]
fn agent_publish_never_blocks_a_later_human_publish() {
    let mut st = DprofState::new();
    let agent = publish(&mut st, Principal::Agent, A100_MOD).unwrap().reference;
    let human = publish(&mut st, Principal::Human, A100).unwrap();
    assert_eq!((human.status, human.branch), (PublishStatus::Created, Branch::Working));
    assert!(human.event.is_some());
    // The agent branch sees both digests of 1.0.0; the working branch only its own.
    assert!(st.resolve(Branch::Agent, &agent).is_ok());
    assert!(st.resolve(Branch::Agent, &human.reference).is_ok());
    assert!(st.resolve(Branch::Working, &agent).is_err());
}

#[test]
fn ac3_identical_republish_returns_existing_and_journals_nothing() {
    let mut st = DprofState::new();
    let mut journal: Vec<DprofEvent> = Vec::new();
    journal.extend(publish(&mut st, Principal::Human, A100).unwrap().event);
    let again = publish(&mut st, Principal::Human, A100).unwrap();
    assert_eq!(again.status, PublishStatus::Existing);
    assert_eq!(again.reference.to_string(), exp_ref(A100));
    assert!(again.event.is_none());
    journal.extend(again.event);
    assert_eq!(journal.len(), 1);
    assert_eq!(st.profile_count(), 1);
}

#[test]
fn req004_publishing_never_moves_existing_characteristics() {
    let mut st = DprofState::new();
    let r100 = publish(&mut st, Principal::Human, A100).unwrap().reference;
    st.bind(Principal::Human, &cid("c1"), Dialect::Asme, &r100).unwrap();
    publish(&mut st, Principal::Human, A110).unwrap();
    assert_eq!(st.binding(Branch::Working, &cid("c1")), Some(&r100));
}

// ------------------------------------------------------------------ AC-4, AC-5

fn three_chars_on_100() -> (DprofState, ProfileRef, ProfileRef, Vec<Vec<u8>>, DprofState) {
    let mut st = DprofState::new();
    let mut journal = Vec::new();
    let p = publish(&mut st, Principal::Human, A100).unwrap();
    journal.push(p.event.unwrap().canonical_bytes().unwrap());
    let r100 = p.reference;
    // The characteristics are created by GDT.CHAR.SPEC; its journal is not this leaf's.
    for c in ["c1", "c2", "c3"] {
        st.bind(Principal::Human, &cid(c), Dialect::Asme, &r100).unwrap();
    }
    let start_of_ac4 = st.clone();
    let p = publish(&mut st, Principal::Human, A110).unwrap();
    journal.push(p.event.unwrap().canonical_bytes().unwrap());
    (st, r100, p.reference, journal, start_of_ac4)
}

#[test]
fn ac4_repoint_changes_only_named_characteristics_in_one_event() {
    let (mut st, r100, r110, _, _) = three_chars_on_100();
    let out = st.repoint(Principal::Human, &["c2", "c1"], &r110.to_string(), false).unwrap();
    assert_eq!(st.binding(Branch::Working, &cid("c1")), Some(&r110));
    assert_eq!(st.binding(Branch::Working, &cid("c2")), Some(&r110));
    assert_eq!(st.binding(Branch::Working, &cid("c3")), Some(&r100));
    let Some(DprofEvent::Repoint(e)) = &out.event else { panic!("event") };
    assert_eq!(e.changes.len(), 2);
    assert_eq!(e.changes[0].char.as_str(), "c1");
    assert_eq!(e.changes[1].char.as_str(), "c2");
    assert!(e.changes.iter().all(|c| c.old == r100 && c.new == r110));
    let bytes = String::from_utf8(out.event.as_ref().unwrap().canonical_bytes().unwrap()).unwrap();
    let want = format!(
        r#"{{"branch":"working","changes":[{{"char":"c1","new":"{n}","old":"{o}"}},{{"char":"c2","new":"{n}","old":"{o}"}}],"command":"gdt.repoint_defaults_profile@1","principal":"human","to":"{n}"}}"#,
        n = exp_ref(A110),
        o = exp_ref(A100)
    );
    assert_eq!(bytes, want);
}

#[test]
fn ac5_dialect_mismatch_refuses_and_changes_nothing() {
    let (mut st, _, _, _, _) = three_chars_on_100();
    let iso = publish(&mut st, Principal::Human, I100).unwrap().reference;
    let before = st.clone();
    let err = st.repoint(Principal::Human, &["c1"], &iso.to_string(), false).unwrap_err();
    assert_eq!(err.code(), "dialect_mismatch");
    assert_eq!(st, before);
    // One mismatching characteristic refuses the whole request.
    let err = st.repoint(Principal::Human, &["c1", "c2", "c3"], &iso.to_string(), false).unwrap_err();
    assert_eq!(err.code(), "dialect_mismatch");
    assert_eq!(st, before);
}

#[test]
fn repoint_error_paths() {
    let (mut st, r100, r110, _, _) = three_chars_on_100();
    let to = r110.to_string();
    let before = st.clone();
    let code =
        |st: &mut DprofState, chars: &[&str], to: &str| st.repoint(Principal::Human, chars, to, false).unwrap_err();
    assert_eq!(code(&mut st, &[], &to).reason(), Some("no_chars"));
    assert_eq!(code(&mut st, &["c1", "c1"], &to).reason(), Some("duplicate_char"));
    assert_eq!(code(&mut st, &["bad id"], &to).reason(), Some("bad_char_id"));
    assert_eq!(code(&mut st, &["c1", "c9"], &to).code(), "char_unknown");
    assert_eq!(code(&mut st, &["c1"], "dp:asme-y14.5-2018@1.1.0").code(), "bad_profile_ref");
    // A well-formed reference with a wrong digest is unknown.
    let wrong = format!("{}#{}", r110.id(), r100.digest().as_str());
    assert_eq!(code(&mut st, &["c1"], &wrong).code(), "defaults_profile_unknown");
    let many: Vec<String> = (0..10_001).map(|i| format!("c{i}")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    assert_eq!(code(&mut st, &many, &to).reason(), Some("too_many_chars"));
    assert_eq!(st, before);
    // Dry run reports the changes and touches nothing.
    let dry = st.repoint(Principal::Human, &["c1"], &to, true).unwrap();
    assert!(dry.event.is_none());
    assert_eq!(dry.changes.len(), 1);
    assert_eq!(st, before);
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_agent_publish_lands_on_agent_branch_until_a_human_adopts_it() {
    let mut st = DprofState::new();
    let p = publish(&mut st, Principal::Agent, A100).unwrap();
    assert_eq!(p.branch, Branch::Agent);
    let Some(DprofEvent::Publish(e)) = &p.event else { panic!("event") };
    assert_eq!(e.principal, Principal::Agent);
    assert_eq!(e.branch, Branch::Agent);
    let stored = st.get(Branch::Agent, p.reference.id()).unwrap();
    assert_eq!(stored.author, Principal::Agent);
    assert_eq!(stored.branch, Branch::Agent);
    assert!(st.get(Branch::Working, p.reference.id()).is_none());
    // gdt.set_governing_spec@1 (GDT.CHAR.SPEC) resolves through DprofState::resolve.
    let err = st.resolve(Branch::Working, &p.reference).unwrap_err();
    assert_eq!(err.code(), "defaults_profile_unknown");
    let err = st.bind(Principal::Human, &cid("c1"), Dialect::Asme, &p.reference).unwrap_err();
    assert_eq!(err.code(), "defaults_profile_unknown");
    assert!(st.resolve(Branch::Agent, &p.reference).is_ok());
    // An identical agent re-publish is a no-op on the agent branch.
    let again = publish(&mut st, Principal::Agent, A100).unwrap();
    assert_eq!((again.status, again.branch), (PublishStatus::Existing, Branch::Agent));
    assert!(again.event.is_none());
    assert!(st.resolve(Branch::Working, &p.reference).is_err());
    // Adoption is human only.
    assert_eq!(st.adopt(Principal::Agent, &p.reference).unwrap_err().code(), "human_only");
    assert_eq!(st.adopt(Principal::Script, &p.reference).unwrap_err().code(), "human_only");
    st.adopt(Principal::Human, &p.reference).unwrap();
    assert!(st.resolve(Branch::Working, &p.reference).is_ok());
    let adopted = st.get(Branch::Working, p.reference.id()).unwrap();
    assert_eq!((adopted.author, adopted.adopted), (Principal::Agent, true));
    // Adopting again changes nothing; after adoption the version is taken on the working
    // branch, so a different human 1.0.0 is immutable there.
    let before = st.clone();
    st.adopt(Principal::Human, &p.reference).unwrap();
    assert_eq!(st, before);
    assert_eq!(publish(&mut st, Principal::Human, A100_MOD).unwrap_err().code(), "profile_immutable");
    // A working-branch-only reference cannot be adopted.
    let mut st2 = DprofState::new();
    let human = publish(&mut st2, Principal::Human, A110).unwrap().reference;
    assert_eq!(st2.adopt(Principal::Human, &human).unwrap_err().code(), "defaults_profile_unknown");
}

#[test]
fn agent_repoint_writes_the_agent_overlay_only() {
    let (mut st, r100, r110, _, _) = three_chars_on_100();
    let out = st.repoint(Principal::Agent, &["c1"], &r110.to_string(), false).unwrap();
    assert_eq!(out.branch, Branch::Agent);
    assert_eq!(st.binding(Branch::Working, &cid("c1")), Some(&r100));
    assert_eq!(st.binding(Branch::Agent, &cid("c1")), Some(&r110));
    assert_eq!(st.binding(Branch::Agent, &cid("c2")), Some(&r100));
    // A human cannot re-point onto an unadopted agent profile.
    let mut st2 = DprofState::new();
    let r = publish(&mut st2, Principal::Human, A100).unwrap().reference;
    st2.bind(Principal::Human, &cid("c1"), Dialect::Asme, &r).unwrap();
    let agent = publish(&mut st2, Principal::Agent, A110).unwrap().reference;
    let err = st2.repoint(Principal::Human, &["c1"], &agent.to_string(), false).unwrap_err();
    assert_eq!(err.code(), "defaults_profile_unknown");
    assert!(st2.repoint(Principal::Agent, &["c1"], &agent.to_string(), false).is_ok());
}

#[test]
fn agents_must_pass_handles() {
    let mut st = DprofState::new();
    let raw = fixtures().join(A100).to_string_lossy().into_owned();
    let err = publish_defaults_profile(&mut st, &roots(), Principal::Agent, &raw, &vocab(), false).unwrap_err();
    assert_eq!(err.code(), "raw_path_forbidden");
    let err =
        publish_defaults_profile(&mut st, &roots(), Principal::Agent, "root://imports/../x.json", &vocab(), false)
            .unwrap_err();
    assert_eq!(err.code(), "path_outside_root");
    assert_eq!(st.profile_count(), 0);
}

#[test]
fn dry_run_publish_stores_and_journals_nothing() {
    let mut st = DprofState::new();
    let p = publish_defaults_profile(&mut st, &roots(), Principal::Human, &handle(A100), &vocab(), true).unwrap();
    assert_eq!(p.status, PublishStatus::Created);
    assert!(p.event.is_none());
    assert_eq!(p.reference.to_string(), exp_ref(A100));
    assert_eq!(st.profile_count(), 0);
}

// ------------------------------------------------------------------ AC-7

#[test]
fn ac7_exact_replay_of_ac1_and_ac4_is_identical() {
    // Journal: AC-1 publish of 1.0.0, then (after GDT.CHAR.SPEC bound c1..c3, outside this
    // leaf's journal) the 1.1.0 publish and the AC-4 re-point.
    let (mut st, _, r110, mut journal, start_of_ac4) = three_chars_on_100();
    let r = st.repoint(Principal::Human, &["c1", "c2"], &r110.to_string(), false).unwrap();
    journal.push(r.event.unwrap().canonical_bytes().unwrap());
    let content: BTreeMap<Digest, Vec<u8>> =
        [A100, A110].iter().map(|f| (*parse_profile(&read(f), &vocab()).unwrap().digest(), read(f))).collect();

    // AC-1's event from an empty store.
    let out = replay(&DprofState::new(), &journal[..1], &content, &vocab());
    assert_eq!(out.outcome, ReplayOutcome::Identical);
    assert_eq!(out.steps, vec![ReplayOutcome::Identical]);
    let mut after_ac1 = out.state;
    let r100 = after_ac1.profiles().next().unwrap().reference();
    for c in ["c1", "c2", "c3"] {
        after_ac1.bind(Principal::Human, &cid(c), Dialect::Asme, &r100).unwrap();
    }
    assert_eq!(after_ac1, start_of_ac4);

    // AC-4's events from the state the characteristics were bound in.
    let out = replay(&start_of_ac4, &journal[1..], &content, &vocab());
    assert_eq!(out.outcome, ReplayOutcome::Identical);
    assert_eq!(out.outcome.as_str(), "IDENTICAL");
    assert_eq!(out.steps, vec![ReplayOutcome::Identical; 2]);
    assert_eq!(out.state, st);
}

#[test]
fn replay_detects_divergence_and_missing_content() {
    let (full, _, _, journal, start) = three_chars_on_100();
    // Missing content.
    let empty: BTreeMap<Digest, Vec<u8>> = BTreeMap::new();
    let out = replay(&start, &journal[1..], &empty, &vocab());
    assert_eq!(out.outcome, ReplayOutcome::NotReplayable);
    // Content whose digest differs from the recorded reference.
    let wrong: BTreeMap<Digest, Vec<u8>> =
        [(*parse_profile(&read(A110), &vocab()).unwrap().digest(), read(A100))].into_iter().collect();
    let out = replay(&start, &journal[1..], &wrong, &vocab());
    assert_eq!(out.outcome, ReplayOutcome::Divergent);
    // A tampered event.
    let mut bad = journal[1].clone();
    let pos = bad.iter().position(|&b| b == b'h').unwrap();
    bad[pos] = b'H';
    let out = replay(&start, &[bad], &full, &vocab());
    assert_eq!(out.outcome, ReplayOutcome::NotReplayable);
    // A step whose re-execution now fails (1.1.0 already present) diverges, and replay stops.
    let out = replay(&full, &journal, &full, &vocab());
    assert_eq!(out.outcome, ReplayOutcome::Divergent);
    assert_eq!(out.steps, vec![ReplayOutcome::Divergent]);
    // The store itself serves content by digest.
    let out = replay(&DprofState::new(), &journal, &full, &vocab());
    assert_eq!(out.outcome, ReplayOutcome::Identical);
}

// ------------------------------------------------------------------ events and snapshot

#[test]
fn events_round_trip_strictly() {
    let (mut st, _, r110, journal, _) = three_chars_on_100();
    let r = st.repoint(Principal::Human, &["c3"], &r110.to_string(), false).unwrap();
    let mut all = journal.clone();
    all.push(r.event.unwrap().canonical_bytes().unwrap());
    for bytes in &all {
        let e = DprofEvent::from_canonical_bytes(bytes).unwrap();
        assert_eq!(&e.canonical_bytes().unwrap(), bytes);
    }
    let text = String::from_utf8(all[0].clone()).unwrap();
    let bad = |t: String| DprofEvent::from_canonical_bytes(t.as_bytes()).unwrap_err().reason().unwrap();
    assert_eq!(bad(text.replace("\"principal\":\"human\"", "\"principal\":\"agent\"")), "branch_principal_mismatch");
    assert_eq!(bad(text.replace("publish_defaults", "publish_default")), "unknown_command");
    assert_eq!(bad(text.replace("\"file\"", "\"path\"")), "bad_keys");
    assert_eq!(bad(text.replace("dp:asme", "dp:gps")), "bad_ref");
    assert_eq!(bad("[]".to_owned()), "wrong_type");
}

#[test]
fn snapshot_round_trips_and_is_checked_on_load() {
    let (mut st, _, r110, _, _) = three_chars_on_100();
    publish(&mut st, Principal::Agent, I100).unwrap();
    st.repoint(Principal::Agent, &["c2"], &r110.to_string(), false).unwrap();
    let bytes = st.to_canonical_bytes().unwrap();
    let back = DprofState::from_canonical_bytes(&bytes, &vocab()).unwrap();
    assert_eq!(back, st);
    assert_eq!(back.to_canonical_bytes().unwrap(), bytes);
    let text = String::from_utf8(bytes).unwrap();
    let load = |t: &str, v: &KeyVocabulary| DprofState::from_canonical_bytes(t.as_bytes(), v).unwrap_err();
    assert_eq!(load(&text.replace("variant-a", "variant-z"), &vocab()).reason(), Some("digest_mismatch"));
    let human = text.replace("\"author\":\"agent\"", "\"author\":\"human\"");
    assert_eq!(load(&human, &vocab()).reason(), Some("branch_author_mismatch"));
    assert_eq!(load(&text.replace("fid.dprof-store/1", "fid.dprof-store/2"), &vocab()).reason(), Some("bad_schema"));
    let small = KeyVocabulary::new(["placeholder.synthetic"]).unwrap();
    assert_eq!(load(&text, &small).code(), "unknown_key");
    // A non-integer literal anywhere in a snapshot is refused at the source text.
    assert_eq!(load(&text.replacen("\"adopted\":false", "\"adopted\":1.0", 1), &vocab()).reason(), Some("bad_number"));
}

#[test]
fn snapshot_with_adopted_profile_round_trips() {
    let mut st = DprofState::new();
    let r = publish(&mut st, Principal::Agent, A100).unwrap().reference;
    st.adopt(Principal::Human, &r).unwrap();
    st.bind(Principal::Human, &cid("c1"), Dialect::Asme, &r).unwrap();
    let bytes = st.to_canonical_bytes().unwrap();
    assert_eq!(DprofState::from_canonical_bytes(&bytes, &vocab()).unwrap(), st);
    // An adopted flag on a human's own working entry is inconsistent.
    let mut human = DprofState::new();
    publish(&mut human, Principal::Human, A100).unwrap();
    let text = String::from_utf8(human.to_canonical_bytes().unwrap()).unwrap();
    let bad = text.replace("\"adopted\":false", "\"adopted\":true");
    let err = DprofState::from_canonical_bytes(bad.as_bytes(), &vocab()).unwrap_err();
    assert_eq!(err.reason(), Some("branch_author_mismatch"));
}

#[test]
fn snapshot_writer_refuses_what_the_loader_would_refuse() {
    let (st, _, _, _, _) = three_chars_on_100();
    let bytes = st.to_canonical_bytes().unwrap();
    assert!(bytes.len() <= fid_gdt::chars::dprof::store::MAX_STORE_BYTES);
    assert_eq!(st.to_canonical_bytes_capped(bytes.len()).unwrap(), bytes);
    let err = st.to_canonical_bytes_capped(bytes.len() - 1).unwrap_err();
    assert_eq!((err.code(), err.reason()), ("store_invalid", Some("too_large")));
    // The cap never exceeds the loader's limit.
    assert_eq!(st.to_canonical_bytes_capped(usize::MAX).unwrap(), bytes);
}

#[test]
fn non_integer_literals_are_refused_as_the_python_reference_refuses_them() {
    let v = fid_canon::parse(&read("expected.json")).unwrap();
    let CValue::Obj(o) = v else { panic!("expected") };
    let Some(CValue::Arr(refused)) = o.get("refused") else { panic!("refused") };
    assert_eq!(refused.len(), 3);
    for c in refused {
        let CValue::Obj(c) = c else { panic!("case") };
        let Some(CValue::Str(file)) = c.get("file") else { panic!("file") };
        let Some(CValue::Str(why)) = c.get("refused") else { panic!("refused") };
        let err = parse_profile(&read(file), &vocab()).unwrap_err();
        assert_eq!((err.code(), err.reason()), ("malformed_profile", Some(why.as_str())), "{file}");
    }
    let m = |t: &str| parse_err(&doc(t)).reason();
    for lit in ["1.0", "1e2", "1E2", "1e+2", "-0.0", "7.0000000000000001", "100e-2"] {
        assert_eq!(m(&format!(r#"{{"placeholder.count":{lit}}}"#)), Some("bad_number"), "{lit}");
        assert_eq!(m(&format!(r#"{{"placeholder.list":[1,{lit}]}}"#)), Some("bad_number"), "{lit}");
    }
    // Digits, dots and exponents inside strings are not number literals.
    assert!(parse_profile(doc(r#"{"placeholder.note":"1.0e2 \"2.5\" \\ 3E4"}"#).as_bytes(), &vocab()).is_ok());
    assert!(parse_profile(doc(r#"{"placeholder.count":-0,"placeholder.list":[10,-20]}"#).as_bytes(), &vocab()).is_ok());
}

// ------------------------------------------------------------------ profile parsing

fn doc(keys: &str) -> String {
    let head = r#""schema":"fid.defaults-profile/1","dialect":"asme","edition":"y14.5-2018","version":"1.0.0""#;
    format!("{{{head},\"keys\":{keys}}}")
}

fn parse_err(text: &str) -> DprofError {
    parse_profile(text.as_bytes(), &vocab()).unwrap_err()
}

#[test]
fn malformed_profiles_are_refused() {
    let m = |t: &str| parse_err(t).reason().unwrap();
    assert_eq!(m(&doc(r#"{"placeholder.count":1.5}"#)), "bad_number");
    assert_eq!(m(&doc(r#"{"placeholder.count":9007199254740992}"#)), "bad_number");
    assert_eq!(m(&doc(r#"{"placeholder.count":null}"#)), "bad_value");
    assert_eq!(m(&doc(r#"{"placeholder.count":{"a":"b"}}"#)), "bad_value");
    assert_eq!(m(&doc(r#"{"placeholder.list":[["a"]]}"#)), "input_limit");
    assert_eq!(m(&doc(r#"{"placeholder.list":[null]}"#)), "bad_value");
    assert_eq!(m(&doc(r#"{"Placeholder.count":1}"#)), "bad_key");
    assert_eq!(m(&doc(r#"{"placeholder.count":1,"placeholder.count":2}"#)), "duplicate_key");
    assert_eq!(m(&doc(r#"[]"#)), "wrong_type");
    assert_eq!(m(&doc("{}").replace("\"keys\"", "\"extra\":1,\"keys\"")), "unknown_field");
    assert_eq!(m(&doc("{}").replace(",\"keys\":{}", "")), "missing_field");
    assert_eq!(m(&doc("{}").replace("/1", "/2")), "bad_schema");
    assert_eq!(m(&doc("{}").replace("\"asme\"", "\"gps\"")), "bad_dialect");
    assert_eq!(m(&doc("{}").replace("y14.5-2018", "Y14.5")), "bad_edition");
    assert_eq!(m(&doc("{}").replace("1.0.0", "1.0.0-rc1")), "bad_version");
    assert_eq!(m(&doc("{}").replace("1.0.0", "01.0.0")), "bad_version");
    assert_eq!(m("{"), "syntax");
    let long = "x".repeat(281);
    assert_eq!(m(&doc(&format!(r#"{{"placeholder.note":"{long}"}}"#))), "text_too_long");
    let ok280 = "\u{e9}".repeat(280);
    assert!(parse_profile(doc(&format!(r#"{{"placeholder.note":"{ok280}"}}"#)).as_bytes(), &vocab()).is_ok());
    let list = vec!["\"a\""; 257].join(",");
    assert_eq!(m(&doc(&format!(r#"{{"placeholder.list":[{list}]}}"#))), "list_too_long");
    let big = vec![b' '; fid_gdt::chars::dprof::profile::MAX_PROFILE_BYTES + 1];
    assert_eq!(parse_profile(&big, &vocab()).unwrap_err().reason(), Some("too_large"));
    // Structure is checked before the vocabulary.
    assert_eq!(parse_err(&doc(r#"{"zz.unknown":1,"placeholder.count":1.5}"#)).code(), "malformed_profile");
}

#[test]
fn unknown_keys_are_refused_and_named() {
    let e = parse_err(&doc(r#"{"zz.unknown":1,"aa.unknown":true,"placeholder.count":1}"#));
    assert_eq!(e.code(), "unknown_key");
    assert_eq!(e.to_string(), "unknown_key{aa.unknown}");
    assert!(parse_profile(doc("{}").as_bytes(), &vocab()).is_ok());
    assert!(parse_profile(doc(r#"{"placeholder.count":-9007199254740991}"#).as_bytes(), &vocab()).is_ok());
}

#[test]
fn vocabulary_is_validated() {
    assert_eq!(KeyVocabulary::new(["a", "a"]).unwrap_err().reason(), Some("duplicate_key"));
    assert_eq!(KeyVocabulary::new(["A"]).unwrap_err().reason(), Some("bad_key"));
    assert_eq!(KeyVocabulary::new([""]).unwrap_err().code(), "bad_vocabulary");
}

// ------------------------------------------------------------------ identifiers

#[test]
fn reference_grammar() {
    let d = "0".repeat(64);
    let ok = format!("dp:asme-y14.5-2018@1.10.0#sha256:{d}");
    let r = rf(&ok);
    assert_eq!(r.to_string(), ok);
    assert_eq!(r.dialect(), Dialect::Asme);
    assert_eq!(r.id().edition().as_str(), "y14.5-2018");
    assert_eq!(r.id().version(), SemVer::new(1, 10, 0).unwrap());
    for bad in [
        format!("dp:asme-y14.5-2018@1.0#sha256:{d}"),
        format!("dp:asme-y14.5-2018@1.0.0.0#sha256:{d}"),
        format!("dp:asme-y14.5-2018@1.0.0#sha256:{}", "A".repeat(64)),
        format!("dp:asme-y14.5-2018@1.0.0#sha256:{}", "0".repeat(63)),
        format!("dp:asmey14.5-2018@1.0.0#sha256:{d}"),
        format!("dp:ASME-y14.5-2018@1.0.0#sha256:{d}"),
        format!("dp:asme-y14..5@1.0.0#sha256:{d}"),
        format!("dp:asme-@1.0.0#sha256:{d}"),
        format!("dq:asme-y14.5-2018@1.0.0#sha256:{d}"),
        format!("dp:asme-y14.5-2018@1.0.0#sha512:{d}"),
        format!("dp:asme-y14.5-2018@1.0.0#sha256:{d}#"),
        format!("dp:asme-y14.5-2018@1.0.0@1#sha256:{d}"),
        format!("dp:asme-y14.5-2018@1234567890.0.0#sha256:{d}"),
        "dp:".to_owned(),
        String::new(),
    ] {
        assert!(ProfileRef::parse(&bad).is_none(), "{bad}");
    }
    assert!(Edition::parse("1101-2017").is_some());
    assert!(Edition::parse("-2017").is_none());
    assert!(Edition::parse("2017.").is_none());
    assert!(Edition::parse(&"a".repeat(33)).is_none());
    assert!(SemVer::parse("999999999.0.0").is_some());
    assert!(SemVer::new(1, 2, 0).unwrap() < SemVer::new(1, 10, 0).unwrap());
    assert!(CharId::parse("qpid:5f0e-AB_1.x").is_some());
    assert!(CharId::parse("-c1").is_none());
    assert!(CharId::parse(&"c".repeat(129)).is_none());
}

#[test]
fn errors_display_their_code() {
    let e = DprofError::BadRequest { reason: "no_chars" };
    assert_eq!(e.to_string(), "bad_request{no_chars}");
    assert_eq!(DprofError::HumanOnly.to_string(), "human_only");
    let long = "x".repeat(1000);
    let e = DprofState::new().resolve_text(Branch::Working, &long).unwrap_err();
    assert_eq!(e.code(), "bad_profile_ref");
}

// ------------------------------------------------------------------ vocabulary from the DMT schema

fn dmt_table() -> fid_gps::dmt::DmtTable {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/dmt/synthetic_table_v1.json");
    fid_gps::dmt::DmtTable::load(&std::fs::read(path).unwrap()).unwrap()
}

fn iso_doc(keys: &str) -> String {
    let head = r#""schema":"fid.defaults-profile/1","dialect":"iso","edition":"1101-2017","version":"1.0.0""#;
    format!("{{{head},\"keys\":{keys}}}")
}

#[test]
fn schema_vocabulary_comes_from_the_dmt_table() {
    // Hand-labelled from GDT.RES.SCHEMA's rules: the named keys plus every ASME row of the
    // synthetic table with two or more variants.
    let v = KeyVocabulary::from_dmt_table(&dmt_table(), Dialect::Asme).unwrap();
    assert_eq!(v.dialect(), Some(Dialect::Asme));
    for k in fid_gps::dmt::NAMED_PROFILE_KEYS.iter().map(|(k, _)| *k) {
        assert!(v.contains(k), "{k}");
    }
    assert!(v.contains("dmt:asme/position/none@1"));
    assert!(v.contains("dmt:asme/size/none@1"));
    assert!(!v.contains("dmt:asme/flatness/none@1"), "one variant: nothing to choose");
    assert!(!v.contains("dmt:iso/flatness/none@1"));
    assert!(!v.contains("placeholder.synthetic"));

    let ok = doc(
        r#"{"dmt:asme/position/none@1":"axis","dmt:asme/size/none@1":"local","unc.decision_rule_hint":"dr.synthetic.default@1"}"#,
    );
    let parsed = parse_profile(ok.as_bytes(), &v).unwrap();
    assert_eq!(parsed.profile().keys().len(), 3);

    let err = |t: &str| parse_profile(t.as_bytes(), &v).unwrap_err();
    // A key outside the schema vocabulary.
    let e = err(&doc(r#"{"placeholder.synthetic":true}"#));
    assert_eq!(e, DprofError::UnknownKey { key: "placeholder.synthetic".to_owned() });
    let e = err(&doc(r#"{"dmt:asme/flatness/none@1":"default"}"#));
    assert_eq!(e.code(), "unknown_key");
    // A key differing only in case is unknown, as is a key with modifiers out of order.
    assert_eq!(err(&doc(r#"{"UNC.decision_rule_hint":"x"}"#)).code(), "malformed_profile");
    assert_eq!(err(&doc(r#"{"dmt:asme/position/mmc+cz@1":"axis"}"#)).code(), "unknown_key");
    // A variant the row lacks, or a non-text value.
    let m = |t: &str| {
        let e = err(t);
        (e.code(), e.reason())
    };
    assert_eq!(m(&doc(r#"{"dmt:asme/position/none@1":"median"}"#)), ("malformed_profile", Some("value_not_allowed")));
    assert_eq!(m(&doc(r#"{"dmt:asme/position/none@1":"Axis"}"#)), ("malformed_profile", Some("value_not_allowed")));
    assert_eq!(m(&doc(r#"{"dmt:asme/position/none@1":true}"#)), ("malformed_profile", Some("value_not_allowed")));
    assert_eq!(m(&doc(r#"{"unc.decision_rule_hint":7}"#)), ("malformed_profile", Some("value_not_allowed")));
    assert_eq!(m(&doc(r#"{"unc.decision_rule_hint":["a"]}"#)), ("malformed_profile", Some("value_not_allowed")));
    // A profile of another dialect.
    assert_eq!(m(&iso_doc("{}")), ("malformed_profile", Some("vocabulary_dialect")));
    // The ISO vocabulary of the synthetic table is the named keys only.
    let iso = KeyVocabulary::from_dmt_table(&dmt_table(), Dialect::Iso).unwrap();
    assert!(parse_profile(iso_doc(r#"{"gdt.datum.label_pattern":"[a-z]"}"#).as_bytes(), &iso).is_ok());
    let e = parse_profile(iso_doc(r#"{"dmt:iso/flatness/none@1":"placeholder"}"#).as_bytes(), &iso).unwrap_err();
    assert_eq!(e.code(), "unknown_key");
}

#[test]
fn publish_checks_against_the_schema_vocabulary() {
    // The fixture profiles use placeholder keys, which the schema vocabulary does not hold.
    let v = KeyVocabulary::from_dmt_table(&dmt_table(), Dialect::Asme).unwrap();
    let mut st = DprofState::new();
    let e = publish_defaults_profile(&mut st, &roots(), Principal::Human, &handle(A100), &v, false).unwrap_err();
    assert_eq!(e.code(), "unknown_key");
    assert_eq!(st, DprofState::new());
}

#[test]
fn structure_is_checked_before_the_vocabulary_dialect() {
    // Hand-labelled: each fixture is an ISO profile checked against the ASME schema vocabulary
    // (wrong dialect) and is also structurally broken; the structural error wins.
    let v = KeyVocabulary::from_dmt_table(&dmt_table(), Dialect::Asme).unwrap();
    for (file, why) in
        [("iso_bad_edition_vs_asme_vocab.json", "bad_edition"), ("iso_bad_value_vs_asme_vocab.json", "bad_value")]
    {
        let e = parse_profile(&read(file), &v).unwrap_err();
        assert_eq!((e.code(), e.reason()), ("malformed_profile", Some(why)), "{file}");
    }
    // Bad version and a bad key likewise come before the dialect check.
    let iso =
        |rest: &str| format!(r#"{{"schema":"fid.defaults-profile/1","dialect":"iso","edition":"1101-2017",{rest}}}"#);
    let e = parse_profile(iso(r#""version":"1.0","keys":{}"#).as_bytes(), &v).unwrap_err();
    assert_eq!(e.reason(), Some("bad_version"));
    let e = parse_profile(iso(r#""version":"1.0.0","keys":{"Bad":"x"}"#).as_bytes(), &v).unwrap_err();
    assert_eq!(e.reason(), Some("bad_key"));
    // Structurally sound, wrong dialect: the dialect check fires before the unknown key.
    let e = parse_profile(iso(r#""version":"1.0.0","keys":{"zz.unknown":"x"}"#).as_bytes(), &v).unwrap_err();
    assert_eq!(e.reason(), Some("vocabulary_dialect"));
}
