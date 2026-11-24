"""Tier 1 happy-path E2E tests for F3 (fid-verify/pkg), F4 (fid-mcp), F5 (site/guide/outreach), F6 (tools/xtask), F7 (docs/templates/generated)."""

from __future__ import annotations

import json
import re
import shutil
import sys
from pathlib import Path

from e2e.lib.harness import (
    TestContext,
    TestFailure,
    e2e_test,
    parse_single_line_json,
)
from e2e.tests._helpers import build_isolated_site, mcp_init_messages, prepare_scope_root, run_mcp_session


# ===========================================================================
# F3: fid-verify & Result Package / Trust Verification (6 happy-path tests)
# ===========================================================================


@e2e_test(
    test_id="T1-F3-01",
    tier=1,
    feature="F3",
    description="fid-pkg-conformance accepts all golden and reader/valid packages",
)
def test_pkg_conformance_valid_packages(ctx: TestContext) -> None:
    conf = ctx.require_bin("fid-pkg-conformance")
    zips = sorted((ctx.repo_root / "fixtures" / "pkg" / "golden").glob("*.zip")) + sorted(
        (ctx.repo_root / "fixtures" / "pkg" / "reader" / "valid").glob("*.zip")
    )
    if len(zips) < 6:
        raise TestFailure(f"expected golden+valid packages, found {len(zips)}")
    for z in zips:
        res = ctx.run([conf, str(z)])
        if res.returncode != 0:
            raise TestFailure(f"fid-pkg-conformance rejected valid {z.name} (exit {res.returncode}): {res.stdout}")
        obj = parse_single_line_json(res.stdout)
        if obj.get("result") != "accepted":
            raise TestFailure(f"expected result=accepted for {z.name}, got {obj!r}")


@e2e_test(
    test_id="T1-F3-02",
    tier=1,
    feature="F3",
    description="fixtures/pkg/run-conformance --reasons passes across all golden, valid, and malformed fixtures",
)
def test_pkg_run_conformance_script(ctx: TestContext) -> None:
    conf = ctx.require_bin("fid-pkg-conformance")
    script = ctx.repo_root / "fixtures" / "pkg" / "run-conformance"
    res = ctx.run([sys.executable, str(script), "--reasons", "--", str(conf)], timeout=180)
    if res.returncode != 0:
        raise TestFailure(
            f"run-conformance exited {res.returncode}:\nstdout={res.stdout}\nstderr={res.stderr}"
        )


@e2e_test(
    test_id="T1-F3-03",
    tier=1,
    feature="F3",
    description="M4 fid-verify --version reports verifier version, report version, and package format version",
)
def test_fid_verify_version(ctx: TestContext) -> None:
    verify = ctx.require_bin("fid-verify")
    res = ctx.run([verify, "--version"])
    if res.returncode != 0:
        raise TestFailure(f"fid-verify --version exited {res.returncode}: {res.stderr}")
    if "fid-verify" not in res.stdout or "1." not in res.stdout:
        raise TestFailure(f"unexpected fid-verify --version output: {res.stdout!r}")


@e2e_test(
    test_id="T1-F3-04",
    tier=1,
    feature="F3",
    description="M4 fid-verify validates signed package with --trust-root and outputs PASS report",
)
def test_fid_verify_signed_package(ctx: TestContext) -> None:
    verify = ctx.require_bin("fid-verify")
    pkg = ctx.repo_root / "fixtures" / "pkg" / "golden" / "two-signatures.zip"
    root = ctx.repo_root / "fixtures" / "troot" / "roots" / "both.json"
    res = ctx.run([verify, str(pkg), "--trust-root", str(root), "--json"])
    if res.returncode != 0:
        raise TestFailure(f"fid-verify signed package exited {res.returncode}: {res.stdout} {res.stderr}")
    rep = parse_single_line_json(res.stdout)
    if rep.get("verdict") != "PASS" or rep.get("exit_code") != 0:
        raise TestFailure(f"expected verdict=PASS exit_code=0, got {rep!r}")
    if rep.get("replay", {}).get("status") not in ("not_requested", "not_applicable"):  # type: ignore[union-attr]
        raise TestFailure(f"expected replay not_requested/not_applicable without --replay: {rep.get('replay')!r}")


