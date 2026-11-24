"""Tier 4 real-world end-to-end application workload scenarios (TEST_INFRA.md Scenarios 1-7)."""

from __future__ import annotations

import csv
import json
import re
import shutil
import sys
from pathlib import Path

from e2e.lib.harness import (
    TestContext,
    TestFailure,
    assert_envelope,
    e2e_test,
    parse_single_line_json,
)
from e2e.tests._helpers import (
    build_isolated_site,
    mcp_init_messages,
    prepare_scope_root,
    run_mcp_session,
    write_disclosure_profiles_index,
    write_site_a11y_tokens,
)


@e2e_test(
    test_id="T4-S1",
    tier=4,
    feature="F2",
    description="Scenario 1: Aerospace Bracket Inspection & Independent Package Verification (fid-slice -> pkg-conformance -> fid-verify + tamper fail)",
)
def test_scenario_1_aerospace_bracket_and_verify(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    conf = ctx.require_bin("fid-pkg-conformance")
    verify = ctx.require_bin("fid-verify")

    out_dir = ctx.workdir / "bracket_inspection"
    r_slice = ctx.run([fid_slice, "--demo", "--out", str(out_dir), "--json"])
    if r_slice.returncode != 0:
        raise TestFailure(f"fid-slice --demo failed: {r_slice.stderr}")
    env = parse_single_line_json(r_slice.stdout)
    assert_envelope(env, outcome="computed", command_id="slice.run_vertical@1")

    pkg = out_dir / "result-package.zip"
    r_conf = ctx.run([conf, str(pkg)])
    if r_conf.returncode != 0 or parse_single_line_json(r_conf.stdout).get("result") != "accepted":
        raise TestFailure(f"fid-pkg-conformance rejected generated package: {r_conf.stdout}")

    # Independent verification + replay on golden signed package and tampered fail-closed check
    golden_signed = ctx.repo_root / "fixtures" / "pkg" / "golden" / "two-signatures.zip"
    trust_root = ctx.repo_root / "fixtures" / "troot" / "roots" / "both.json"
    r_v1 = ctx.run([verify, str(golden_signed), "--trust-root", str(trust_root), "--replay", "--json"])
    if r_v1.returncode != 0:
        raise TestFailure(
            f"fid-verify golden two-signatures.zip failed (exit {r_v1.returncode}): {r_v1.stdout} {r_v1.stderr}"
        )
    rep = parse_single_line_json(r_v1.stdout)
    if (
        rep.get("verdict") != "PASS"
        or rep.get("chain", {}).get("verdict") != "VALID"
        or rep.get("replay", {}).get("status") != "IDENTICAL"
    ):
        raise TestFailure(f"unexpected fid-verify report: {rep!r}")

    # Tamper 1 byte -> must fail closed in both fid-slice --verify and fid-verify
    tampered = ctx.workdir / "tampered.zip"
    raw = bytearray(pkg.read_bytes())
    raw[len(raw) // 2] ^= 0x01
    tampered.write_bytes(bytes(raw))
    r_ts = ctx.run([fid_slice, "--verify", str(tampered), "--json"])
    r_tv = ctx.run([verify, str(tampered), "--json"])
    if r_ts.returncode == 0 or r_tv.returncode == 0:
        raise TestFailure(f"tampered package was not rejected: slice={r_ts.returncode} verify={r_tv.returncode}")


@e2e_test(
    test_id="T4-S2",
    tier=4,
    feature="F2",
    description="Scenario 2: 2D Engineering Drawing Auto-Ballooning & Report Export Workflow (vector_bracket + scanned_only)",
)
def test_scenario_2_2d_drawing_auto_ballooning(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    fid = ctx.require_bin("fid")
    vec_pdf = ctx.repo_root / "fixtures" / "rpt" / "balloon" / "auto" / "vector_bracket.pdf"
    scan_pdf = ctx.repo_root / "fixtures" / "rpt" / "balloon" / "auto" / "scanned_only.pdf"
    truth = json.loads(
        (ctx.repo_root / "fixtures" / "rpt" / "balloon" / "auto" / "vector_bracket.truth.json").read_text(
            encoding="utf-8"
        )
    )

    # Confirm F1 report ballooning commands are wired into fid CLI tree
    for sub in ("extract-drawing-text", "propose-balloons", "export-balloon-pack"):
        r_h = ctx.run([fid, "report", sub, "--help"])
        if r_h.returncode != 0 or "Command:" not in r_h.stdout:
            raise TestFailure(f"fid report {sub} --help failed (exit {r_h.returncode}): {r_h.stdout} {r_h.stderr}")

    # Run Day-1 vertical-slice-0 auto-ballooning pipeline on vector_bracket.pdf
    export_dir = ctx.workdir / "balloon_export"
    r_exp = ctx.run([fid_slice, "--balloon", str(vec_pdf), "--out", str(export_dir), "--json"])
    if r_exp.returncode != 0:
        raise TestFailure(f"fid-slice --balloon failed (exit {r_exp.returncode}): {r_exp.stdout} {r_exp.stderr}")
    env_exp = parse_single_line_json(r_exp.stdout)
    assert_envelope(env_exp, outcome="computed")

    csv_path = export_dir / "form3-starter.csv"
    pdf_out = export_dir / "ballooned.pdf"
    if not csv_path.is_file() or not pdf_out.is_file():
        raise TestFailure(f"missing ballooned.pdf or form3-starter.csv in {export_dir}")
    pdf_bytes = pdf_out.read_bytes()
    if len(pdf_bytes) <= 100 or not pdf_bytes.startswith(b"%PDF-"):
        raise TestFailure(f"invalid ballooned.pdf: len={len(pdf_bytes)} header={pdf_bytes[:16]!r}")
    with csv_path.open("r", encoding="utf-8", newline="") as fh:
        reader = csv.DictReader(fh)
        fieldnames = reader.fieldnames or []
        rows = list(reader)
    if len(fieldnames) < 3 or len(rows) != truth["callout_count"]:
        raise TestFailure(
            f"expected >=3 headers and {truth['callout_count']} rows in form3-starter.csv, "
            f"got headers={fieldnames!r} rows={len(rows)}"
        )

    # Scanned-only PDF must emit raster_only / OCR diagnostic and zero fabricated balloons
    scan_dir = ctx.workdir / "scan_export"
    r_scan = ctx.run([fid_slice, "--balloon", str(scan_pdf), "--out", str(scan_dir), "--json"])
    if r_scan.returncode != 0:
        raise TestFailure(f"fid-slice --balloon on scanned_only.pdf failed: {r_scan.stdout} {r_scan.stderr}")
    env_scan = parse_single_line_json(r_scan.stdout)
    assert_envelope(env_scan, outcome="computed")
    scan_res = env_scan.get("result")
    if not isinstance(scan_res, dict) or scan_res.get("proposals", []) != [] or scan_res.get("balloon_count", 0) != 0:
        raise TestFailure(f"expected zero fabricated balloons on scanned_only.pdf, got {scan_res!r}")
    scan_dump = json.dumps(scan_res).lower()
    if "report.raster_sheet_needs_ocr" not in scan_dump and "raster_only" not in scan_dump:
        raise TestFailure(f"expected report.raster_sheet_needs_ocr / raster_only on scanned_only.pdf, got {scan_res!r}")


@e2e_test(
    test_id="T4-S3",
    tier=4,
    feature="F4",
    description="Scenario 3: Autonomous Agent MCP Session & Progressive Disclosure Workflow (initialize -> search -> enable -> invoke -> taint -> EOF)",
)
def test_scenario_3_mcp_progressive_disclosure(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    idx = write_disclosure_profiles_index(ctx.workdir)
    msgs_in = mcp_init_messages() + [
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
        {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "tools_search_tools", "arguments": {"keyword": "release"}},
        },
        {
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {"name": "tools_enable_tools", "arguments": {"commands": ["rpt.release_report@1"]}},
        },
        {
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "tools_invoke_readonly",
                "arguments": {"command": "project.list_features@1", "args": {"principal": "human:override"}},
            },
        },
    ]
    rc, msgs, _, stderr = run_mcp_session(mcp, ["--headless", "--profiles-index", str(idx)], msgs_in, cwd=ctx.repo_root)
    if rc != 0:
        raise TestFailure(f"fid-mcp exited {rc}: {stderr}")
    if len(msgs) < 5:
        raise TestFailure(f"expected >=5 JSON-RPC messages (replies + list_changed notification), got {msgs!r}")

    r1 = next((m for m in msgs if m.get("id") == 1), None)
    r2 = next((m for m in msgs if m.get("id") == 2), None)
    r3 = next((m for m in msgs if m.get("id") == 3), None)
    r4 = next((m for m in msgs if m.get("id") == 4), None)
    r5 = next((m for m in msgs if m.get("id") == 5), None)
    for idx_id, reply in ((1, r1), (2, r2), (3, r3), (4, r4), (5, r5)):
        if not reply or "result" not in reply:
            raise TestFailure(f"missing result for JSON-RPC id={idx_id}: {msgs!r}")
    if "protocolVersion" not in r1["result"]:  # type: ignore[index]
        raise TestFailure(f"incomplete initialize result: {r1!r}")
    tools = r2["result"].get("tools", [])  # type: ignore[index]
    if not isinstance(tools, list) or not (1 <= len(tools) <= 15):
        raise TestFailure(f"unexpected tools/list count: {r2!r}")
    for call_id, call_rep in ((3, r3), (4, r4), (5, r5)):
        res_obj = call_rep["result"]  # type: ignore[index]
        if res_obj.get("isError"):
            raise TestFailure(f"JSON-RPC id={call_id} returned isError=true: {call_rep!r}")
        if not isinstance(res_obj.get("content"), list) or not res_obj["content"]:
            raise TestFailure(f"JSON-RPC id={call_id} returned empty content: {call_rep!r}")

    if "rpt.release_report@1" not in json.dumps(r3["result"]) and "rpt_release_report" not in json.dumps(r3["result"]):  # type: ignore[index]
        raise TestFailure(f"tools_search_tools('release') missing rpt.release_report@1: {r3!r}")
    if not any(m.get("method") == "notifications/tools/list_changed" for m in msgs):
        raise TestFailure(f"missing notifications/tools/list_changed: {msgs!r}")
    r5_dump = json.dumps(r5["result"])  # type: ignore[index]
    if "untrusted" not in r5_dump and "FID-UNTRUSTED-DATA-BEGIN" not in r5_dump:
        raise TestFailure(f"expected taint wrapping (untrusted / DELIM_OPEN) in id=5 result: {r5!r}")


@e2e_test(
    test_id="T4-S4",
    tier=4,
    feature="F5",
    description="Scenario 4: Spec-to-Site-to-Guide-to-QA Publishing Pipeline (scope -> site/build -> /guide/ WASM -> a11y + link-check -> worker CSP)",
)
def test_scenario_4_spec_site_guide_qa_pipeline(ctx: TestContext) -> None:
    outreach = ctx.require_bin("fid-outreach")
    wasm_bin = ctx.require_bin("fid-guide-wasm")

    scope_root = prepare_scope_root(ctx, "scope_check_t4")
    r_scope = ctx.run([sys.executable, str(scope_root / "tools" / "scope.py"), "check", "--root", str(scope_root)])
    if r_scope.returncode != 0:
        raise TestFailure(f"scope.py check failed: {r_scope.stdout}")

    copy_root = build_isolated_site(ctx)

    dist = copy_root / "site" / "dist"
    if not (dist / "guide" / "index.html").is_file():
        raise TestFailure("built site is missing /guide/index.html")

    # Preset digest parity
    topics_dir = ctx.repo_root / "guide" / "topics"
    for jf in sorted(topics_dir.glob("*.json")):
        topic = json.loads(jf.read_text(encoding="utf-8"))
        for preset in topic.get("presets", []):
            if "request" in preset and "expected_digest" in preset:
                r_w = ctx.run([wasm_bin], stdin_text=json.dumps(preset["request"]))
                if r_w.returncode != 0 or parse_single_line_json(r_w.stdout).get("result_digest") != preset["expected_digest"]:
                    raise TestFailure(f"WASM preset mismatch in {jf.name}")

    # qa a11y + link-check + worker.js CSP
    tokens_path = write_site_a11y_tokens(ctx.workdir)
    a11y_out = ctx.workdir / "a11y.json"
    r_a11y = ctx.run([outreach, "qa", "a11y", "--root", str(dist), "--tokens", str(tokens_path), "--out", str(a11y_out)])
    if r_a11y.returncode != 0 or not a11y_out.is_file():
        raise TestFailure(f"qa a11y failed (exit {r_a11y.returncode}): {r_a11y.stdout} {r_a11y.stderr}")
    a11y_rep = json.loads(a11y_out.read_text(encoding="utf-8"))
    if a11y_rep.get("result") != "pass" or a11y_rep.get("violations") != []:
        raise TestFailure(f"qa a11y did not pass cleanly: {a11y_rep!r}")

    r_lc = ctx.run([outreach, "link-check", "--edition", str(dist), "--json"])
    if r_lc.returncode != 0:
        raise TestFailure(f"link-check failed: {r_lc.stdout}")
    lc_obj = parse_single_line_json(r_lc.stdout)
    if lc_obj.get("ok") is False or lc_obj.get("result") == "fail":
        raise TestFailure(f"link-check reported failure: {lc_obj!r}")
    worker = (ctx.repo_root / "site" / "worker.js").read_text(encoding="utf-8")
    if "wasm-unsafe-eval" not in worker or "script-src 'none'" not in worker:
        raise TestFailure("worker.js CSP scoping incomplete")


@e2e_test(
    test_id="T4-S5",
    tier=4,
    feature="F6",
    description="Scenario 5: Nightly RDL & Release Verification Pipeline (tracim_public + xtask rel runbook-check/flow-sim + verify_guide)",
    capabilities_required=("c_toolchain",),
)
def test_scenario_5_rdl_and_release_pipeline(ctx: TestContext) -> None:
    wf = ctx.repo_root / ".github" / "workflows" / "tracim-public.yml"
    r_cfg = ctx.run([sys.executable, str(ctx.repo_root / "tools" / "rdl" / "tracim_public.py"), "check-config", "--workflow", str(wf)])
    if r_cfg.returncode != 0:
        raise TestFailure(f"tracim_public check-config failed: {r_cfg.stdout} {r_cfg.stderr}")
    r_rdl_tests = ctx.run([sys.executable, "-m", "unittest", "discover", "-s", "tools/rdl/tests", "-v"])
    if r_rdl_tests.returncode != 0:
        raise TestFailure(f"tools/rdl/tests failed: {r_rdl_tests.stdout} {r_rdl_tests.stderr}")

    tag = "verify-windows" if ctx.caps.is_windows else "verify-linux"
    r_vg = ctx.run([sys.executable, str(ctx.repo_root / "tools" / "verify_guide.py"), "--tag", tag])
    if r_vg.returncode != 0:
        raise TestFailure(f"verify_guide failed: {r_vg.stdout}")

    xtask = ctx.require_bin("fid-xtask")
    r_rb = ctx.run([xtask, "rel", "runbook-check", "--dir", str(ctx.repo_root / "release")])
    if r_rb.returncode != 0:
        raise TestFailure(f"rel runbook-check failed: {r_rb.stdout} {r_rb.stderr}")
    r_fs = ctx.run(
        [xtask, "rel", "flow-sim", "--tag", "v1.4.0-beta.1", "--fixtures", str(ctx.repo_root / "fixtures" / "rel" / "flow" / "pass")]
    )
    if r_fs.returncode != 0:
        raise TestFailure(f"rel flow-sim failed: {r_fs.stdout} {r_fs.stderr}")


@e2e_test(
    test_id="T4-S6",
    tier=4,
    feature="F7",
    description="Scenario 6: Cross-Implementation Canonicalization & Result Package Spec Conformance (fid canon vs Node -> check_spec_drift + v1.md -> pkg-conformance)",
)
def test_scenario_6_canon_spec_drift_and_pkg_conformance(ctx: TestContext) -> None:
    node = ctx.require_node()
    fid = ctx.require_bin("fid")
    conf = ctx.require_bin("fid-pkg-conformance")

    r_node = ctx.run([node, str(ctx.repo_root / "fixtures" / "core" / "jrn" / "canon" / "runner.mjs")])
    if r_node.returncode != 0:
        raise TestFailure(f"Node RFC 8785 oracle failed: {r_node.stdout}")

    sample_json = ctx.repo_root / "fixtures" / "canon" / "vectors" / "keys_utf16_order.json"
    sample_canon = (ctx.repo_root / "fixtures" / "canon" / "vectors" / "keys_utf16_order.canon").read_text(encoding="utf-8").strip()
    r_fid = ctx.run([fid, "canon", str(sample_json)])
    if r_fid.returncode != 0 or r_fid.stdout.strip() != sample_canon:
        raise TestFailure(f"fid canon mismatch on keys_utf16_order: {r_fid.stdout!r}")

    r_drift = ctx.run([sys.executable, str(ctx.repo_root / "fixtures" / "pkg" / "check_spec_drift.py")])
    if r_drift.returncode != 0:
        raise TestFailure(f"check_spec_drift.py failed: {r_drift.stdout}")

    v1_md = (ctx.repo_root / "docs" / "result-package" / "v1.md").read_text(encoding="utf-8")
    if "interp/registers/<sha256>.json" not in v1_md or "interp/rosters/<sha256>.json" not in v1_md:
        raise TestFailure("docs/result-package/v1.md missing interp/* members")

    r_conf = ctx.run([sys.executable, str(ctx.repo_root / "fixtures" / "pkg" / "run-conformance"), "--reasons", "--", str(conf)], timeout=180)
    if r_conf.returncode != 0:
        raise TestFailure(f"run-conformance failed: {r_conf.stdout}")


@e2e_test(
    test_id="T4-S7",
    tier=4,
    feature="F7",
    description="Scenario 7: Contributor & CI Governance Gate Workflow (xtask fmt/layers/lint -> doc-run -> D-042/D-052 templates -> generated/ parity)",
    capabilities_required=("c_toolchain",),
)
def test_scenario_7_contributor_and_governance_gates(ctx: TestContext) -> None:
    xtask = ctx.require_bin("fid-xtask")
    for sub in (["fmt", "--check", "--workspace", str(ctx.repo_root)], ["layers"], ["lint-crate-graph"]):
        r = ctx.run([xtask, *sub], timeout=180)
        if r.returncode != 0:
            raise TestFailure(f"xtask {' '.join(sub)} failed: {r.stdout} {r.stderr}")

    outreach = ctx.require_bin("fid-outreach")
    head = ctx.run(["git", "rev-parse", "HEAD"]).stdout.strip()
    orig = (ctx.repo_root / "docs" / "contributor" / "guide.md").read_text(encoding="utf-8")
    patched = re.sub(r"^revision:\s*[0-9a-f]+", f"revision: {head}", orig, count=1, flags=re.M)
    tmp_guide = ctx.workdir / "guide.md"
    tmp_guide.write_text(patched, encoding="utf-8")
    r_doc = ctx.run([outreach, "doc-run", "--guide", str(tmp_guide), "--repo", str(ctx.repo_root), "--json"], timeout=180)
    if r_doc.returncode != 0:
        raise TestFailure(f"doc-run failed: {r_doc.stdout} {r_doc.stderr}")

    pr_tpl = (ctx.repo_root / ".github" / "PULL_REQUEST_TEMPLATE.md").read_text(encoding="utf-8")
    if "generated/` is committed" in pr_tpl or "A person (not an agent or bot)" in pr_tpl:
        raise TestFailure(".github/PULL_REQUEST_TEMPLATE.md violates D-042/D-052")

    r_stats = ctx.run([sys.executable, str(ctx.repo_root / "tools" / "scope.py"), "stats"])
    m = re.search(r"built:\s*(\d+)\s+leaves", r_stats.stdout)
    if not m:
        raise TestFailure(f"failed to parse scope.py stats: {r_stats.stdout!r}")
    built_md = (ctx.repo_root / "generated" / "BUILT.md").read_text(encoding="utf-8")
    if f"**{m.group(1)}**" not in built_md:
        raise TestFailure(f"generated/BUILT.md stale vs scope.py stats ({m.group(1)})")
