"""Tier 5: Adversarial & security-boundary opaque-box E2E tests (opt-in via --tier 5)."""

from __future__ import annotations

import json
import re
import sys

from e2e.lib.harness import TestContext, TestFailure, e2e_test, parse_single_line_json
from e2e.tests._helpers import (
    mcp_init_messages,
    run_mcp_session,
    write_disclosure_profiles_index,
)


@e2e_test(
    test_id="T5-F4-01",
    tier=5,
    feature="F4",
    description="fid-mcp fails closed on oversized unterminated framing without smuggling and accepts CRLF at max boundary",
)
def test_t5_mcp_framing_smuggling_and_crlf(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    payload = ("a" * (36 * 1024 * 1024 + 16_384)) + "\n" + json.dumps(
        {"jsonrpc": "2.0", "id": 99, "method": "initialize", "params": {}}
    ) + "\n"
    res = ctx.run([mcp, "--headless", "--project", "root:projects/min.fid"], stdin_text=payload, timeout=30)
    if res.returncode == 0:
        raise TestFailure("expected non-zero exit when framing discard budget exhausted")
    lines = [ln for ln in res.stdout.splitlines() if ln.strip()]
    if len(lines) != 1:
        raise TestFailure(f"expected single -32600 reply (no smuggling), got {len(lines)}: {lines!r}")
    reply = json.loads(lines[0])
    if reply.get("error", {}).get("code") != -32600:
        raise TestFailure(f"expected -32600 error code, got {reply!r}")

    prefix = '{"id":1,"jsonrpc":"2.0","method":"initialize","params":{"pad":"'
    suffix = '"}}\r\n'
    pad = "b" * (4 * 1024 * 1024 - len(prefix) - (len(suffix) - 2))
    crlf_msg = prefix + pad + suffix
    res_ok = ctx.run([mcp, "--headless", "--project", "root:projects/min.fid"], stdin_text=crlf_msg, timeout=30)
    if res_ok.returncode != 0:
        raise TestFailure(f"CRLF frame at MAX_MESSAGE_BYTES should succeed, rc={res_ok.returncode}: {res_ok.stderr}")


@e2e_test(
    test_id="T5-F4-02",
    tier=5,
    feature="F4",
    description="fid-mcp enforces lifecycle, fractional ID, disclosure gate, profile integrity, CLI empty args, and plain canon strings",
)
def test_t5_mcp_lifecycle_disclosure_profiles_and_canon(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    idx = write_disclosure_profiles_index(ctx.workdir)

    # 1. Uninitialized tools/call -> -32002; fractional ID -> -32600; init; un-enabled direct tool -> isError; canon string -> ok.
    msgs_in = [
        {"jsonrpc": "2.0", "id": 10, "method": "tools/call", "params": {"name": "tools_search_tools", "arguments": {}}},
        {"jsonrpc": "2.0", "id": 1.5, "method": "initialize", "params": {}},
        *mcp_init_messages(),
        {"jsonrpc": "2.0", "id": 20, "method": "tools/call", "params": {"name": "align_set_datum", "arguments": {}}},
        {
            "jsonrpc": "2.0",
            "id": 21,
            "method": "tools/call",
            "params": {
                "name": "tools_invoke_readonly",
                "arguments": {"command": "canon.canonicalize@1", "args": {"input": "plain_string"}},
            },
        },
    ]
    rc, msgs, _, stderr = run_mcp_session(mcp, ["--headless"], msgs_in, cwd=ctx.repo_root)
    if rc != 0:
        raise TestFailure(f"session exited {rc}: {stderr}")
    by_id = {m.get("id"): m for m in msgs}
    if by_id.get(10, {}).get("error", {}).get("code") != -32002:
        raise TestFailure(f"expected -32002 on uninitialized tools/call: {by_id.get(10)!r}")
    if not any(m.get("error", {}).get("code") == -32600 for m in msgs):
        raise TestFailure(f"expected -32600 on fractional rpc id: {msgs!r}")
    if not by_id.get(20, {}).get("result", {}).get("isError"):
        raise TestFailure(f"expected PolicyRefused isError on un-enabled tool: {by_id.get(20)!r}")
    if by_id.get(21, {}).get("result", {}).get("isError"):
        raise TestFailure(f"canon.canonicalize@1 should accept plain JSON string: {by_id.get(21)!r}")

    # 2. Tampered profiles index (digest mismatch) rejected with exit 2.
    bad_idx = ctx.workdir / "bad-digest-profiles.json"
    bad_obj = json.loads(idx.read_text(encoding="utf-8"))
    bad_obj["manifest_digest"] = "sha256:" + ("0" * 64)
    bad_idx.write_text(json.dumps(bad_obj), encoding="utf-8")
    r_bad = ctx.run([mcp, "--profiles-index", str(bad_idx), "--print-profile"])
    if r_bad.returncode != 2:
        raise TestFailure(f"expected exit 2 for mismatched manifest_digest, got {r_bad.returncode}")

    # 3. Empty flag forms rejected with exit 2.
    for args in (["--server", ""], ["--server="], ["--profiles-index="], ["--manifest="], ["--project="], ["--agent="]):
        r_emp = ctx.run([mcp, *args])
        if r_emp.returncode != 2:
            raise TestFailure(f"expected exit 2 for {args}, got {r_emp.returncode}")


@e2e_test(
    test_id="T5-F3-01",
    tier=5,
    feature="F3",
    description="fid-verify handles --help --unknown precedence, -- end-of-options, and trust root open copy",
)
def test_t5_verify_cli_edge_cases(ctx: TestContext) -> None:
    verify = ctx.require_bin("fid-verify")
    r_help = ctx.run([verify, "--help", "--unknown"])
    if r_help.returncode != 0 or "usage:" not in r_help.stdout:
        raise TestFailure(f"--help --unknown failed: rc={r_help.returncode} out={r_help.stdout!r}")

    r_dash = ctx.run([verify, "--", "-nonexistent.fpkg"])
    if r_dash.returncode != 70 or "unknown option" in r_dash.stderr:
        raise TestFailure(f"-- sentinel failed: rc={r_dash.returncode} err={r_dash.stderr!r}")

    r_tr = ctx.run([verify, str(ctx.repo_root / "fixtures" / "pkg" / "golden" / "results.fpkg"), "--trust-root", str(ctx.repo_root / "fixtures" / "troot" / "roots"), "--json"])
    if r_tr.returncode != 70 or "cannot open trust root" not in (r_tr.stdout + r_tr.stderr):
        raise TestFailure(f"trust root open error copy unexpected: rc={r_tr.returncode} out={r_tr.stdout!r} err={r_tr.stderr!r}")


@e2e_test(
    test_id="T5-F5-01",
    tier=5,
    feature="F5",
    description="fid-guide-wasm gdt.zone_eval handles tolerance=0, overflow, and rejects unknown keys",
)
def test_t5_guide_wasm_gdt_edges(ctx: TestContext) -> None:
    wasm = ctx.require_bin("fid-guide-wasm")
    r0 = ctx.run([wasm], stdin_text=json.dumps({"op": "gdt.zone_eval", "params": {"unit": "mm", "shape": "cylinder", "tolerance": 0.0, "points": [[0, 0, 0], [5, 0, 0]]}}))
    if r0.returncode != 0:
        raise TestFailure(f"tolerance=0 failed: {r0.stderr}")
    d0 = parse_single_line_json(r0.stdout)["result"]
    if d0["contains"] is not False or d0["utilization_pct"] is not None:
        raise TestFailure(f"expected contains=False and utilization_pct=None, got {d0!r}")

    r_ovf = ctx.run([wasm], stdin_text=json.dumps({"op": "gdt.zone_eval", "params": {"unit": "um", "shape": "cylinder", "tolerance": 1e-305, "points": [[0, 0, 0], [1000, 0, 0]]}}))
    if r_ovf.returncode != 0 or parse_single_line_json(r_ovf.stdout)["result"]["utilization_pct"] is not None:
        raise TestFailure(f"expected overflow utilization_pct=None, got rc={r_ovf.returncode} out={r_ovf.stdout!r}")

    r_unk = ctx.run([wasm], stdin_text=json.dumps({"op": "gdt.zone_eval", "params": {"unit": "mm", "shape": "cylinder", "tolerance": 0.1, "axis_dir": [0, 0, 0], "points": [[0, 0, 0]]}}))
    if r_unk.returncode == 0:
        raise TestFailure("expected error on unknown key axis_dir")


@e2e_test(
    test_id="T5-F5-02",
    tier=5,
    feature="F5",
    description="site/worker.js normalizes percent-encoded & dot-segment routes and site/build.py emits zero inline styles",
)
def test_t5_site_csp_and_worker_traversal(ctx: TestContext) -> None:
    node = ctx.require_node()
    worker_url = (ctx.repo_root / "site" / "worker.js").as_uri()
    probe = (
        f"import worker from {json.dumps(worker_url)};\n"
        "const env = { ASSETS: { fetch: async () => new Response('ok', { headers: { 'content-type': 'text/html' } }) } };\n"
        "const out = {};\n"
        "for (const p of ['/%6eotes/foo', '/%67uide/fits/', '/guide/..%2fstatus/', '/guide/%2e%2e/status/', '/%ZZ']) {\n"
        "  const res = await worker.fetch(new Request('https://metrologymaven.io' + p), env);\n"
        "  out[p] = { status: res.status, csp: res.headers.get('Content-Security-Policy') || '' };\n"
        "}\n"
        "console.log(JSON.stringify(out));\n"
    )
    r = ctx.run([node, "--input-type=module", "-e", probe])
    if r.returncode != 0:
        raise TestFailure(f"node worker probe failed: {r.stderr}")
    data = parse_single_line_json(r.stdout)
    if data["/%6eotes/foo"]["status"] != 301:
        raise TestFailure(f"expected 301 for /%6eotes/foo, got {data['/%6eotes/foo']!r}")
    if not re.search(r"(?:^|;\s*)script-src\s+'self'\s+'wasm-unsafe-eval'\s*(?:;|$)", data["/%67uide/fits/"]["csp"]):
        raise TestFailure(f"expected wasm-unsafe-eval on /%67uide/fits/: {data['/%67uide/fits/']!r}")
    for p in ("/guide/..%2fstatus/", "/guide/%2e%2e/status/", "/%ZZ"):
        csp = data[p]["csp"]
        if not re.search(r"(?:^|;\s*)script-src\s+'none'\s*(?:;|$)", csp) or "wasm-unsafe-eval" in csp:
            raise TestFailure(f"traversal/malformed path {p} escaped non-guide CSP: {csp!r}")


@e2e_test(
    test_id="T5-F7-01",
    tier=5,
    feature="F7",
    description="check_spec_drift.py catches multiline/nested/table-swap drift and scope.py blocks list-scalar crash & milestone escape",
)
def test_t5_spec_drift_and_scope_hardening(ctx: TestContext) -> None:
    r = ctx.run([sys.executable, "-m", "unittest", "discover", "-s", "tools/tests", "-p", "test_pkg_drift.py", "-v"], cwd=ctx.repo_root)
    if r.returncode != 0:
        raise TestFailure(f"test_pkg_drift.py failed:\n{r.stdout}\n{r.stderr}")
    r2 = ctx.run([sys.executable, "-m", "unittest", "discover", "-s", "tools/tests", "-p", "test_site_csp.py", "-v"], cwd=ctx.repo_root)
    if r2.returncode != 0:
        raise TestFailure(f"test_site_csp.py failed:\n{r2.stdout}\n{r2.stderr}")


@e2e_test(
    test_id="T5-F2-01",
    tier=5,
    feature="F2",
    description="fid CLI avoids flag value-stealing & rejects bad object params; fid-slice rejects conflicting modes",
    capabilities_required=("c_toolchain",),
)
def test_t5_cli_and_slice_hardening(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    r_hlp = ctx.run([fid, "project", "list-features", "--reason", "--help", "--json"])
    if "USAGE:" in r_hlp.stdout:
        raise TestFailure(f"--reason --help triggered help unexpectedly: {r_hlp.stdout!r}")

    r_ctrl = ctx.run([fid, "ctrl", "--help"])
    if r_ctrl.returncode != 0 or "fid ctrl status" not in r_ctrl.stdout:
        raise TestFailure(f"fid ctrl --help failed: rc={r_ctrl.returncode} out={r_ctrl.stdout!r}")

    slice_bin = ctx.require_bin("fid-slice")
    r_sl = ctx.run([slice_bin, "--demo", "--balloon-demo", "--out", str(ctx.workdir / "sl")])
    if r_sl.returncode != 1:
        raise TestFailure(f"expected exit 1 on mutually exclusive slice modes, got {r_sl.returncode}")