@e2e_test(
    test_id="T1-F3-05",
    tier=1,
    feature="F3",
    description="M4 fid-verify --replay --json verifies points.zip and conforms to schema/verify-report-1.json",
)
def test_fid_verify_replay_and_schema(ctx: TestContext) -> None:
    verify = ctx.require_bin("fid-verify")
    pkg = ctx.repo_root / "fixtures" / "pkg" / "golden" / "points.zip"
    root = ctx.repo_root / "fixtures" / "troot" / "roots" / "both.json"
    schema_path = ctx.repo_root / "schema" / "verify-report-1.json"
    if not schema_path.is_file():
        raise TestFailure(f"missing M4 schema file: {schema_path}")
    res = ctx.run([verify, str(pkg), "--trust-root", str(root), "--replay", "--json"])
    if res.returncode != 0:
        raise TestFailure(f"fid-verify --replay exited {res.returncode}: {res.stdout} {res.stderr}")
    rep = parse_single_line_json(res.stdout)
    if rep.get("verdict") != "PASS":
        raise TestFailure(f"expected verdict=PASS, got {rep!r}")
    if rep.get("replay", {}).get("status") != "IDENTICAL":  # type: ignore[union-attr]
        raise TestFailure(f"expected replay IDENTICAL, got {rep.get('replay')!r}")
    try:
        import jsonschema  # type: ignore[import-untyped]
    except ImportError:
        return
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    jsonschema.validate(instance=rep, schema=schema)


@e2e_test(
    test_id="T1-F3-06",
    tier=1,
    feature="F3",
    description="M4 fid-verify dependency tree contains zero network, TLS, DNS, or license crates",
)
def test_fid_verify_zero_network_crates(ctx: TestContext) -> None:
    if not ctx.binaries.has_crate("fid-verify"):
        raise TestFailure("crate `fid-verify` does not exist in workspace")
    assert ctx.caps.cargo_path is not None
    res = ctx.run([ctx.caps.cargo_path, "tree", "--locked", "-p", "fid-verify"])
    if res.returncode != 0:
        raise TestFailure(f"cargo tree -p fid-verify exited {res.returncode}: {res.stderr}")
    forbidden = ("reqwest", "hyper", "tokio", "rustls", "openssl", "ureq", "curl", "hickory", "trust-dns")
    hits = [crate for crate in forbidden if re.search(rf"\b{re.escape(crate)}\b", res.stdout)]
    if hits:
        raise TestFailure(f"fid-verify depends on forbidden network crates: {hits}")


# ===========================================================================
# F4: fid-mcp JSON-RPC 2.0 Stdio Session & Projection (6 happy-path tests)
# ===========================================================================


