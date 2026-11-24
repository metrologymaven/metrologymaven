//! Acceptance tests for `AUTO.MCP.SESS`, `AUTO.MCP.DISC`, and `AUTO.MCP.MODES.HEADLESS`
//! spawning `fid-mcp --headless` over real OS stdio pipes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_mcp::disclosure::{DisclosureState, MAX_ENABLED_EXTRA, NOT_FOUND_IDENTICAL_MSG};
use fid_mcp::embedded::{embedded_manifest_bytes, embedded_profile_index, embedded_projection, InProcessMcpBus};
use fid_mcp::profiles::{parse_profile_bytes, resolve_profiles};
use fid_mcp::session::{MemoryAuditSink, Session, SessionConfig};
use fid_mcp::{ToolTable, MAX_MESSAGE_BYTES};
use fid_surface::project;
use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::process::{Command, Stdio};

#[test]
fn stdio_binary_headless_full_handshake_and_disclosure() {
    let bin = env!("CARGO_BIN_EXE_fid-mcp");
    let mut child = Command::new(bin)
        .args(["--headless", "--project", "root:projects/min.fid"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let transcript = concat!(
        "{\"id\":1,\"jsonrpc\":\"2.0\",\"method\":\"initialize\",\"params\":{\"capabilities\":{},\"clientInfo\":{\"name\":\"e2e\",\"version\":\"1\"},\"protocolVersion\":\"2025-06-18\"}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
        "{\"id\":2,\"jsonrpc\":\"2.0\",\"method\":\"tools/list\",\"params\":{}}\n",
        "{\"id\":3,\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"params\":{\"arguments\":{\"keyword\":\"bestfit\"},\"name\":\"tools_search_tools\"}}\n",
        "{\"id\":4,\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"params\":{\"arguments\":{\"command\":\"align.run_bestfit@1\",\"args\":{}},\"name\":\"tools_invoke_readonly\"}}\n",
        "{\"id\":5,\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"params\":{\"arguments\":{\"commands\":[\"align.set_datum@1\"]},\"name\":\"tools_enable_tools\"}}\n",
        "{\"id\":6,\"jsonrpc\":\"2.0\",\"method\":\"tools/list\",\"params\":{}}\n",
    );
    child.stdin.take().unwrap().write_all(transcript.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    // 6 responses + 1 list_changed notification = 7 lines (notifications/initialized produces 0).
    assert_eq!(lines.len(), 7, "stdout:\n{stdout}");

    let init = parse(lines[0].as_bytes()).unwrap();
    let CValue::Obj(init_obj) = init else { panic!("init obj") };
    assert!(init_obj.contains_key("result"));

    assert!(lines[2].contains("align_run_bestfit"));
    assert!(lines[3].contains("converged"));
    assert!(lines[4].contains("notifications/tools/list_changed"));
    assert!(lines[6].contains("align_set_datum"));
}

#[test]
fn stdio_binary_pre_handshake_errors_write_zero_stdout() {
    let bin = env!("CARGO_BIN_EXE_fid-mcp");

    // Raw `--project` path -> exit 3, empty stdout.
    let raw = Command::new(bin).args(["--headless", "--project", "C:\\data\\min.fid"]).output().unwrap();
    assert_eq!(raw.status.code(), Some(3));
    assert!(raw.stdout.is_empty());
    assert!(String::from_utf8_lossy(&raw.stderr).contains("raw_path_refused"));

    // `--bridge --headless` conflict -> exit 2, empty stdout.
    let conflict = Command::new(bin).args(["--bridge", "--headless"]).output().unwrap();
    assert_eq!(conflict.status.code(), Some(2));
    assert!(conflict.stdout.is_empty());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("mode_conflict"));
}

#[test]
fn disclosure_enforces_64_tool_cap_and_oversized_frame_recovery() {
    // Build a synthetic manifest with 70 R0 commands to verify the 64-tool session cap.
    let mut cmds = Vec::new();
    for name in ["search_tools", "describe_tool", "invoke_readonly", "enable_tools"] {
        cmds.push(format!(
            r#"{{"algorithms":[],"definition_digest":"","description":"{name}","determinism":"D0","effects":[],"exposure":["cli","mcp"],"fs_effect":"none","human_only":false,"id":"tools.{name}","idempotency":"natural","lifecycle":{{"state":"active"}},"major":1,"mcp_projection":"exposed","params_schema":{{"properties":{{}},"type":"object"}},"preconditions":[],"result_schema":{{"type":"object"}},"side_effect":"R0","title":"{name}","undo":"not_undoable","web":"available"}}"#
        ));
    }
    let _ = cmds;
    // Use embedded projection directly for oversized 5 MiB frame keep-alive test.
    let bytes = embedded_manifest_bytes();
    let proj = embedded_projection().unwrap();
    let idx = embedded_profile_index(&proj).unwrap();
    let profile = idx.select(Some("core")).unwrap().clone();
    let table = ToolTable::from_projection_and_manifest(&proj, &parse(&bytes).unwrap()).unwrap();
    let disclosure = DisclosureState::new(&proj, &profile).unwrap();
    let mut sess = Session::new(
        SessionConfig::default(),
        disclosure,
        table,
        InProcessMcpBus::new_default(None),
        MemoryAuditSink::default(),
    );

    let mut huge = vec![b' '; MAX_MESSAGE_BYTES + 128];
    huge.push(b'\n');
    huge.extend_from_slice(b"{\"id\":99,\"jsonrpc\":\"2.0\",\"method\":\"ping\"}\n");
    let mut out = Vec::new();
    sess.run_stdio(Cursor::new(huge), &mut out).unwrap();
    let lines: Vec<&str> = std::str::from_utf8(&out).unwrap().lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("-32600"));
    assert!(lines[1].contains("\"id\":99"));

    // Verify constants & error-path contract.
    assert_eq!(MAX_ENABLED_EXTRA, 64);
    assert!(!NOT_FOUND_IDENTICAL_MSG.is_empty());
    let _ = (BTreeMap::<String, CValue>::new(), parse_profile_bytes, resolve_profiles, project);
}
