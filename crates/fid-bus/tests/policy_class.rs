//! Acceptance criteria of spec node CORE.BUS.POL.CLASS (AC-1 .. AC-4, REQ-005), against
//! `fixtures/core/bus/pol/class/matrix.csv` and `schema/policy-matrix.json`, both written by
//! the independent reference `fixtures/core/bus/pol/class/make_matrix.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Code, PolicyReason};
use fid_bus::policy::class::{
    authorise_cancel, authorise_class, class_rule, policy_matrix_json, policy_matrix_value, ClassRule,
    MATRIX_SCHEMA_VERSION,
};
use fid_bus::principal::{
    Channel, ChannelKind, ConfirmMethod, HumanPresence, Identity, JournalAppend, OsUser, PresenceOutcome, Principal,
    PrincipalId, PrincipalKind, PrincipalRef, SessionId, SessionKey, SessionTable, UpgradeClock, UpgradeContext,
    UpgradeGrant, UpgradeRequest,
};
use fid_canon::{parse, to_canonical_bytes, CValue, Digest};
use fid_journal::{EntryDraft, EntryTime, Manifests};
use fid_registry::CommandDef;
use fid_types::SideEffect;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

// ------------------------------------------------------------------ principals and commands

struct TestChannel {
    kind: ChannelKind,
    identity: Identity,
    session: SessionKey,
    grant: Option<UpgradeGrant>,
}

impl Channel for TestChannel {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    fn authenticated_identity(&self) -> Option<Identity> {
        Some(self.identity.clone())
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
    fn upgrade_grant(&self) -> Option<&UpgradeGrant> {
        self.grant.as_ref()
    }
}

fn pid(s: &str) -> PrincipalId {
    PrincipalId::new(s).unwrap()
}

fn channel(table: &mut SessionTable, kind: ChannelKind, subject: &str) -> TestChannel {
    let mut identity = Identity::new(pid(subject));
    identity.os_user = Some(OsUser::new(subject).unwrap());
    if matches!(kind, ChannelKind::ServerSession | ChannelKind::Internal) {
        identity.acting_for = Some(PrincipalRef { kind: PrincipalKind::Human, id: pid("trigger") });
    }
    let session = table.open_session(SessionId::new(&format!("{}-{subject}", kind.as_str())).unwrap()).unwrap();
    TestChannel { kind, identity, session, grant: None }
}

/// A principal of `kind`, derived by the bus from a channel of that kind (the only way to
/// build one).
fn principal_as(table: &mut SessionTable, kind: PrincipalKind, subject: &str) -> Principal {
    let ch = match kind {
        PrincipalKind::Human => ChannelKind::Gui,
        PrincipalKind::Agent => ChannelKind::Mcp,
        PrincipalKind::Script => ChannelKind::Cli,
        PrincipalKind::Service => ChannelKind::Service,
        PrincipalKind::Device => ChannelKind::Device,
        PrincipalKind::System => ChannelKind::Internal,
    };
    let c = channel(table, ch, subject);
    let p = table.derive_principal(&c).unwrap();
    assert_eq!(p.kind(), kind);
    p
}

/// A test command definition of class `class` with the given `human_only` flag.
fn test_def(id: &str, class: &str, human_only: bool) -> CommandDef {
    let json = format!(
        r#"{{"id":"{id}","major":1,"title":"Test command","description":"Policy class test command.",
"params_schema":{{"type":"object"}},"result_schema":{{"type":"object"}},"side_effect":"{class}",
"mcp_projection":"never","determinism":"D0","idempotency":"none","fs_effect":"none","undo":"head_move",
"preconditions":[],"effects":["TestDone"],"algorithms":[],"exposure":["cli"],
"lifecycle":{{"state":"active","since":"2026-01-01"}},"web":"available","human_only":{human_only}}}"#
    );
    let def = CommandDef::from_value(parse(json.as_bytes()).unwrap()).unwrap();
    assert_eq!((def.side_effect.as_str(), def.human_only), (class, human_only));
    def
}

fn assert_policy(r: Result<(), BusError>, reason: &str) {
    let e = r.unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Policy, reason), "{e}");
}

// ------------------------------------------------------------------ AC-1

struct Row {
    principal: String,
    class: String,
    human_only: bool,
    expected: String,
}

fn matrix_rows() -> Vec<Row> {
    let text = String::from_utf8(read("fixtures/core/bus/pol/class/matrix.csv")).unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("principal,class,human_only,expected"));
    lines
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            assert_eq!(f.len(), 4, "{l}");
            let human_only = match f[2] {
                "true" => true,
                "false" => false,
                other => panic!("human_only {other:?}"),
            };
            Row { principal: f[0].into(), class: f[1].into(), human_only, expected: f[3].into() }
        })
        .collect()
}

fn expected_of(rule: ClassRule) -> String {
    match rule {
        ClassRule::Allow => "allow".into(),
        ClassRule::Refuse(r) => format!("policy/{}", r.as_str()),
    }
}