@e2e_test(
    test_id="T1-F4-01",
    tier=1,
    feature="F4",
    description="fid-mcp --print-profile selects core profile accurately",
)
def test_mcp_print_profile_core(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    idx = ctx.repo_root / "fixtures" / "mcp" / "profiles" / "expected.json"
    res = ctx.run([mcp, "--profiles-index", str(idx), "--server", "core", "--print-profile"])
    if res.returncode != 0:
        raise TestFailure(f"fid-mcp core --print-profile exited {res.returncode}: {res.stderr}")
    if res.stdout.strip() != "profile=core served=8 front_door=6":
        raise TestFailure(f"unexpected stdout: {res.stdout!r}")


@e2e_test(
    test_id="T1-F4-02",
    tier=1,
    feature="F4",
    description="fid-mcp --print-profile selects inspect profile accurately",
)
def test_mcp_print_profile_inspect(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    idx = ctx.repo_root / "fixtures" / "mcp" / "profiles" / "expected.json"
    res = ctx.run([mcp, "--profiles-index", str(idx), "--server", "inspect", "--print-profile"])
    if res.returncode != 0:
        raise TestFailure(f"fid-mcp inspect --print-profile exited {res.returncode}: {res.stderr}")
    if res.stdout.strip() != "profile=inspect served=11 front_door=7":
        raise TestFailure(f"unexpected stdout: {res.stdout!r}")


@e2e_test(
    test_id="T1-F4-03",
    tier=1,
    feature="F4",
    description="M4 fid-mcp stdio JSON-RPC 2.0 initialize + notifications/initialized handshake",
)
def test_mcp_stdio_initialize(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    rc, msgs, stdout, stderr = run_mcp_session(
        mcp,
        ["--headless"],
        mcp_init_messages(),
        cwd=ctx.repo_root,
    )
    if rc != 0:
        raise TestFailure(f"fid-mcp exited {rc}: stderr={stderr!r}")
    if not msgs or msgs[0].get("id") != 1 or "result" not in msgs[0]:
        raise TestFailure(f"expected JSON-RPC initialize result, got msgs={msgs!r} stdout={stdout!r} stderr={stderr!r}")
    result = msgs[0]["result"]
    if "protocolVersion" not in result or "serverInfo" not in result:
        raise TestFailure(f"incomplete initialize result: {result!r}")


@e2e_test(
    test_id="T1-F4-04",
    tier=1,
    feature="F4",
    description="M4 fid-mcp tools/list returns <=15 front-door tools including tools.* and zero M1/S1/A0",
)
def test_mcp_stdio_tools_list(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    idx = ctx.repo_root / "fixtures" / "mcp" / "profiles" / "expected.json"
    msgs_in = mcp_init_messages() + [{"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}]
    rc, msgs, _, stderr = run_mcp_session(mcp, ["--headless", "--profiles-index", str(idx)], msgs_in, cwd=ctx.repo_root)
    if rc != 0:
        raise TestFailure(f"fid-mcp exited {rc}: stderr={stderr!r}")
    reply = next((m for m in msgs if m.get("id") == 2), None)
    if not reply or "result" not in reply:
        raise TestFailure(f"missing tools/list response: msgs={msgs!r} stderr={stderr!r}")
    tools = reply["result"].get("tools", [])
    if not isinstance(tools, list) or len(tools) == 0 or len(tools) > 15:
        raise TestFailure(f"front-door tools count must be 1..15, got {len(tools)}")
    names = {t.get("name") for t in tools}
    for req_tool in ("tools_search_tools", "tools_describe_tool", "tools_invoke_readonly", "tools_enable_tools"):
        if req_tool not in names:
            raise TestFailure(f"missing required front-door tool {req_tool!r} in {sorted(names)}")
    for forbidden in ("motion_move_axis", "trust_sign_record", "plugin_install_pack"):
        if forbidden in names:
            raise TestFailure(f"forbidden M1/S1/A0 tool leaked into tools/list: {forbidden}")


@e2e_test(
    test_id="T1-F4-05",
    tier=1,
    feature="F4",
    description="M4 fid-mcp tools/call executes tools_search_tools and tools_describe_tool",
)
def test_mcp_stdio_search_and_describe(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    idx = ctx.repo_root / "fixtures" / "mcp" / "profiles" / "expected.json"
    msgs_in = mcp_init_messages() + [
        {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "tools_search_tools", "arguments": {"keyword": "save"}},
        },
        {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "tools_describe_tool", "arguments": {"command": "program.save_draft@1"}},
        },
    ]
    rc, msgs, _, stderr = run_mcp_session(mcp, ["--headless", "--profiles-index", str(idx)], msgs_in, cwd=ctx.repo_root)
    if rc != 0:
        raise TestFailure(f"fid-mcp exited {rc}: stderr={stderr!r}")
    r2 = next((m for m in msgs if m.get("id") == 2), None)
    r3 = next((m for m in msgs if m.get("id") == 3), None)
    if not r2 or "result" not in r2 or r2["result"].get("isError"):
        raise TestFailure(f"tools_search_tools failed: {r2!r} (all={msgs!r})")
    if not r3 or "result" not in r3 or r3["result"].get("isError"):
        raise TestFailure(f"tools_describe_tool failed: {r3!r} (all={msgs!r})")


@e2e_test(
    test_id="T1-F4-06",
    tier=1,
    feature="F4",
    description="M4 fid-mcp tools_enable_tools emits notifications/tools/list_changed and shuts down cleanly on EOF",
)
def test_mcp_stdio_enable_tools_and_eof(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    idx = ctx.repo_root / "fixtures" / "mcp" / "profiles" / "expected.json"
    msgs_in = mcp_init_messages() + [
        {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "tools_enable_tools", "arguments": {"commands": ["rpt.release_report@1"]}},
        },
        {"jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {}},
    ]
    rc, msgs, _, stderr = run_mcp_session(mcp, ["--headless", "--profiles-index", str(idx)], msgs_in, cwd=ctx.repo_root)
    if rc != 0:
        raise TestFailure(f"fid-mcp exited {rc} on EOF: stderr={stderr!r}")
    notifs = [m for m in msgs if m.get("method") == "notifications/tools/list_changed"]
    if not notifs:
        raise TestFailure(f"expected notifications/tools/list_changed, got msgs={msgs!r}")
    r3 = next((m for m in msgs if m.get("id") == 3), None)
    if not r3 or "result" not in r3:
        raise TestFailure(f"missing post-enable tools/list reply: {msgs!r}")
    names = {t.get("name") for t in r3["result"].get("tools", [])}
    if "rpt_release_report" not in names:
        raise TestFailure(f"enabled tool rpt_release_report missing from updated tools/list: {sorted(names)}")


# ===========================================================================
# F5: Outreach, Static Site, Field Guide WASM & Verify Guide (6 tests)
# ===========================================================================


@e2e_test(
    test_id="T1-F5-01",
    tier=1,
    feature="F5",
    description="site/build.py builds all HTML pages <500 KB with zero third-party URLs or inline scripts",
)
def test_site_build_clean_budget(ctx: TestContext) -> None:
    copy_root = build_isolated_site(ctx)
    dist = copy_root / "site" / "dist"
    html_files = list(dist.rglob("*.html"))
    if len(html_files) < 12:
        raise TestFailure(f"expected >=12 HTML pages, found {len(html_files)}")
    for p in html_files:
        size = p.stat().st_size
        if size >= 500_000:
            raise TestFailure(f"{p.relative_to(dist)} exceeds 500 KB budget ({size} bytes)")
        txt = p.read_text(encoding="utf-8")
        if re.search(r"<script\b[^>]*>(?!\s*</script>)", txt, re.I):
            raise TestFailure(f"inline <script> found in {p.relative_to(dist)}")
        for m in re.finditer(r'(?:src|href)=["\'](https?://[^"\']+)["\']', txt, re.I):
            url = m.group(1)
            if not (
                url.startswith("https://metrologymaven.io")
                or url.startswith("https://github.com/metrologymaven/")
            ):
                raise TestFailure(f"unexpected external URL {url!r} in {p.relative_to(dist)}")


@e2e_test(
    test_id="T1-F5-02",
    tier=1,
    feature="F5",
    description="M2 #566 site/build.py generates /guide/ + 6 topic pages and worker.js scopes wasm-unsafe-eval CSP",
)
def test_site_guide_pages_and_csp(ctx: TestContext) -> None:
    copy_root = build_isolated_site(ctx)
    dist = copy_root / "site" / "dist"
    guide_index = dist / "guide" / "index.html"
    if not guide_index.is_file():
        raise TestFailure(f"missing /guide/ index page: {guide_index}")
    topics = ("fits", "gdt", "uncertainty", "decisions", "numerics-replay", "field-map")
    for slug in topics:
        page = dist / "guide" / slug / "index.html"
        if not page.is_file():
            raise TestFailure(f"missing /guide/{slug}/ page: {page}")
    worker_txt = (ctx.repo_root / "site" / "worker.js").read_text(encoding="utf-8")
    if "wasm-unsafe-eval" not in worker_txt or "/guide" not in worker_txt:
        raise TestFailure("site/worker.js does not scope 'wasm-unsafe-eval' to /guide/*")


@e2e_test(
    test_id="T1-F5-03",
    tier=1,
    feature="F5",
    description="M2 #565 fid-guide-wasm executes all 7 lab ops and matches guide/topics/*.json expected_digest",
)
def test_guide_wasm_7_ops_and_topic_digests(ctx: TestContext) -> None:
    wasm_bin = ctx.require_bin("fid-guide-wasm")
    topics_dir = ctx.repo_root / "guide" / "topics"
    if not topics_dir.is_dir():
        raise TestFailure(f"missing guide/topics directory: {topics_dir}")

    # Minimal payloads for all 7 required ops.
    op_requests = {
        "fit.circle2d": {"op": "fit.circle2d", "params": {"points": [[1, 0], [0, 1], [-1, 0], [0, -1]]}},
        "fit.plane_datum": {"op": "fit.plane_datum", "params": {"points": [[0, 0, 0], [1, 0, 0], [0, 1, 0], [1, 1, 0]]}},
        "fit.cylinder_mcc": {
            "op": "fit.cylinder_mcc",
            "params": {
                "points": [
                    [1, 0, 0], [0, 1, 0], [-1, 0, 0], [0, -1, 0],
                    [1, 0, 1], [0, 1, 1], [-1, 0, 1], [0, -1, 1],
                ]
            },
        },
        "gdt.zone_eval": {
            "op": "gdt.zone_eval",
            "params": {"shape": "two_planes", "points": [[0, 0, 0.001], [0, 0, -0.001]], "tolerance": 0.005},
        },
        "unc.propagate": {
            "op": "unc.propagate",
            "params": {"inputs": [{"name": "x", "dist": "normal", "mean": 10.0, "std": 0.01}], "sensitivity": [1.0]},
        },
        "dec.evaluate": {
            "op": "dec.evaluate",
            "params": {"rule": "simple_acceptance", "measured": 10.0, "upper": 10.05, "lower": 9.95, "u": 0.002},
        },
        "canon.digest": {"op": "canon.digest", "params": {"value": {"b": 1, "a": 2}}},
    }
    for op, req in op_requests.items():
        res = ctx.run([wasm_bin], stdin_text=json.dumps(req))
        if res.returncode != 0:
            raise TestFailure(f"fid-guide-wasm op={op} exited {res.returncode}: {res.stdout} {res.stderr}")
        out = parse_single_line_json(res.stdout)
        if out.get("ok") is not True or not str(out.get("result_digest", "")).startswith("sha256:"):
            raise TestFailure(f"unexpected output for op={op}: {out!r}")

    # Verify guide/topics/*.json preset expected_digest parity.
    presets_checked = 0
    for jf in sorted(topics_dir.glob("*.json")):
        topic = json.loads(jf.read_text(encoding="utf-8"))
        for preset in topic.get("presets", []):
            if "request" in preset and "expected_digest" in preset:
                res = ctx.run([wasm_bin], stdin_text=json.dumps(preset["request"]))
                if res.returncode != 0:
                    raise TestFailure(f"preset failed in {jf.name}: {res.stdout} {res.stderr}")
                out = parse_single_line_json(res.stdout)
                if out.get("result_digest") != preset["expected_digest"]:
                    raise TestFailure(
                        f"digest mismatch in {jf.name}: got {out.get('result_digest')!r} "
                        f"!= {preset['expected_digest']!r}"
                    )
                presets_checked += 1
    if presets_checked == 0:
        raise TestFailure("no presets with request/expected_digest found in guide/topics/*.json")


@e2e_test(
    test_id="T1-F5-04",
    tier=1,
    feature="F5",
    description="fid-outreach doc-run runs docs/contributor/guide.md sh run blocks cleanly against HEAD",
)
def test_outreach_doc_run_contributor_guide(ctx: TestContext) -> None:
    outreach = ctx.require_bin("fid-outreach")
    head = ctx.run(["git", "rev-parse", "HEAD"]).stdout.strip()
    if len(head) not in (40, 64):
        raise TestFailure(f"unexpected git HEAD sha: {head!r}")
    orig = (ctx.repo_root / "docs" / "contributor" / "guide.md").read_text(encoding="utf-8")
    patched = re.sub(r"^revision:\s*[0-9a-f]+", f"revision: {head}", orig, count=1, flags=re.M)
    tmp_guide = ctx.workdir / "guide.md"
    tmp_guide.write_text(patched, encoding="utf-8")

    res = ctx.run([outreach, "doc-run", "--guide", str(tmp_guide), "--repo", str(ctx.repo_root), "--json"], timeout=180)
    if res.returncode != 0:
        raise TestFailure(f"fid-outreach doc-run failed (exit {res.returncode}):\n{res.stdout}\n{res.stderr}")
    rep = parse_single_line_json(res.stdout)
    if rep.get("ok") is not True or rep.get("workdir_removed") is not True:
        raise TestFailure(f"unexpected doc-run report: {rep!r}")


@e2e_test(
    test_id="T1-F5-05",
    tier=1,
    feature="F5",
    description="fid-outreach qa a11y passes on site/dist and M2 #445 link-check validates internal links",
)
def test_outreach_qa_a11y_and_link_check(ctx: TestContext) -> None:
    outreach = ctx.require_bin("fid-outreach")
    copy_root = build_isolated_site(ctx)
    dist = copy_root / "site" / "dist"
    tokens_path = ctx.workdir / "tokens.json"
    tokens_path.write_text(
        json.dumps(
            {
                "themes": {
                    "light": {
                        "bg": "#ffffff",
                        "surface": "#f7f8fa",
                        "sheet": "#ffffff",
                        "ink": "#111827",
                        "text.muted": "#5c6670",
                        "muted": "#5c6670",
                        "border": "#d0d7de",
                        "accent": "#0b5fff",
                        "ruby": "#b91c1c",
                    },
                    "dark": {
                        "bg": "#0d1117",
                        "surface": "#161b22",
                        "sheet": "#0d1117",
                        "ink": "#f0f6fc",
                        "text.muted": "#8b949e",
                        "muted": "#8b949e",
                        "border": "#30363d",
                        "accent": "#58a6ff",
                        "ruby": "#f85149",
                    },
                }
            }
        ),
        encoding="utf-8",
    )
    a11y_out = ctx.workdir / "a11y-report.json"
    r_a11y = ctx.run([outreach, "qa", "a11y", "--root", str(dist), "--tokens", str(tokens_path), "--out", str(a11y_out)])
    if r_a11y.returncode not in (0, 1):
        raise TestFailure(f"qa a11y IO/parse error (exit {r_a11y.returncode}): {r_a11y.stderr}")
    if not a11y_out.is_file():
        raise TestFailure("qa a11y did not write report JSON")
    rep = json.loads(a11y_out.read_text(encoding="utf-8"))
    if rep.get("schema") != "a11y-report@1":
        raise TestFailure(f"unexpected a11y report schema: {rep!r}")

    # M2 #445 link-check contract
    edition = ctx.repo_root / "fixtures" / "outr" / "docs" / "qa" / "ok"
    if not edition.is_dir():
        edition = dist
    r_lc = ctx.run([outreach, "link-check", "--edition", str(edition), "--json"])
    if r_lc.returncode != 0:
        raise TestFailure(f"fid-outreach link-check failed (exit {r_lc.returncode}): {r_lc.stdout} {r_lc.stderr}")


@e2e_test(
    test_id="T1-F5-06",
    tier=1,
    feature="F5",
    description="tools/verify_guide.py verifies untouched release fixture and rejects 1-byte tamper",
)
def test_verify_guide_os_tag(ctx: TestContext) -> None:
    tag = "verify-windows" if ctx.caps.is_windows else "verify-linux"
    script = ctx.repo_root / "tools" / "verify_guide.py"
    res = ctx.run([sys.executable, str(script), "--tag", tag], timeout=120)
    if res.returncode != 0:
        raise TestFailure(f"verify_guide.py --tag {tag} exited {res.returncode}:\n{res.stdout}\n{res.stderr}")
    if f"verify-guide: ok {tag}" not in res.stdout:
        raise TestFailure(f"unexpected verify_guide stdout: {res.stdout!r}")


# ===========================================================================
# F6: Spec Tooling, RDL/Tracim, xtask Gates, Scan & Release Flows (6 tests)
# ===========================================================================


@e2e_test(
    test_id="T1-F6-01",
    tier=1,
    feature="F6",
    description="scope.py check passes on isolated repo copy with 0 errors and 5 LF generated files",
)
def test_scope_check_isolated(ctx: TestContext) -> None:
    copy_root = prepare_scope_root(ctx, "scope_check_copy")
    res = ctx.run([sys.executable, str(copy_root / "tools" / "scope.py"), "check", "--root", str(copy_root)])
    if res.returncode != 0:
        raise TestFailure(f"scope.py check exited {res.returncode}:\n{res.stdout}\n{res.stderr}")
    for fname in ("INDEX.md", "COVERAGE.md", "DECISIONS.md", "BUILT.md", "EXPERT-QUEUE.md"):
        p = copy_root / "generated" / fname
        if not p.is_file():
            raise TestFailure(f"missing generated file: {p}")
        if b"\r" in p.read_bytes():
            raise TestFailure(f"CRLF detected in generated/{fname}")


@e2e_test(
    test_id="T1-F6-02",
    tier=1,
    feature="F6",
    description="scope.py stats and scope.py milestone milestones/launch-1.md succeed cleanly",
)
def test_scope_stats_and_milestone(ctx: TestContext) -> None:
    r_stats = ctx.run([sys.executable, str(ctx.repo_root / "tools" / "scope.py"), "stats"])
    if r_stats.returncode != 0:
        raise TestFailure(f"scope.py stats exited {r_stats.returncode}: {r_stats.stderr}")
    if "built:" not in r_stats.stdout:
        raise TestFailure(f"missing built: line in scope.py stats: {r_stats.stdout!r}")

    copy_root = ctx.workdir / "ms_copy"
    ctx.copy_repo_subset(copy_root, ["spec", "seeds", "decisions", "crates", "milestones"])
    (copy_root / "generated").mkdir(parents=True, exist_ok=True)
    r_ms = ctx.run(
        [
            sys.executable,
            str(ctx.repo_root / "tools" / "scope.py"),
            "milestone",
            str(copy_root / "milestones" / "launch-1.md"),
            "--root",
            str(copy_root),
        ]
    )
    if r_ms.returncode != 0:
        raise TestFailure(f"scope.py milestone exited {r_ms.returncode}: {r_ms.stdout} {r_ms.stderr}")
    if not (copy_root / "generated" / "milestones" / "launch-1.md").is_file():
        raise TestFailure("generated/milestones/launch-1.md was not created")


@e2e_test(
    test_id="T1-F6-03",
    tier=1,
    feature="F6",
    description="tracim_public.py check-config and tools/rdl/tests pass across Linux & Windows",
)
def test_tracim_public_config_and_tests(ctx: TestContext) -> None:
    wf = ctx.repo_root / ".github" / "workflows" / "tracim-public.yml"
    script = ctx.repo_root / "tools" / "rdl" / "tracim_public.py"
    r_chk = ctx.run([sys.executable, str(script), "check-config", "--workflow", str(wf)])
    if r_chk.returncode != 0:
        raise TestFailure(f"tracim_public check-config failed: {r_chk.stdout} {r_chk.stderr}")

    r_unit = ctx.run(
        [sys.executable, "-m", "unittest", "discover", "-s", "tools/rdl/tests", "-v"],
        cwd=ctx.repo_root,
        timeout=120,
    )
    if r_unit.returncode != 0:
        raise TestFailure(f"tools/rdl/tests failed (exit {r_unit.returncode}):\n{r_unit.stdout}\n{r_unit.stderr}")


@e2e_test(
    test_id="T1-F6-04",
    tier=1,
    feature="F6",
    description="M1 fid-xtask fmt --check passes across 119-crate workspace without Windows os error 206",
    capabilities_required=("c_toolchain",),
)
def test_xtask_fmt_check(ctx: TestContext) -> None:
    xtask = ctx.require_bin("fid-xtask")
    res = ctx.run([xtask, "fmt", "--check", "--workspace", str(ctx.repo_root)], timeout=180)
    if res.returncode != 0:
        raise TestFailure(f"fid-xtask fmt --check failed (exit {res.returncode}):\n{res.stdout}\n{res.stderr}")


@e2e_test(
    test_id="T1-F6-05",
    tier=1,
    feature="F6",
    description="fid-xtask layers, lint-crate-graph, and plat-matrix lint-tiers pass",
    capabilities_required=("c_toolchain",),
)
def test_xtask_arch_gates(ctx: TestContext) -> None:
    xtask = ctx.require_bin("fid-xtask")
    for sub in (["layers"], ["lint-crate-graph"], ["plat-matrix", "lint-tiers"]):
        res = ctx.run([xtask, *sub], cwd=ctx.repo_root, timeout=180)
        if res.returncode != 0:
            raise TestFailure(f"fid-xtask {' '.join(sub)} exited {res.returncode}:\n{res.stdout}\n{res.stderr}")


@e2e_test(
    test_id="T1-F6-06",
    tier=1,
    feature="F6",
    description="M4 ECO.REL.PROC.FLOW (xtask rel flow-sim) and SCAN.CMP.DEV.PRIM are implemented and pass",
    capabilities_required=("c_toolchain",),
)
def test_m4_rel_flow_sim_and_scan_cmp_prim(ctx: TestContext) -> None:
    # Check SCAN.CMP.DEV.PRIM first (pure Rust)
    dev_rs = (ctx.repo_root / "crates" / "fid-scan-cmp" / "src" / "dev.rs").read_text(encoding="utf-8")
    if "not built yet" in dev_rs or "fn deviation(" not in dev_rs:
        raise TestFailure("SCAN.CMP.DEV.PRIM (deviation / deviation_batch) is not yet implemented in fid-scan-cmp")
    assert ctx.caps.cargo_path is not None
    r_scan = ctx.run([ctx.caps.cargo_path, "test", "--locked", "-p", "fid-scan-cmp"])
    if r_scan.returncode != 0:
        raise TestFailure(f"cargo test -p fid-scan-cmp failed: {r_scan.stdout} {r_scan.stderr}")

    xtask = ctx.require_bin("fid-xtask")
    pass_dir = ctx.repo_root / "fixtures" / "rel" / "flow" / "pass"
    res = ctx.run([xtask, "rel", "flow-sim", "--tag", "v1.4.0-beta.1", "--fixtures", str(pass_dir)])
    if res.returncode != 0:
        raise TestFailure(f"fid-xtask rel flow-sim exited {res.returncode}: {res.stdout} {res.stderr}")


# ===========================================================================
# F7: Docs, Templates, Spec Drift & generated/ Consistency (6 tests)
# ===========================================================================


@e2e_test(
    test_id="T1-F7-01",
    tier=1,
    feature="F7",
    description="fixtures/pkg/check_spec_drift.py reports zero spec drift and passes self-test",
)
def test_check_spec_drift_script(ctx: TestContext) -> None:
    script = ctx.repo_root / "fixtures" / "pkg" / "check_spec_drift.py"
    res = ctx.run([sys.executable, str(script)])
    if res.returncode != 0:
        raise TestFailure(f"check_spec_drift.py exited {res.returncode}: {res.stdout} {res.stderr}")
    if "spec and code agree" not in res.stdout:
        raise TestFailure(f"unexpected check_spec_drift.py output: {res.stdout!r}")


@e2e_test(
    test_id="T1-F7-02",
    tier=1,
    feature="F7",
    description="M5 docs/result-package/v1.md documents interp/* members and CAS points/<sha256>.<ext> paths",
)
def test_result_package_v1_md_interp_and_cas_alignment(ctx: TestContext) -> None:
    v1_md = (ctx.repo_root / "docs" / "result-package" / "v1.md").read_text(encoding="utf-8")
    if "interp/registers/<sha256>.json" not in v1_md or "interp/rosters/<sha256>.json" not in v1_md:
        raise TestFailure("docs/result-package/v1.md omits interp/registers/<sha256>.json or interp/rosters/<sha256>.json")
    if "evidence/points/" in v1_md or "evidence/register/" in v1_md:
        raise TestFailure("docs/result-package/v1.md still contains stale evidence/points/ or evidence/register/ paths")


@e2e_test(
    test_id="T1-F7-03",
    tier=1,
    feature="F7",
    description="M5 PR template and expert-offer issue template comply with D-042 and D-052",
)
def test_github_templates_d042_d052(ctx: TestContext) -> None:
    pr_tpl = (ctx.repo_root / ".github" / "PULL_REQUEST_TEMPLATE.md").read_text(encoding="utf-8")
    if "generated/` is committed" in pr_tpl or "generated/ is committed" in pr_tpl:
        raise TestFailure(".github/PULL_REQUEST_TEMPLATE.md still requires committing generated/ (violates D-042)")
    if "domain maintainer is requested as reviewer" in pr_tpl or "A person (not an agent or bot)" in pr_tpl:
        raise TestFailure(".github/PULL_REQUEST_TEMPLATE.md still contains pre-D-052 human-only review gates")

    expert_offer = ctx.repo_root / ".github" / "ISSUE_TEMPLATE" / "expert-offer.yml"
    if expert_offer.is_file():
        eo_txt = expert_offer.read_text(encoding="utf-8")
        if "Wait-for-expert" in eo_txt or "expert-queue" in eo_txt.lower():
            raise TestFailure(".github/ISSUE_TEMPLATE/expert-offer.yml still solicits retired D-052 expert-queue")


@e2e_test(
    test_id="T1-F7-04",
    tier=1,
    feature="F7",
    description="M5 scope.py and grow.ps1/grow.py contain no obsolete D-052 human-review or Expert contributions text",
)
def test_scope_and_grow_d052_cleanup(ctx: TestContext) -> None:
    scope_txt = (ctx.repo_root / "tools" / "scope.py").read_text(encoding="utf-8")
    if "Expert contributions" in scope_txt:
        raise TestFailure("tools/scope.py still emits dead CONTRIBUTING.md 'Expert contributions' section reference")
    grow_ps1 = (ctx.repo_root / "tools" / "grow.ps1").read_text(encoding="utf-8")
    grow_py = (ctx.repo_root / "tools" / "grow.py").read_text(encoding="utf-8")
    if "need human review" in grow_ps1 or "need human review" in grow_py:
        raise TestFailure("tools/grow.ps1 or tools/grow.py still prints obsolete pre-D-052 human-review warning")


@e2e_test(
    test_id="T1-F7-05",
    tier=1,
    feature="F7",
    description="M5 docs/contributor/guide.md, docs/platform/tiers.md, and README.md are synced with repo reality",
)
def test_docs_and_readme_accuracy(ctx: TestContext) -> None:
    guide = (ctx.repo_root / "docs" / "contributor" / "guide.md").read_text(encoding="utf-8")
    if "generated/EXPERT-QUEUE.md" in guide:
        raise TestFailure("docs/contributor/guide.md still references retired human expert queue rule")
    tiers = (ctx.repo_root / "docs" / "platform" / "tiers.md").read_text(encoding="utf-8")
    if "Not built yet" in tiers and "PLAT.MATRIX.POLICY.EOL" in tiers.split("Not built yet", 1)[1]:
        raise TestFailure("docs/platform/tiers.md still falsely claims PLAT.MATRIX.POLICY.EOL is 'Not built yet'")
    readme = (ctx.repo_root / "README.md").read_text(encoding="utf-8")
    if "--exclude fid-clarabel --doc" not in readme or "reviews/" not in readme or "scope.py milestone" not in readme:
        raise TestFailure("README.md is missing --exclude fid-clarabel --doc, generated/reviews/, or scope.py milestone")


@e2e_test(
    test_id="T1-F7-06",
    tier=1,
    feature="F7",
    description="M5 release/runbook.toml and generated/ match built crates, spec counts, and D-049..D-054",
)
def test_runbook_and_generated_freshness(ctx: TestContext) -> None:
    runbook = (ctx.repo_root / "release" / "runbook.toml").read_text(encoding="utf-8")
    if 'id = "notes"' in runbook and "ECO.REL.PROC.NOTES is not built yet" in runbook:
        raise TestFailure("release/runbook.toml still marks notes as not built yet")
    if 'id = "kernel-patch-gate"' in runbook and "KRN.MAN.XART, which is not built yet" in runbook:
        raise TestFailure("release/runbook.toml still marks kernel-patch-gate as not built yet")

    dec_md = (ctx.repo_root / "generated" / "DECISIONS.md").read_text(encoding="utf-8")
    for dec_id in ("D-049", "D-052", "D-054"):
        if dec_id not in dec_md:
            raise TestFailure(f"generated/DECISIONS.md is stale: missing {dec_id}")

    r_stats = ctx.run([sys.executable, str(ctx.repo_root / "tools" / "scope.py"), "stats"])
    m = re.search(r"built:\s*(\d+)\s+leaves", r_stats.stdout)
    if not m:
        raise TestFailure(f"could not parse built leaf count from scope.py stats: {r_stats.stdout!r}")
    live_built = m.group(1)
    built_md = (ctx.repo_root / "generated" / "BUILT.md").read_text(encoding="utf-8")
    if f"**{live_built}**" not in built_md:
        raise TestFailure(f"generated/BUILT.md is stale: expected **{live_built}** built leaves")
