//! Acceptance criteria of spec node AUTO.CLI.DISPATCH, driven through `fid_cli::dispatch::run`
//! against a fake bus behind a test `TargetTransport`.
//!
//! Inputs: `fixtures/manifest/cli-dispatch.json`, `cli-dispatch-collision.json`,
//! `min-classes.json`, `roots-config.json` and `fixtures/cli/dispatch/outcomes.json`, all
//! written by `fixtures/manifest/make_fixtures.py`. Expected parameter documents and messages
//! are hand-labelled from the leaf's acceptance criteria; expected exit codes come from the
//! fixture (hand-labelled from REQ-AUTO.CLI.OUT-002). `AUTO.CLI.ARGS` and `AUTO.CLI.GATE` are
//! not built: [`TestArgs`] and [`TestGate`] are minimal doubles of their specs, so AC-1, AC-7
//! and AC-12 test DISPATCH's side of those steps only (see the leaf's Open questions).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, parse_with_limits, to_canonical_bytes, CValue};
use fid_cli::dispatch::{
    check_tree_flags, run, ArgParser, Channel, ChannelKind, Dispatch, Gate, TargetKind, TargetTransport,
    IDENTITY_FIELDS,
};
use fid_cli::tree::{emit, parse_tree, CliMajor, CliParam, CliTree};
use fid_cli::ExitCode;
use fid_surface::jobs::{
    uuid_v7, BusEvent, BusRequest, BusTransport, CountingKeySource, ManualClock, Purpose, SendError,
};
use fid_surface::roots::PrincipalKind;
use fid_surface::{project, Envelope, ErrorCode, Outcome};
use fid_types::SideEffect;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap()
}

fn tree_of(manifest: &str) -> CliTree {
    let proj = project(&read(manifest)).unwrap();
    parse_tree(&emit(&proj).unwrap().bytes).unwrap()
}

fn dispatch_tree() -> CliTree {
    tree_of("fixtures/manifest/cli-dispatch.json")
}

/// The tree without the listed majors, built by hand (not with `visible_view`).
fn without(tree: &CliTree, ids: &[&str]) -> CliTree {
    let mut t = tree.clone();
    for cmds in t.domains.values_mut() {
        for c in cmds.iter_mut() {
            c.majors.retain(|m| !ids.contains(&m.command_id.as_str()));
        }
        cmds.retain(|c| !c.majors.is_empty());
    }
    t.domains.retain(|_, cmds| !cmds.is_empty());
    t
}

fn argv(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| (*s).to_string()).collect()
}

// ------------------------------------------------------------------ fake bus

/// What the fake bus does with the next submit.
#[derive(Debug, Clone)]
enum Answer {
    /// Accept, then finish with this envelope.
    Finish(Envelope),
    /// Refuse before accepting.
    Refuse(String),
    /// Accept, then never finish.
    Silent,
}

const TEST_USER: &str = "test-os-user";
const TEST_SESSION: &str = "test-process-session";

struct FakeTarget {
    kind: TargetKind,
    channel_kind: ChannelKind,
    principal: PrincipalKind,
    answers: VecDeque<Answer>,
    sent: Vec<BusRequest>,
    inbox: VecDeque<BusEvent>,
    clock: ManualClock,
    jobs: u128,
}

impl FakeTarget {
    fn new(principal: PrincipalKind) -> Self {
        FakeTarget {
            kind: TargetKind::None,
            channel_kind: ChannelKind::Cli,
            principal,
            answers: VecDeque::new(),
            sent: Vec::new(),
            inbox: VecDeque::new(),
            clock: ManualClock::new(),
            jobs: 0,
        }
    }

    fn answering(principal: PrincipalKind, a: Answer) -> Self {
        let mut f = Self::new(principal);
        f.answers.push_back(a);
        f
    }

    fn submits(&self) -> Vec<&BusRequest> {
        self.sent.iter().filter(|r| r.purpose == Purpose::Submit).collect()
    }
}