/// AC-1, REQ-005: the fixture has one row for every combination of this build's principal
/// kinds, side-effect classes and `human_only` values, and `authorise_class` (and the bare
/// table) gives the expected outcome for each.
#[test]
fn ac1_matrix_matches_fixture() {
    let rows = matrix_rows();
    assert_eq!(rows.len(), 132);
    assert_eq!(PrincipalKind::ALL.len() * SideEffect::ALL.len() * 2, 132);
    let keys: BTreeSet<(String, String, bool)> =
        rows.iter().map(|r| (r.principal.clone(), r.class.clone(), r.human_only)).collect();
    assert_eq!(keys.len(), 132, "duplicate rows");
    for k in PrincipalKind::ALL {
        for c in SideEffect::ALL {
            for h in [false, true] {
                assert!(keys.contains(&(k.as_str().to_string(), c.as_str().to_string(), h)), "{k} {c} {h}");
            }
        }
    }

    let mut table = SessionTable::new();
    let mut allowed = 0;
    for (i, row) in rows.iter().enumerate() {
        let kind = PrincipalKind::parse(&row.principal).unwrap();
        let class = SideEffect::parse(&row.class).unwrap();
        assert_eq!(expected_of(class_rule(kind, class, row.human_only)), row.expected, "row {i}");

        let p = principal_as(&mut table, kind, &format!("p{i}"));
        let def = test_def("test.policy_cell", &row.class, row.human_only);
        let got = match authorise_class(&p, &def) {
            Ok(()) => "allow".to_string(),
            Err(e) => {
                assert_eq!(e.code(), Code::Policy, "row {i}");
                format!("policy/{}", e.reason())
            }
        };
        assert_eq!(got, row.expected, "row {i}: {} {} human_only={}", row.principal, row.class, row.human_only);
        allowed += usize::from(got == "allow");
    }
    // 22 human cells + 5 kinds x 7 permitted classes without human_only.
    assert_eq!(allowed, 22 + 5 * 7);
}

/// Hand-labelled from CONSTITUTION §3 and D-031, independent of the fixture: the four
/// human-only classes and the human-only flag.
#[test]
fn constitution_bans_hand_labelled() {
    let banned = [SideEffect::M1, SideEffect::S1, SideEffect::W3, SideEffect::A0];
    for k in PrincipalKind::ALL {
        for c in SideEffect::ALL {
            for h in [false, true] {
                let want = if *k == PrincipalKind::Human {
                    ClassRule::Allow
                } else if banned.contains(c) {
                    ClassRule::Refuse(PolicyReason::ClassForbidden)
                } else if h {
                    ClassRule::Refuse(PolicyReason::HumanOnly)
                } else {
                    ClassRule::Allow
                };
                assert_eq!(class_rule(*k, *c, h), want, "{k} {c} {h}");
            }
        }
    }
}

/// REQ-005: the published `schema/policy-matrix.json` (written by `make_matrix.py`) equals
/// this build's matrix byte for byte.
#[test]
fn published_matrix_matches() {
    let mut want = policy_matrix_json().unwrap();
    want.push(b'\n');
    let have = read("schema/policy-matrix.json");
    assert!(have == want, "schema/policy-matrix.json differs from the Rust table:\n{}", String::from_utf8_lossy(&want));
    assert_eq!(to_canonical_bytes(&policy_matrix_value()).unwrap(), policy_matrix_json().unwrap());
    let CValue::Obj(o) = policy_matrix_value() else { panic!("object expected") };
    assert_eq!(o.get("schema_version"), Some(&CValue::Num(f64::from(MATRIX_SCHEMA_VERSION))));
}

// ------------------------------------------------------------------ AC-2

/// The pipeline stand-in: the class check runs first and a refusal returns before
/// anything is appended.
fn invoke(principal: &Principal, def: &CommandDef, journal: &mut Vec<String>) -> Result<(), BusError> {
    authorise_class(principal, def)?;
    journal.push(format!("{}@{}", def.id, def.major));
    Ok(())
}

/// AC-2: an agent calling a W3 test command gets `policy/class_forbidden`; nothing is
/// journaled. Same for M1, S1 and A0, with or without `human_only`.
#[test]
fn ac2_agent_w3_refused() {
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let mut journal = Vec::new();
    let e = invoke(&agent, &test_def("test.release_record", "W3", false), &mut journal).unwrap_err();
    assert_eq!(e, BusError::Policy(PolicyReason::ClassForbidden, e.detail().clone()));
    assert!(e.detail().as_untrusted_str().contains("test.release_record@1"));
    for class in ["W3", "M1", "S1", "A0"] {
        for h in [false, true] {
            assert_policy(invoke(&agent, &test_def("test.forbidden", class, h), &mut journal), "class_forbidden");
        }
    }
    assert!(journal.is_empty());
    // Control: a W2 command without human_only goes through.
    invoke(&agent, &test_def("test.touch_value", "W2", false), &mut journal).unwrap();
    assert_eq!(journal, vec!["test.touch_value@1".to_string()]);
}

// ------------------------------------------------------------------ AC-3

struct Confirms(OsUser);

impl HumanPresence for Confirms {
    fn confirm(&self, _req: &UpgradeRequest, _deadline: Duration) -> PresenceOutcome {
        PresenceOutcome::Confirmed { os_user: self.0.clone(), method: ConfirmMethod::OsPresence }
    }
}

