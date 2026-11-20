//! The permit rule of spec node CORE.BUS.POL.EGRESS (REQ-004) as the facade sees it: a permit
//! only for X1, I1 and D1, at most one per execution scope, and a connection only with one.
//! Only loopback addresses are used; no test reaches another host.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Code};
use fid_bus::policy::egress::{issue_permit_for_tests, permits_network, PermitScope};
use fid_net::connect_tcp;
use fid_registry::CommandDef;
use fid_types::SideEffect;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::time::Duration;

/// Hand-labelled from CONSTITUTION §3.1 and REQ-CORE.BUS.POL-008: network only for X1, I1, D1.
const EXPECTED: [(&str, bool); 11] = [
    ("R0", false),
    ("R1", false),
    ("W1", false),
    ("W2", false),
    ("W3", false),
    ("X1", true),
    ("I1", true),
    ("D1", true),
    ("M1", false),
    ("S1", false),
    ("A0", false),
];

/// A minimal valid command definition of `class`, read through the registry's own rules.
fn def(class: &str) -> CommandDef {
    let projection = match class {
        "M1" | "S1" | "A0" => "never",
        "W3" | "X1" | "D1" => "request_only",
        _ => "exposed",
    };
    let text = format!(
        r#"{{"id":"test.net_probe","major":1,"title":"Net probe","description":"Synthetic.",
        "params_schema":{{"type":"object"}},"result_schema":{{"type":"object"}},
        "side_effect":"{class}","mcp_projection":"{projection}","determinism":"N","idempotency":"none",
        "fs_effect":"none","undo":"not_undoable","preconditions":[],"effects":["TestDone"],"algorithms":[],
        "exposure":["cli"],"lifecycle":{{"state":"active","since":"2026-11-20"}},"human_only":false,
        "network":"none","web":"available","cli_bare":false}}"#
    );
    CommandDef::from_value(fid_canon::parse(text.as_bytes()).unwrap()).unwrap()
}

fn assert_class_forbidden(e: BusError) {
    assert_eq!(e.code(), Code::Policy);
    assert_eq!(e.reason(), "class_forbidden");
}

#[test]
fn only_x1_i1_and_d1_get_a_permit() {
    assert_eq!(SideEffect::ALL.len(), EXPECTED.len(), "a new class needs a hand-labelled row");
    for (wire, allowed) in EXPECTED {
        let class = SideEffect::parse(wire).unwrap();
        assert_eq!(permits_network(class), allowed, "{wire}");
        let d = def(wire);
        let mut scope = PermitScope::new();
        match issue_permit_for_tests(&d, &mut scope) {
            Ok(p) => {
                assert!(allowed, "{wire} got a permit");
                assert_eq!(p.class(), class);
            }
            Err(e) => {
                assert!(!allowed, "{wire} was refused");
                assert_class_forbidden(e);
            }
        }
        assert_eq!(scope.issued(), allowed, "{wire}: a refused issue leaves the scope unchanged");
    }
}

#[test]
fn a_scope_issues_one_permit() {
    let d = def("X1");
    let mut scope = PermitScope::new();
    {
        let first = issue_permit_for_tests(&d, &mut scope);
        assert!(first.is_ok());
    }
    assert_class_forbidden(issue_permit_for_tests(&d, &mut scope).unwrap_err());
    assert_class_forbidden(issue_permit_for_tests(&def("D1"), &mut scope).unwrap_err());
}

#[test]
fn a_permit_connects_to_a_loopback_listener() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).unwrap();
        buf
    });
    let d = def("X1");
    let mut scope = PermitScope::new();
    let permit = issue_permit_for_tests(&d, &mut scope).unwrap();
    {
        let mut stream = connect_tcp(&permit, &addr, Duration::from_secs(5)).unwrap();
        stream.write_all(b"ok").unwrap();
    }
    assert_eq!(server.join().unwrap(), b"ok");
}

#[test]
fn debug_output_names_only_the_class() {
    let d = def("I1");
    let mut scope = PermitScope::new();
    let permit = issue_permit_for_tests(&d, &mut scope).unwrap();
    assert_eq!(format!("{permit:?}"), "NetPermit { class: \"I1\" }");
}

/// AC-5 is the pair of `compile_fail` doctests in `src/lib.rs`. Stable rustdoc does not check
/// the error of a `compile_fail` block, so this pins what each case does: the W2 handler calls
/// the facade without a permit, or tries to issue one itself, and the control makes the same
/// call with a permit and compiles (a `no_run` doctest).
#[test]
fn ac5_compile_fail_cases_are_the_intended_ones() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compile_fail");
    let read = |n: &str| std::fs::read_to_string(dir.join(n)).unwrap();
    let without = read("w2_without_permit.rs");
    assert!(without.contains("impl Handler for W2Handler"));
    assert!(without.contains("fid_net::connect_tcp(&self.addr, Duration::from_secs(1))"));
    // No permit is obtained anywhere in the case (its comment names the type only).
    assert!(!without.contains("issue_permit") && !without.contains("use fid_bus::policy::egress"));
    let forged = read("w2_forged_permit.rs");
    assert!(forged.contains("impl Handler for W2Handler"));
    assert!(forged.contains("fid_bus::policy::egress::issue_permit("));
    let literal = read("w2_struct_literal_permit.rs");
    assert!(literal.contains("impl Handler for W2Handler"));
    assert!(literal.contains("NetPermit { class: SideEffect::X1, _scope: PhantomData }"));
    let control = read("with_permit_control.rs");
    assert!(control.contains("fid_net::connect_tcp(permit, &self.addr, Duration::from_secs(1))"));
    let lib = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs")).unwrap();
    for n in ["w2_without_permit.rs", "w2_forged_permit.rs", "w2_struct_literal_permit.rs"] {
        let at = lib.find(&format!("tests/compile_fail/{n}")).unwrap();
        assert!(lib[..at].ends_with("#![doc = \"```compile_fail\"]\n#![doc = include_str!(\"../"), "{n}");
    }
}