impl BusTransport for FakeTarget {
    fn send(&mut self, r: &BusRequest) -> Result<(), SendError> {
        self.sent.push(r.clone());
        if r.purpose != Purpose::Submit {
            return Ok(());
        }
        self.jobs += 1;
        let job = uuid_v7(0x0191_0000_0000, self.jobs);
        let answer = self
            .answers
            .pop_front()
            .unwrap_or_else(|| Answer::Finish(Envelope::computed(r.command.as_str(), CValue::Obj(BTreeMap::new()))));
        match answer {
            Answer::Finish(env) => {
                self.inbox.push_back(BusEvent::Accepted { key: r.key, job });
                self.inbox.push_back(BusEvent::Finished { key: r.key, job, envelope: Box::new(env) });
            }
            Answer::Refuse(reason) => {
                self.inbox.push_back(BusEvent::Refused { key: r.key, attempt: r.attempt, reason })
            }
            Answer::Silent => self.inbox.push_back(BusEvent::Accepted { key: r.key, job }),
        }
        Ok(())
    }

    fn recv(&mut self) -> Option<BusEvent> {
        self.inbox.pop_front()
    }
}

impl TargetTransport for FakeTarget {
    fn kind(&self) -> TargetKind {
        self.kind
    }

    fn channel(&self) -> Channel {
        Channel { kind: self.channel_kind, identity: TEST_USER.to_string(), session: TEST_SESSION.to_string() }
    }

    fn principal_kind(&self) -> PrincipalKind {
        self.principal
    }

    fn idle(&mut self) {
        self.clock.advance(Duration::from_secs(1));
    }
}

// ------------------------------------------------------------------ ARGS and GATE doubles

/// Minimal double of `AUTO.CLI.ARGS`: a quantity is `<decimal><unit>` with the digits and the
/// unit kept as typed (REQ-AUTO.CLI.ARGS-001); everything else is the token as a string.
/// `--params -` reads `stdin`.
struct TestArgs {
    stdin: Vec<u8>,
}

impl TestArgs {
    fn new() -> Self {
        TestArgs { stdin: Vec::new() }
    }
}

fn split_quantity(raw: &str) -> Option<(&str, &str)> {
    let end = raw.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | 'e' | 'E')))?;
    let (num, unit) = raw.split_at(end);
    let unit = unit.trim_start();
    if num.is_empty() || unit.is_empty() {
        return None;
    }
    Some((num, unit))
}

impl ArgParser for TestArgs {
    fn parse_value(&self, param: &CliParam, raw: &str) -> Result<CValue, Envelope> {
        if param.quantity_kind.is_some() {
            let Some((value, unit)) = split_quantity(raw) else {
                let msg = format!("`{}` needs a unit", param.name);
                return Err(Envelope::failed("cli.invoke@1", ErrorCode::UsageError, &msg, None));
            };
            let mut o = BTreeMap::new();
            o.insert("unit".to_string(), CValue::Str(unit.to_string()));
            o.insert("value".to_string(), CValue::Str(value.to_string()));
            return Ok(CValue::Obj(o));
        }
        Ok(CValue::Str(raw.to_string()))
    }

    fn read_params(&mut self, source: &str) -> Result<BTreeMap<String, CValue>, Envelope> {
        assert_eq!(source, "-", "tests read --params from stdin only");
        match parse_with_limits(&self.stdin, 1024 * 1024, 32) {
            Ok(CValue::Obj(o)) => Ok(o),
            other => panic!("bad test stdin: {other:?}"),
        }
    }
}

/// Minimal double of `AUTO.CLI.GATE`: M1, S1 and A0 are refused with `policy_refused`, and
/// `--dry-run` on a projection without dry-run support with `dry_run_unsupported`.
struct TestGate;

impl Gate for TestGate {
    fn class_and_dry_run(&self, major: &CliMajor, dry_run: bool) -> Result<(), Envelope> {
        let id = major.command_id.as_str();
        if matches!(major.side_effect, SideEffect::M1 | SideEffect::S1 | SideEffect::A0) {
            return Err(Envelope::failed(id, ErrorCode::PolicyRefused, "refused at the CLI", None));
        }
        if dry_run && !major.dry_run {
            return Err(Envelope::failed(id, ErrorCode::DryRunUnsupported, "dry_run_unsupported", None));
        }
        Ok(())
    }

    fn raw_paths(&self, _kind: PrincipalKind, _major: &CliMajor, _params: &CValue) -> Result<(), Envelope> {
        Ok(())
    }
}