struct StepClock(u64);

impl UpgradeClock for StepClock {
    fn now(&mut self) -> EntryTime {
        self.0 += 1_000_000;
        EntryTime { utc: "2026-11-19T10:00:00Z".into(), mono_ns: self.0, sync: "ntp".into() }
    }
}

#[derive(Default)]
struct CountingJournal(usize);

impl JournalAppend for CountingJournal {
    fn append(&mut self, _draft: &EntryDraft) -> Result<(), BusError> {
        self.0 += 1;
        Ok(())
    }
}

/// AC-3: a script calling a W2 `human_only` command gets `policy/human_only`; after the
/// CLI session is upgraded to human (D-012), the same call on the same channel is allowed.
#[test]
fn ac3_human_only_and_upgrade() {
    let mut table = SessionTable::new();
    let mut ch = channel(&mut table, ChannelKind::Cli, "alice");
    let def = test_def("test.adopt_choice", "W2", true);
    let mut journal = Vec::new();

    let script = table.derive_principal(&ch).unwrap();
    assert_eq!(script.kind(), PrincipalKind::Script);
    assert_policy(invoke(&script, &def, &mut journal), "human_only");
    assert!(journal.is_empty());

    let pending = table.prepare_upgrade(&ch).unwrap();
    let confirmed = pending.confirm(&Confirms(OsUser::new("alice").unwrap()), &mut StepClock(1_000)).unwrap();
    let ctx = UpgradeContext {
        stream: "record".into(),
        branch: "main".into(),
        manifests: Manifests { registry: Digest([0x11; 32]), kernel: Digest([0x22; 32]) },
    };
    let mut upgrade_journal = CountingJournal::default();
    ch.grant = Some(table.commit_upgrade(confirmed, &mut upgrade_journal, &ctx).unwrap());
    assert_eq!(upgrade_journal.0, 1);

    let human = table.derive_principal(&ch).unwrap();
    assert_eq!(human.kind(), PrincipalKind::Human);
    invoke(&human, &def, &mut journal).unwrap();
    assert_eq!(journal.len(), 1);
    // The upgraded human may also release (W3, human_only) where the script could not.
    let release = test_def("test.release_record", "W3", true);
    assert_policy(authorise_class(&script, &release), "class_forbidden");
    authorise_class(&human, &release).unwrap();
}

/// D-031 for every non-human kind and every class that is not banned outright.
#[test]
fn human_only_refuses_every_non_human_kind() {
    let mut table = SessionTable::new();
    for k in PrincipalKind::ALL {
        let p = principal_as(&mut table, *k, &format!("who-{}", k.as_str()));
        for class in ["R0", "R1", "W1", "W2", "X1", "I1", "D1"] {
            let r = authorise_class(&p, &test_def("test.manual_reading", class, true));
            if *k == PrincipalKind::Human {
                r.unwrap();
            } else {
                assert_policy(r, "human_only");
            }
        }
    }
}

// ------------------------------------------------------------------ AC-4

/// AC-4: job J of script principal S. Agent A cancelling J gets `policy/not_job_owner`;
/// S and a human may cancel it. The owner is the job's recorded principal.
#[test]
fn ac4_cancel_rule() {
    let mut table = SessionTable::new();
    let s = principal_as(&mut table, PrincipalKind::Script, "s");
    let a = principal_as(&mut table, PrincipalKind::Agent, "a");
    let h = principal_as(&mut table, PrincipalKind::Human, "h");
    let recorded = s.reference();

    assert_policy(authorise_cancel(&a, Some(&recorded)), "not_job_owner");
    authorise_cancel(&s, Some(&recorded)).unwrap();
    authorise_cancel(&h, Some(&recorded)).unwrap();

    // Same identity text, different kind: not the owner.
    let a_as_s = principal_as(&mut table, PrincipalKind::Agent, "s");
    assert_eq!(a_as_s.id(), s.id());
    assert_policy(authorise_cancel(&a_as_s, Some(&recorded)), "not_job_owner");

    // Spoofs across kinds: a script cancelling an agent's job, an agent cancelling a
    // human's job.
    assert_policy(authorise_cancel(&s, Some(&a.reference())), "not_job_owner");
    assert_policy(authorise_cancel(&a, Some(&h.reference())), "not_job_owner");

    // Every non-human kind: own job allowed, another principal's job refused, a job with
    // no recorded owner refused. A human may cancel all three.
    for k in PrincipalKind::ALL {
        let p = principal_as(&mut table, *k, &format!("owner-{}", k.as_str()));
        let other = PrincipalRef { kind: *k, id: pid("someone-else") };
        authorise_cancel(&p, Some(&p.reference())).unwrap();
        if *k == PrincipalKind::Human {
            authorise_cancel(&p, Some(&other)).unwrap();
            authorise_cancel(&p, None).unwrap();
        } else {
            assert_policy(authorise_cancel(&p, Some(&other)), "not_job_owner");
            assert_policy(authorise_cancel(&p, None), "not_job_owner");
        }
    }
}