// ------------------------------------------------------------------ running

#[derive(Debug, PartialEq, Eq)]
struct Ran {
    exit: ExitCode,
    stdout: String,
    stderr: String,
}

fn run_with(tree: &CliTree, a: &[&str], target: &mut FakeTarget, args: &mut TestArgs) -> Ran {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let d = Dispatch { tree, version: "0.0.1", args, gate: &TestGate, timeout: Duration::from_secs(5) };
    let clock = target.clock.clone();
    let exit = run(d, &argv(a), target, clock, CountingKeySource::new(0x0190_0000_0000), &mut (), &mut out, &mut err);
    Ran { exit, stdout: String::from_utf8(out).unwrap(), stderr: String::from_utf8(err).unwrap() }
}

fn run_on(tree: &CliTree, a: &[&str], target: &mut FakeTarget) -> Ran {
    run_with(tree, a, target, &mut TestArgs::new())
}

fn canon(v: &CValue) -> String {
    String::from_utf8(to_canonical_bytes(v).unwrap()).unwrap()
}

fn one_envelope(r: &Ran) -> Envelope {
    Envelope::decode(r.stdout.trim_end_matches('\n').as_bytes()).unwrap()
}

fn usage_message(r: &Ran) -> String {
    assert_eq!(r.exit, ExitCode::Usage, "{r:?}");
    let env = one_envelope(r);
    let e = env.error.unwrap();
    assert_eq!(e.code, ErrorCode::UsageError, "{}", e.message);
    e.message
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected an object, got {other:?}"),
    }
}

fn text(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

fn outcomes() -> BTreeMap<String, CValue> {
    obj(&parse(&read("fixtures/cli/dispatch/outcomes.json")).unwrap()).clone()
}

fn envelope_of(v: &CValue) -> Envelope {
    Envelope::decode(&to_canonical_bytes(v).unwrap()).unwrap()
}

// ------------------------------------------------------------------ acceptance criteria

/// AC-1: one invocation, RFC 8785 parameter document, `cli` channel, no identity fields, and
/// stdout is exactly the envelope the bus returned.
#[test]
fn ac1_set_offset_sends_one_invocation_and_prints_the_bus_envelope() {
    let tree = dispatch_tree();
    let reply = Envelope {
        journal_entry_hash: Some(format!("sha256:{}", "ab".repeat(32))),
        ..Envelope::bare("demo.set_offset@1", Outcome::Committed)
    };
    let mut bus = FakeTarget::answering(PrincipalKind::Script, Answer::Finish(reply.clone()));
    let r = run_on(&tree, &["demo", "set-offset", "--offset", "12.700mm", "--label=a", "--json"], &mut bus);
    assert_eq!(r.exit, ExitCode::Success, "{r:?}");
    let sent = bus.submits();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].command.as_str(), "demo.set_offset@1");
    assert_eq!(canon(&sent[0].params), r#"{"label":"a","offset":{"unit":"mm","value":"12.700"}}"#);
    for k in IDENTITY_FIELDS {
        assert!(!obj(&sent[0].params).contains_key(k), "{k}");
    }
    let ch = bus.channel();
    assert_eq!((ch.kind, ch.identity.as_str()), (ChannelKind::Cli, TEST_USER));
    assert_eq!(r.stdout, format!("{}\n", String::from_utf8(reply.encode().unwrap()).unwrap()));
}

/// AC-2: every outcome and error code of AUTO.SURF.ENV passes through unchanged with its
/// REQ-AUTO.CLI.OUT-002 exit code.
#[test]
fn ac2_every_outcome_passes_through_with_its_exit_code() {
    let tree = dispatch_tree();
    let fx = outcomes();
    let CValue::Arr(cases) = &fx["outcomes"] else { panic!("outcomes") };
    assert!(cases.len() >= 20);
    for case in cases {
        let c = obj(case);
        let name = text(&c["name"]);
        let CValue::Num(exit) = c["exit"] else { panic!("exit") };
        let answer = if let Some(env) = c.get("finish") {
            Answer::Finish(envelope_of(env))
        } else if let Some(reason) = c.get("refuse") {
            Answer::Refuse(text(reason).to_string())
        } else {
            Answer::Silent
        };
        let mut bus = FakeTarget::answering(PrincipalKind::Script, answer);
        let r = run_on(&tree, &["demo", "show-thing", "--json"], &mut bus);
        assert_eq!(f64::from(r.exit.code()), exit, "{name}: {r:?}");
        assert_eq!(bus.submits().len(), 1, "{name}");
        if let Some(env) = c.get("finish") {
            assert_eq!(r.stdout, format!("{}\n", canon(env)), "{name}");
        } else if c.contains_key("refuse") {
            assert_eq!(
                r.stdout,
                "{\"command_id\":\"demo.show_thing@1\",\"envelope_version\":1,\"outcome\":\"not_run\"}\n"
            );
        } else {
            let env = one_envelope(&r);
            assert_eq!(env.outcome, Outcome::NoReply, "{name}");
            assert_eq!(env.job_id, Some(uuid_v7(0x0191_0000_0000, 1).to_string()));
        }
    }
}

/// AC-3 (D-035): hidden majors and root-configuration commands are refused exactly as a tree
/// without them would refuse; a path without `--major` takes its newest visible major.
#[test]
fn ac3_hidden_and_root_config_majors_are_absent_for_a_script() {
    let tree = dispatch_tree();
    let deleted = without(&tree, &["demo.old_cmd@1", "demo.mix@2", "project.set_import_root@1"]);
    let argvs: [&[&str]; 4] = [
        &["demo", "old-cmd"],
        &["demo", "mix", "--major", "2"],
        &["project", "set-import-root"],
        &["demo", "old-cmdd"],
    ];
    for a in argvs {
        for json in [false, true] {
            let mut a = a.to_vec();
            if json {
                a.push("--json");
            }
            let mut bus = FakeTarget::new(PrincipalKind::Script);
            let got = run_on(&tree, &a, &mut bus);
            let mut bus2 = FakeTarget::new(PrincipalKind::Script);
            let want = run_on(&deleted, &a, &mut bus2);
            assert_eq!(got, want, "{a:?}");
            assert_eq!(got.exit, ExitCode::Usage, "{a:?}");
            assert!(bus.sent.is_empty() && bus2.sent.is_empty(), "{a:?}");
            let all = format!("{}{}", got.stdout, got.stderr);
            for secret in ["old_cmd", "demo.mix@2", "set_import_root"] {
                assert!(!all.contains(secret), "{a:?} leaks {secret}: {all}");
            }
        }
    }
    let mut bus = FakeTarget::new(PrincipalKind::Script);
    let r = run_on(&tree, &["demo", "mix", "--json"], &mut bus);
    assert_eq!(r.exit, ExitCode::Success, "{r:?}");
    let sent = bus.submits();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].command.as_str(), "demo.mix@1");
}

/// AC-4 (D-035): a bus answer that the major is hidden or unknown becomes the refusal of a
/// tree without that major.
#[test]
fn ac4_bus_hidden_or_unknown_answers_become_the_absent_refusal() {
    let tree = dispatch_tree();
    let deleted = without(&tree, &["demo.show_thing@1"]);
    let CValue::Arr(cases) = &outcomes()["absent"] else { panic!("absent") };
    assert_eq!(cases.len(), 4);
    for case in cases {
        let c = obj(case);
        let name = text(&c["name"]);
        for json in [false, true] {
            let a: &[&str] = if json { &["demo", "show-thing", "--json"] } else { &["demo", "show-thing"] };
            let mut bus = FakeTarget::answering(PrincipalKind::Script, Answer::Finish(envelope_of(&c["finish"])));
            let got = run_on(&tree, a, &mut bus);
            assert_eq!(bus.submits().len(), 1, "{name}");
            let mut bus2 = FakeTarget::new(PrincipalKind::Script);
            let want = run_on(&deleted, a, &mut bus2);
            assert!(bus2.sent.is_empty());
            assert_eq!(got, want, "{name} json={json}");
            assert_eq!(got.exit, ExitCode::Usage, "{name}");
        }
    }
}

/// AC-10: handler and lookup errors pass through unchanged.
#[test]
fn ac10_other_not_found_and_validation_errors_pass_through() {
    let tree = dispatch_tree();
    let CValue::Arr(cases) = &outcomes()["passthrough"] else { panic!("passthrough") };
    assert_eq!(cases.len(), 4);
    for case in cases {
        let c = obj(case);
        let name = text(&c["name"]);
        let CValue::Num(exit) = c["exit"] else { panic!("exit") };
        let mut bus = FakeTarget::answering(PrincipalKind::Script, Answer::Finish(envelope_of(&c["finish"])));
        let r = run_on(&tree, &["demo", "show-thing", "--json"], &mut bus);
        assert_eq!(r.stdout, format!("{}\n", canon(&c["finish"])), "{name}");
        assert_eq!(f64::from(r.exit.code()), exit, "{name}");
    }
}

/// AC-11: no schema default is inserted.
#[test]
fn ac11_defaults_are_not_inserted() {
    let tree = dispatch_tree();
    let mut bus = FakeTarget::new(PrincipalKind::Script);
    let r = run_on(&tree, &["demo", "set-offset", "--offset", "1mm"], &mut bus);
    assert_eq!(r.exit, ExitCode::Success, "{r:?}");
    assert_eq!(canon(&bus.submits()[0].params), r#"{"offset":{"unit":"mm","value":"1"}}"#);
}

/// AC-12: the required check sees the `--params` document; flags win over it.
#[test]
fn ac12_params_document_and_required_check() {
    let tree = dispatch_tree();
    let mut bus = FakeTarget::new(PrincipalKind::Script);
    let mut args = TestArgs { stdin: br#"{"label":"a"}"#.to_vec() };
    let r = run_with(&tree, &["demo", "set-offset", "--params", "-"], &mut bus, &mut args);
    assert_eq!(r.exit, ExitCode::Usage, "{r:?}");
    assert!(r.stdout.contains("--offset"), "{r:?}");
    assert!(bus.sent.is_empty());

    let mut bus = FakeTarget::new(PrincipalKind::Script);
    let mut args = TestArgs { stdin: br#"{"offset":{"value":"2","unit":"mm"}}"#.to_vec() };
    let r = run_with(&tree, &["demo", "set-offset", "--params", "-"], &mut bus, &mut args);
    assert_eq!(r.exit, ExitCode::Success, "{r:?}");
    assert_eq!(canon(&bus.submits()[0].params), r#"{"offset":{"unit":"mm","value":"2"}}"#);

    // Explicit flags win over the document.
    let mut bus = FakeTarget::new(PrincipalKind::Script);
    let mut args = TestArgs { stdin: br#"{"label":"doc","offset":{"value":"2","unit":"mm"}}"#.to_vec() };
    let r = run_with(&tree, &["demo", "set-offset", "--params", "-", "--label", "flag"], &mut bus, &mut args);
    assert_eq!(r.exit, ExitCode::Success, "{r:?}");
    assert_eq!(canon(&bus.submits()[0].params), r#"{"label":"flag","offset":{"unit":"mm","value":"2"}}"#);
}

/// AC-13: visibility comes before every other step.
#[test]
fn ac13_absent_refusal_comes_before_flag_and_dry_run_errors() {
    let tree = dispatch_tree();
    let deleted = without(&tree, &["demo.old_cmd@1", "demo.mix@2", "project.set_import_root@1"]);
    let pairs: [(&[&str], &[&str]); 2] = [
        (&["demo", "old-cmd", "--no-such-flag"], &["demo", "old-cmd"]),
        (&["demo", "mix", "--major", "2", "--dry-run", "--ofset", "x"], &["demo", "mix", "--major", "2"]),
    ];
    for (with_flags, bare) in pairs {
        let mut bus = FakeTarget::new(PrincipalKind::Script);
        let got = run_on(&tree, with_flags, &mut bus);
        let want = run_on(&deleted, bare, &mut FakeTarget::new(PrincipalKind::Script));
        assert_eq!(got, want, "{with_flags:?}");
        assert!(bus.sent.is_empty());
        let all = format!("{}{}", got.stdout, got.stderr);
        for s in ["--no-such-flag", "--ofset", "dry_run_unsupported"] {
            assert!(!all.contains(s), "{with_flags:?}: {all}");
        }
    }
}

/// AC-5: unknown, missing, repeated and stray arguments.
#[test]
fn ac5_unknown_missing_repeated_and_stray_arguments() {
    let tree = dispatch_tree();
    let cases: [(&[&str], &[&str]); 5] = [
        (&["demo", "set-offset", "--ofset", "1mm", "--json"], &["`--ofset`", "`--offset`"]),
        (&["demo", "set-offset", "--ofset=1mm", "--json"], &["`--ofset`", "`--offset`"]),
        (&["demo", "set-offset", "--label", "a", "--json"], &["missing", "`--offset`"]),
        (&["demo", "set-offset", "--offset", "1mm", "--label", "a", "--label", "b", "--json"], &["`--label`"]),
        (&["demo", "set-offset", "--offset", "1mm", "stray", "--json"], &["`stray`"]),
    ];
    for (a, want) in cases {
        let mut bus = FakeTarget::new(PrincipalKind::Script);
        let msg = usage_message(&run_on(&tree, a, &mut bus));
        for w in want {
            assert!(msg.contains(w), "{a:?}: {msg}");
        }
        assert!(bus.sent.is_empty(), "{a:?}");
    }
}

/// AC-6: reserved-flag and twin-property collisions.
#[test]
fn ac6_flag_collisions_name_the_major_and_the_property() {
    let errs = check_tree_flags(&tree_of("fixtures/manifest/cli-dispatch-collision.json")).unwrap_err();
    let got: Vec<(String, String)> = errs.iter().map(|e| (e.major.as_str().to_string(), e.property.clone())).collect();
    assert_eq!(
        got,
        vec![
            ("demo.bad_flags@1".to_string(), "dry_run".to_string()),
            ("demo.twin_flags@1".to_string(), "a_b".to_string())
        ]
    );
    assert!(errs.iter().all(|e| e.code() == "flag_collision"));
    assert!(errs[0].to_string().contains("--dry-run"));
    assert!(errs[1].to_string().contains("\"a-b\""));
    // The fixtures of this leaf build cleanly.
    for m in ["fixtures/manifest/cli-dispatch.json", "fixtures/manifest/min-classes.json"] {
        assert_eq!(check_tree_flags(&tree_of(m)), Ok(()), "{m}");
    }
}

/// AC-7: every visible major not of class M1, S1 or A0 reaches the bus exactly once.
#[test]
fn ac7_min_classes_reach_the_bus_except_m1_s1_a0() {
    let tree = tree_of("fixtures/manifest/min-classes.json");
    let mut reached = Vec::new();
    let mut checked = 0;
    for (domain, cmds) in &tree.domains {
        for c in cmds {
            for m in &c.majors {
                let mut a = vec![domain.clone(), c.path[2].clone(), "--major".to_string(), m.major.to_string()];
                for p in m.params.iter().filter(|p| p.required) {
                    a.push(format!("--{}", p.name.replace('_', "-")));
                    a.push(if p.quantity_kind.is_some() { "1mm".to_string() } else { "root:in/a.stl".to_string() });
                }
                let a: Vec<&str> = a.iter().map(String::as_str).collect();
                let mut bus = FakeTarget::new(PrincipalKind::Script);
                let r = run_on(&tree, &a, &mut bus);
                assert!(!format!("{}{}", r.stdout, r.stderr).contains("AUTO.CLI.DISPATCH"));
                let n = bus.submits().len();
                let refused = matches!(m.side_effect, SideEffect::M1 | SideEffect::S1 | SideEffect::A0);
                assert_eq!(n, usize::from(!refused), "{}: {r:?}", m.command_id.as_str());
                if n == 1 {
                    assert_eq!(bus.submits()[0].command, m.command_id);
                    reached.push(m.command_id.as_str().to_string());
                }
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 13);
    assert_eq!(reached.len(), 10, "{reached:?}");
}

/// AC-8: an upgraded (`human`) session sees and invokes the root-configuration command; a
/// `script` session does not, whatever its argv says.
#[test]
fn ac8_human_session_invokes_root_config_command() {
    let tree = tree_of("fixtures/manifest/roots-config.json");
    let mut bus = FakeTarget::new(PrincipalKind::Human);
    let r = run_on(&tree, &["project", "set-import-root", "--json"], &mut bus);
    assert_eq!(r.exit, ExitCode::Success, "{r:?}");
    let sent = bus.submits();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].command.as_str(), "project.set_import_root@1");
    assert_eq!(canon(&sent[0].params), "{}");

    // Identity model: `--confirm-human` on argv does not make a script session human.
    let mut bus = FakeTarget::new(PrincipalKind::Script);
    let r = run_on(&tree, &["project", "set-import-root", "--confirm-human", "--json"], &mut bus);
    assert_eq!(r.exit, ExitCode::Usage, "{r:?}");
    assert!(bus.sent.is_empty());
}

/// AC-9: object properties take one bounded JSON value; arrays append per occurrence.
#[test]
fn ac9_object_and_array_values() {
    let tree = dispatch_tree();
    let mut bus = FakeTarget::new(PrincipalKind::Script);
    let r = run_on(&tree, &["demo", "place-label", "--anchor", r#"{"x":"1","y":"2"}"#], &mut bus);
    assert_eq!(r.exit, ExitCode::Success, "{r:?}");
    assert_eq!(canon(&bus.submits()[0].params), r#"{"anchor":{"x":"1","y":"2"}}"#);

    let big = format!("{{\"x\":\"{}\"}}", "a".repeat(2 * 1024 * 1024));
    let mut bus = FakeTarget::new(PrincipalKind::Script);
    let r = run_on(&tree, &["demo", "place-label", "--anchor", &big, "--json"], &mut bus);
    let msg = usage_message(&r);
    assert!(msg.contains("params_too_large"), "{msg}");
    let details = one_envelope(&r).error.unwrap().details.unwrap();
    assert_eq!(text(&obj(&details)["cli_error"]), "params_too_large");
    assert!(bus.sent.is_empty());

    let mut bus = FakeTarget::new(PrincipalKind::Script);
    let r = run_on(&tree, &["demo", "place-label", "--anchor", "[1]", "--json"], &mut bus);
    assert!(usage_message(&r).contains("params_parse"));
    assert!(bus.sent.is_empty());

    let mut bus = FakeTarget::new(PrincipalKind::Script);
    let r = run_on(&tree, &["demo", "place-label", "--tags", "b", "--tags=a"], &mut bus);
    assert_eq!(r.exit, ExitCode::Success, "{r:?}");
    assert_eq!(canon(&bus.submits()[0].params), r#"{"tags":["b","a"]}"#);
}

/// REQ-008 and REQ-009: a headless or target-free invocation needs a `cli` channel, and a
/// `--params` document cannot carry identity fields.
#[test]
fn channel_and_identity_fields() {
    let tree = dispatch_tree();
    let mut bus = FakeTarget::new(PrincipalKind::Script);
    bus.channel_kind = ChannelKind::Control;
    let r = run_on(&tree, &["demo", "show-thing", "--json"], &mut bus);
    assert_eq!(r.exit, ExitCode::Internal, "{r:?}");
    assert!(bus.sent.is_empty());

    // The app target sends over the control channel, whose server binds the principal.
    bus.kind = TargetKind::App;
    let r = run_on(&tree, &["demo", "show-thing", "--json"], &mut bus);
    assert_eq!(r.exit, ExitCode::Success, "{r:?}");

    for field in IDENTITY_FIELDS {
        let mut bus = FakeTarget::new(PrincipalKind::Script);
        let doc = format!(r#"{{"{field}":"human","offset":{{"value":"2","unit":"mm"}}}}"#);
        let mut args = TestArgs { stdin: doc.into_bytes() };
        let r = run_with(&tree, &["demo", "set-offset", "--params", "-", "--json"], &mut bus, &mut args);
        assert!(usage_message(&r).contains(field), "{field}");
        assert!(bus.sent.is_empty(), "{field}");
    }
}

/// The gate's class and dry-run checks run before flag mapping and send nothing.
#[test]
fn gate_refusals_send_nothing_and_come_before_flag_errors() {
    let tree = dispatch_tree();
    let mut bus = FakeTarget::new(PrincipalKind::Script);
    let r = run_on(&tree, &["demo", "show-thing", "--dry-run", "--bogus", "--json"], &mut bus);
    assert_eq!(r.exit, ExitCode::Usage, "{r:?}");
    assert_eq!(one_envelope(&r).error.unwrap().code, ErrorCode::DryRunUnsupported);
    assert!(bus.sent.is_empty());
}
