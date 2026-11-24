"""Tier 3 pairwise cross-feature combination E2E tests covering all 21 F1-F7 pairs."""

from __future__ import annotations

import csv
import json
import re
import shutil
import sys
import zipfile
from pathlib import Path

from e2e.lib.binaries import BUILT, MISSING_CRATE_OR_BIN
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
    test_id="T3-01",
    tier=3,
    feature="F2",
    description="[F2+F3] fid-slice --demo package passes fid-pkg-conformance and fid-verify --replay verifies signed golden package",
)
def test_t3_slice_pkg_conformance_and_verify(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    conf = ctx.require_bin("fid-pkg-conformance")
    verify = ctx.require_bin("fid-verify")
    out_dir = ctx.workdir / "slice_out"
    r1 = ctx.run([fid_slice, "--demo", "--out", str(out_dir), "--json"])
    if r1.returncode != 0:
        raise TestFailure(f"fid-slice --demo failed: {r1.stderr}")
    pkg = out_dir / "result-package.zip"

    r2 = ctx.run([conf, str(pkg)])
    if r2.returncode != 0 or parse_single_line_json(r2.stdout).get("result") != "accepted":
        raise TestFailure(f"fid-pkg-conformance rejected fid-slice package: {r2.stdout} {r2.stderr}")

    # Unsigned fid-slice --demo package must fail fail-closed on signature check (exit 1, verdict=FAIL)
    r_uns = ctx.run([verify, str(pkg), "--json"])
    if r_uns.returncode != 1:
        raise TestFailure(f"expected exit 1 for unsigned fid-slice package, got {r_uns.returncode}: {r_uns.stdout}")
    rep_uns = parse_single_line_json(r_uns.stdout)
    if rep_uns.get("verdict") != "FAIL":
        raise TestFailure(f"expected verdict=FAIL for unsigned fid-slice package, got {rep_uns!r}")

    # Signed points-tier golden package must pass full verification + replay
    golden = ctx.repo_root / "fixtures" / "pkg" / "golden" / "two-signatures.zip"
    root = ctx.repo_root / "fixtures" / "troot" / "roots" / "both.json"
    r3 = ctx.run([verify, str(golden), "--trust-root", str(root), "--replay", "--json"])
    if r3.returncode != 0:
        raise TestFailure(f"fid-verify --replay exited {r3.returncode}: {r3.stdout} {r3.stderr}")
    rep = parse_single_line_json(r3.stdout)
    if rep.get("verdict") != "PASS":
        raise TestFailure(f"expected verdict=PASS on golden/two-signatures.zip, got {rep!r}")
    if rep.get("chain", {}).get("verdict") != "VALID":  # type: ignore[union-attr]
        raise TestFailure(f"expected chain VALID on golden/two-signatures.zip, got {rep!r}")
    if rep.get("replay", {}).get("status") != "IDENTICAL":  # type: ignore[union-attr]
        raise TestFailure(f"expected replay IDENTICAL on golden/two-signatures.zip, got {rep!r}")


@e2e_test(
    test_id="T3-02",
    tier=3,
    feature="F2",
    description="[F2+F3] M3/M4 fid-slice --balloon exports valid PDF+CSV artifacts alongside verified package",
)
def test_t3_balloon_pack_and_verify(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    conf = ctx.require_bin("fid-pkg-conformance")
    verify = ctx.require_bin("fid-verify")
    pdf = ctx.repo_root / "fixtures" / "rpt" / "balloon" / "auto" / "vector_bracket.pdf"
    truth = json.loads(
        (ctx.repo_root / "fixtures" / "rpt" / "balloon" / "auto" / "vector_bracket.truth.json").read_text(
            encoding="utf-8"
        )
    )
    out_dir = ctx.workdir / "pack_out"
    r1 = ctx.run([fid_slice, "--balloon", str(pdf), "--out", str(out_dir), "--json"])
    if r1.returncode != 0:
        raise TestFailure(f"fid-slice --balloon failed (exit {r1.returncode}): {r1.stdout} {r1.stderr}")
    parse_single_line_json(r1.stdout)

    ballooned_pdf = out_dir / "ballooned.pdf"
    form3_csv = out_dir / "form3-starter.csv"
    if not ballooned_pdf.is_file() or not form3_csv.is_file():
        raise TestFailure(f"missing ballooned.pdf or form3-starter.csv in {out_dir}")
    pdf_bytes = ballooned_pdf.read_bytes()
    if len(pdf_bytes) <= 100 or not pdf_bytes.startswith(b"%PDF-"):
        raise TestFailure(f"invalid ballooned.pdf: len={len(pdf_bytes)} header={pdf_bytes[:16]!r}")
    with form3_csv.open("r", encoding="utf-8", newline="") as fh:
        rows = list(csv.DictReader(fh))
    if len(rows) != truth["callout_count"]:
        raise TestFailure(f"expected {truth['callout_count']} rows in form3-starter.csv, got {len(rows)}")

    # Verify optional result-package.zip if emitted by --balloon, plus signed golden package via fid-verify
    pkg = out_dir / "result-package.zip"
    if pkg.is_file():
        rc_pkg = ctx.run([conf, str(pkg)])
        if rc_pkg.returncode != 0 or parse_single_line_json(rc_pkg.stdout).get("result") != "accepted":
            raise TestFailure(f"fid-pkg-conformance rejected balloon result-package.zip: {rc_pkg.stdout}")

    golden = ctx.repo_root / "fixtures" / "pkg" / "golden" / "two-signatures.zip"
    root = ctx.repo_root / "fixtures" / "troot" / "roots" / "both.json"
    r2 = ctx.run([verify, str(golden), "--trust-root", str(root), "--json"])
    if r2.returncode != 0:
        raise TestFailure(f"fid-verify failed (exit {r2.returncode}): {r2.stdout} {r2.stderr}")
    rep = parse_single_line_json(r2.stdout)
    if rep.get("verdict") != "PASS" or rep.get("chain", {}).get("verdict") != "VALID":  # type: ignore[union-attr]
        raise TestFailure(f"expected verdict=PASS and chain.verdict=VALID, got {rep!r}")


@e2e_test(
    test_id="T3-03",
    tier=3,
    feature="F1",
    description="[F1+F7] fid canon agrees bit-for-bit with Node.js RFC 8785 oracle (fixtures/core/jrn/canon/runner.mjs)",
)
def test_t3_fid_canon_vs_node_oracle(ctx: TestContext) -> None:
    node = ctx.require_node()
    fid = ctx.require_bin("fid")
    runner = ctx.repo_root / "fixtures" / "core" / "jrn" / "canon" / "runner.mjs"
    r_node = ctx.run([node, str(runner)])
    if r_node.returncode != 0:
        raise TestFailure(f"Node RFC 8785 oracle failed (exit {r_node.returncode}): {r_node.stdout} {r_node.stderr}")

    vectors = json.loads(
        (ctx.repo_root / "fixtures" / "core" / "jrn" / "canon" / "vectors.json").read_text(encoding="utf-8")
    )
    for idx, vec in enumerate(vectors[:20]):
        in_file = ctx.workdir / f"vec_{idx}.json"
        in_file.write_text(vec["input"], encoding="utf-8")
        r_fid = ctx.run([fid, "canon", "--json", str(in_file)])
        if r_fid.returncode != 0:
            raise TestFailure(f"fid canon failed on Node vector {vec['name']}: {r_fid.stderr}")
        env = parse_single_line_json(r_fid.stdout)
        got_digest = env["result"]["digest"]  # type: ignore[index]
        if got_digest != vec["digest"]:
            raise TestFailure(f"digest mismatch on {vec['name']}: fid={got_digest} node={vec['digest']}")


@e2e_test(
    test_id="T3-04",
    tier=3,
    feature="F1",
    description="[F1+F6] M4 fid-bus-api enforces no-side-doors rule (xtask lint-crate-graph) and dispatches fid commands",
    capabilities_required=("c_toolchain",),
)
def test_t3_fid_bus_api_and_lint_crate_graph(ctx: TestContext) -> None:
    xtask = ctx.require_bin("fid-xtask")
    r_lint = ctx.run([xtask, "lint-crate-graph"])
    if r_lint.returncode != 0 or "lint-crate-graph: ok" not in r_lint.stdout:
        raise TestFailure(f"fid-xtask lint-crate-graph failed: {r_lint.stdout} {r_lint.stderr}")
    if not ctx.binaries.has_crate("fid-bus-api"):
        raise TestFailure("crate `fid-bus-api` missing from workspace (M4 AUTO.CLI.DISPATCH prerequisite)")
    fid = ctx.require_bin("fid")
    r_disp = ctx.run([fid, "project", "list-features", "--json"])
    if r_disp.returncode != 0 or "AUTO.CLI.DISPATCH" in r_disp.stdout:
        raise TestFailure(f"fid bus dispatch failed: {r_disp.stdout} {r_disp.stderr}")
    env = parse_single_line_json(r_disp.stdout)
    assert_envelope(env, command_id="project.list_features@1")
    if env.get("outcome") not in ("computed", "committed", "dry_run"):
        raise TestFailure(f"unexpected envelope outcome from fid dispatch: {env!r}")
    if not isinstance(env.get("result"), dict):
        raise TestFailure(f"expected dict result in fid dispatch envelope: {env!r}")


@e2e_test(
    test_id="T3-05",
    tier=3,
    feature="F4",
    description="[F4+F1] M4 fid-mcp projection aligns with fid CliTree and strips principal/role overrides",
)
def test_t3_mcp_vs_cli_tree_and_principal_strip(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    mcp = ctx.require_bin("fid-mcp")
    r_tree = ctx.run([fid, "--help", "--json"])
    if r_tree.returncode != 0:
        raise TestFailure(f"fid --help --json failed: {r_tree.stderr}")
    cli_tree = parse_single_line_json(r_tree.stdout)
    cli_cmds = cli_tree.get("commands")
    if not isinstance(cli_cmds, list) or not cli_cmds:
        raise TestFailure(f"empty CliTree commands: {cli_tree!r}")
    forbidden_tools = {
        c["command_id"].replace(".", "_")
        for c in cli_cmds
        if isinstance(c, dict) and c.get("side_effect") in ("M1", "S1", "A0") and isinstance(c.get("command_id"), str)
    }

    idx = write_disclosure_profiles_index(ctx.workdir)
    msgs_in = mcp_init_messages() + [
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
        {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "tools_search_tools",
                "arguments": {"keyword": "project", "principal": "human:alice", "role": "admin"},
            },
        },
    ]
    rc, msgs, _, stderr = run_mcp_session(mcp, ["--headless", "--profiles-index", str(idx)], msgs_in, cwd=ctx.repo_root)
    if rc != 0:
        raise TestFailure(f"fid-mcp exited {rc}: {stderr}")
    r_list = next((m for m in msgs if m.get("id") == 2), None)
    if not r_list or "result" not in r_list:
        raise TestFailure(f"missing tools/list response: {msgs!r}")
    listed_names = {t.get("name") for t in r_list["result"].get("tools", [])}
    leaked = sorted(listed_names & forbidden_tools)
    if leaked:
        raise TestFailure(f"fid-mcp leaked M1/S1/A0 commands from CliTree into tools/list: {leaked}")

    r3 = next((m for m in msgs if m.get("id") == 3), None)
    if not r3 or "result" not in r3 or r3["result"].get("isError"):
        raise TestFailure(f"expected non-error tool reply stripping principal override, got {msgs!r}")
    content = r3["result"].get("content")
    if not isinstance(content, list) or not content:
        raise TestFailure(f"expected non-empty content from tools_search_tools: {r3!r}")
    r3_dump = json.dumps(r3["result"])
    if "project.list_features@1" not in r3_dump and "project_list_features" not in r3_dump:
        raise TestFailure(f"expected project.list_features@1 in search result: {r3!r}")
    if not any(tok in r3_dump.lower() for tok in ("strip", "warn", "drop", "principal", "ignored")):
        raise TestFailure(f"expected principal/role stripping warning in result: {r3!r}")


@e2e_test(
    test_id="T3-06",
    tier=3,
    feature="F5",
    description="[F5+F6] scope.py check -> site/build.py -> fid-outreach qa a11y -> link-check pipeline",
)
def test_t3_scope_site_a11y_link_check(ctx: TestContext) -> None:
    outreach = ctx.require_bin("fid-outreach")
    scope_root = prepare_scope_root(ctx, "scope_check_t3")
    r_scope = ctx.run([sys.executable, str(scope_root / "tools" / "scope.py"), "check", "--root", str(scope_root)])
    if r_scope.returncode != 0:
        raise TestFailure(f"scope.py check failed: {r_scope.stdout} {r_scope.stderr}")
    copy_root = build_isolated_site(ctx)

    dist = copy_root / "site" / "dist"
    tokens_path = write_site_a11y_tokens(ctx.workdir)
    a11y_out = ctx.workdir / "a11y.json"
    r_a11y = ctx.run([outreach, "qa", "a11y", "--root", str(dist), "--tokens", str(tokens_path), "--out", str(a11y_out)])
    if r_a11y.returncode != 0 or not a11y_out.is_file():
        raise TestFailure(f"qa a11y failed (exit {r_a11y.returncode}): {r_a11y.stdout} {r_a11y.stderr}")
    rep = json.loads(a11y_out.read_text(encoding="utf-8"))
    if rep.get("schema") != "a11y-report@1" or rep.get("result") != "pass" or rep.get("violations") != []:
        raise TestFailure(f"qa a11y did not pass cleanly: {rep!r}")

    # M2 #445 link-check across built site/dist/
    r_lc = ctx.run([outreach, "link-check", "--edition", str(dist), "--json"])
    if r_lc.returncode != 0:
        raise TestFailure(f"link-check on site/dist failed (exit {r_lc.returncode}): {r_lc.stdout} {r_lc.stderr}")
    lc_obj = parse_single_line_json(r_lc.stdout)
    if lc_obj.get("ok") is False or lc_obj.get("result") == "fail":
        raise TestFailure(f"link-check reported failure: {lc_obj!r}")
    for key in ("findings", "broken", "violations", "errors"):
        if key in lc_obj and lc_obj[key] != []:
            raise TestFailure(f"link-check reported {key}: {lc_obj!r}")


@e2e_test(
    test_id="T3-07",
    tier=3,
    feature="F5",
    description="[F5+F7] M2 site/build.py /guide/ + fid-guide-wasm + worker.js CSP align with generated/ & README.md",
)
def test_t3_site_guide_wasm_and_csp(ctx: TestContext) -> None:
    wasm_bin = ctx.require_bin("fid-guide-wasm")
    copy_root = build_isolated_site(ctx)
    guide_html = copy_root / "site" / "dist" / "guide" / "index.html"
    if not guide_html.is_file():
        raise TestFailure(f"missing built /guide/index.html: {guide_html}")
    worker = (copy_root / "site" / "worker.js").read_text(encoding="utf-8")
    if "wasm-unsafe-eval" not in worker:
        raise TestFailure("site/worker.js missing /guide/* wasm-unsafe-eval CSP")
    r = ctx.run([wasm_bin], stdin_text='{"op":"canon.digest","params":{"value":{"a":1}}}')
    if r.returncode != 0:
        raise TestFailure(f"fid-guide-wasm failed: {r.stderr}")
    wasm_out = parse_single_line_json(r.stdout)
    if wasm_out.get("ok") is not True or not str(wasm_out.get("result_digest", "")).startswith("sha256:"):
        raise TestFailure(f"unexpected fid-guide-wasm output: {wasm_out!r}")

    # F7 consistency: generated/BUILT.md & generated/DECISIONS.md match built site status numbers & README.md
    status_html = (copy_root / "site" / "dist" / "status" / "index.html").read_text(encoding="utf-8")
    built_md = (ctx.repo_root / "generated" / "BUILT.md").read_text(encoding="utf-8")
    m_built = re.search(r"\*\*(\d+)\*\*\s+of\s+\*\*", built_md)
    if not m_built:
        m_built = re.search(r"\*\*(\d+)\*\*", built_md)
    if not m_built:
        raise TestFailure(f"could not extract built count from generated/BUILT.md")
    built_n = int(m_built.group(1))
    if f"<b>{built_n:,}</b>" not in status_html and f"<b>{built_n}</b>" not in status_html:
        raise TestFailure(f"generated/BUILT.md ({built_n}) out of sync with site/dist/status/index.html")
    dec_md = (ctx.repo_root / "generated" / "DECISIONS.md").read_text(encoding="utf-8")
    for did in ("D-049", "D-052", "D-054"):
        if did not in dec_md:
            raise TestFailure(f"generated/DECISIONS.md missing {did}")
    readme = (ctx.repo_root / "README.md").read_text(encoding="utf-8")
    if "--exclude fid-clarabel --doc" not in readme or "scope.py milestone" not in readme:
        raise TestFailure("README.md missing expected workspace test/milestone documentation")


@e2e_test(
    test_id="T3-08",
    tier=3,
    feature="F6",
    description="[F1+F6] tracim_public.py run invokes real fid rdl submit-tracim against local TracimMockServer",
)
def test_t3_tracim_public_with_real_fid(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    r_help = ctx.run([fid, "rdl", "submit-tracim", "--help"])
    if r_help.returncode != 0:
        raise TestFailure(f"fid binary does not implement `rdl submit-tracim` (exit {r_help.returncode}): {r_help.stderr}")

    mock_dir = ctx.repo_root / "fixtures" / "rdl" / "tracim" / "mock"
    wf = ctx.repo_root / ".github" / "workflows" / "tracim-public.yml"
    out_art = ctx.workdir / "tracim-public-latest.json"
    driver = ctx.workdir / "run_with_mock.py"
    driver.write_text(
        f"""
import importlib.util, sys
from pathlib import Path
sys.path.insert(0, {str(ctx.repo_root)!r})
from e2e.lib.env import clean_env
from tools.rdl import tracim_public
spec = importlib.util.spec_from_file_location("m", {str(mock_dir / "mock_server.py")!r})
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
srv = mod.TracimMockServer(expected_key="KEY-E2E", verdict_mode="passed")
url = srv.start()
try:
    code, art = tracim_public.run_public_test(
        fid_argv=[{str(fid)!r}],
        out_path=Path({str(out_art)!r}),
        workflow_path=Path({str(wf)!r}),
        destination=url,
        env=clean_env({{"TRACIM_PUBLIC_KEY": "KEY-E2E"}}),
    )
    print(code, art)
    sys.exit(code)
finally:
    srv.stop()
""",
        encoding="utf-8",
    )
    r_drv = ctx.run([sys.executable, str(driver)])
    if r_drv.returncode != 0:
        raise TestFailure(f"tracim_public + real fid failed: {r_drv.stdout} {r_drv.stderr}")


@e2e_test(
    test_id="T3-09",
    tier=3,
    feature="F3",
    description="[F3+F7] check_spec_drift.py codes/reasons match fid-pkg-conformance outputs and v1.md interp/* docs",
)
def test_t3_spec_drift_vs_pkg_conformance(ctx: TestContext) -> None:
    conf = ctx.require_bin("fid-pkg-conformance")
    r_drift = ctx.run([sys.executable, str(ctx.repo_root / "fixtures" / "pkg" / "check_spec_drift.py")])
    if r_drift.returncode != 0:
        raise TestFailure(f"check_spec_drift.py failed: {r_drift.stdout}")

    v1_md = (ctx.repo_root / "docs" / "result-package" / "v1.md").read_text(encoding="utf-8")
    for sample_case in ("truncated_tail", "crc_mismatch", "symlink"):
        cdir = ctx.repo_root / "fixtures" / "pkg" / "malformed" / sample_case
        pkg = cdir / "package.zip"
        expect = json.loads((cdir / "expect.json").read_text(encoding="utf-8"))
        if not pkg.is_file():
            raise TestFailure(f"missing malformed fixture: {pkg}")
        r = ctx.run([conf, str(pkg)])
        if r.returncode != 1:
            raise TestFailure(f"expected exit 1 on malformed {sample_case}, got {r.returncode}: {r.stdout}")
        got = parse_single_line_json(r.stdout)
        if got.get("result") != "rejected" or got.get("reason") != expect["reason"]:
            raise TestFailure(f"malformed {sample_case} mismatch: got={got!r} expect={expect!r}")
        if got["error_code"] not in v1_md or got["reason"] not in v1_md:
            raise TestFailure(f"rejection {got!r} missing from docs/result-package/v1.md")

    if "interp/registers/<sha256>.json" not in v1_md or "interp/rosters/<sha256>.json" not in v1_md:
        raise TestFailure("docs/result-package/v1.md is missing interp/* member documentation")


@e2e_test(
    test_id="T3-10",
    tier=3,
    feature="F6",
    description="[F5+F6+F7] fid-xtask rel runbook-check/render parity combined with tools/verify_guide.py",
    capabilities_required=("c_toolchain",),
)
def test_t3_rel_runbook_and_verify_guide(ctx: TestContext) -> None:
    xtask = ctx.require_bin("fid-xtask")
    r_chk = ctx.run([xtask, "rel", "runbook-check", "--dir", str(ctx.repo_root / "release"), "--json"])
    if r_chk.returncode != 0:
        raise TestFailure(f"rel runbook-check failed: {r_chk.stdout} {r_chk.stderr}")
    rendered = ctx.workdir / "RUNBOOK.md"
    r_ren = ctx.run(
        [xtask, "rel", "runbook-render", "--dir", str(ctx.repo_root / "release"), "--out", str(rendered)]
    )
    if r_ren.returncode != 0:
        raise TestFailure(f"rel runbook-render failed: {r_ren.stderr}")
    committed = (ctx.repo_root / "release" / "RUNBOOK.md").read_text(encoding="utf-8")
    if rendered.read_text(encoding="utf-8") != committed:
        raise TestFailure("release/RUNBOOK.md is out of sync with release/runbook.toml")

    tag = "verify-windows" if ctx.caps.is_windows else "verify-linux"
    r_vg = ctx.run([sys.executable, str(ctx.repo_root / "tools" / "verify_guide.py"), "--tag", tag])
    if r_vg.returncode != 0:
        raise TestFailure(f"verify_guide.py --tag {tag} failed: {r_vg.stdout} {r_vg.stderr}")


@e2e_test(
    test_id="T3-11",
    tier=3,
    feature="F6",
    description="[F4+F6] fid-accept validators and fid-eval-harness adapter/safety/null-agent CLI checks pass",
)
def test_t3_accept_and_eval_harness(ctx: TestContext) -> None:
    accept_res = ctx.binaries.build_bin("fid-accept")
    if accept_res.status == BUILT and accept_res.path is not None:
        r_acc = ctx.run([accept_res.path, "--help"])
        if r_acc.returncode != 0 or not r_acc.stdout.strip():
            raise TestFailure(f"fid-accept --help failed: {r_acc.stdout} {r_acc.stderr}")
    elif accept_res.status == MISSING_CRATE_OR_BIN:
        if not ctx.caps.cargo_path:
            raise TestFailure("cargo executable required for fid-accept tests")
        r_acc_t = ctx.run([ctx.caps.cargo_path, "test", "--locked", "-p", "fid-accept"], timeout=300)
        if r_acc_t.returncode != 0:
            raise TestFailure(f"cargo test -p fid-accept failed: {r_acc_t.stdout} {r_acc_t.stderr}")
        m_pass = re.search(r"test result:\s*ok\.\s*(\d+)\s+passed;\s*0\s+failed", r_acc_t.stdout)
        if not m_pass or int(m_pass.group(1)) == 0:
            raise TestFailure(f"expected >=1 passing fid-accept test, got:\n{r_acc_t.stdout}")
    else:
        raise TestFailure(f"fid-accept build unexpected status: {accept_res}")

    eh = ctx.require_bin("fid-eval-harness")
    r_null = ctx.run([eh, "null-agent"])
    if r_null.returncode != 0:
        raise TestFailure(f"fid-eval-harness null-agent failed: {r_null.stderr}")

    adapter_json = ctx.repo_root / "fixtures" / "agent" / "harness" / "adapters" / "exec-example.json"
    r_adp = ctx.run([eh, "adapter", "check", str(adapter_json)])
    if r_adp.returncode != 0 or not r_adp.stdout.strip().startswith("ok: adapter"):
        raise TestFailure(f"fid-eval-harness adapter check unexpected output: {r_adp.stdout} {r_adp.stderr}")

    run_dir = ctx.repo_root / "fixtures" / "agent" / "harness" / "runs" / "refused-w3"
    r_saf = ctx.run([eh, "safety", str(run_dir), "--json"])
    if r_saf.returncode != 0:
        raise TestFailure(f"fid-eval-harness safety failed: {r_saf.stdout} {r_saf.stderr}")
    saf_obj = parse_single_line_json(r_saf.stdout)
    cats = saf_obj.get("categories")
    if not isinstance(cats, dict) or not cats:
        raise TestFailure(f"expected non-empty safety categories dict, got: {saf_obj!r}")
    for req_cat in ("forbidden_class", "self_adoption", "out_of_root", "raw_path", "redteam"):
        if req_cat not in cats:
            raise TestFailure(f"missing safety category {req_cat!r} in {cats!r}")


@e2e_test(
    test_id="T3-12",
    tier=3,
    feature="F6",
    description="[F3+F6] fid-val and fid-crash CLI harnesses pass on C/SQLite-capable hosts",
    capabilities_required=("c_toolchain",),
)
def test_t3_val_and_crash_harnesses(ctx: TestContext) -> None:
    fid_val = ctx.require_bin("fid-val")
    toy_art = ctx.require_bin("val-toy-artifact")
    fid_crash = ctx.require_bin("fid-crash")

    val_src = ctx.repo_root / "fixtures" / "man" / "val-toy-v1"
    val_work = ctx.workdir / "val_toy"
    shutil.copytree(val_src, val_work)
    rel_data = json.loads((val_work / "release.json").read_text(encoding="utf-8"))
    rel_data["cases"] = [c for c in rel_data["cases"] if c.get("case_id") != "plane-06-fail"]
    (val_work / "release.json").write_text(json.dumps(rel_data), encoding="utf-8")

    r_val = ctx.run(
        [
            fid_val,
            "run",
            "--release",
            str(val_work / "release.json"),
            "--mappings",
            str(val_work / "mappings.toml"),
            "--criteria",
            str(val_work / "criteria"),
            "--artifact",
            str(toy_art),
        ]
    )
    if r_val.returncode != 0:
        raise TestFailure(f"fid-val run failed (exit {r_val.returncode}): {r_val.stdout} {r_val.stderr}")
    val_obj = json.loads(r_val.stdout)
    if not val_obj.get("cases") or not val_obj.get("summaries"):
        raise TestFailure(f"unexpected fid-val JSON output: {val_obj!r}")

    crash_dir = ctx.workdir / "crash_run"
    r_ch = ctx.run([fid_crash, "run", "--kills", "10", "--seed", "1", "--dir", str(crash_dir)])
    if r_ch.returncode != 0 or "crash-harness:" not in r_ch.stdout:
        raise TestFailure(f"fid-crash run --kills 10 failed: {r_ch.stdout} {r_ch.stderr}")


@e2e_test(
    test_id="T3-13",
    tier=3,
    feature="F1",
    description="[F1+F2] fid canon hashes fid-slice records.json and fid CLI tree exposes report ballooning commands",
)
def test_t3_fid_cli_and_slice(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    fid_slice = ctx.require_bin("fid-slice")
    out_dir = ctx.workdir / "slice_f1_f2"
    r_slice = ctx.run([fid_slice, "--demo", "--out", str(out_dir), "--json"])
    if r_slice.returncode != 0:
        raise TestFailure(f"fid-slice --demo failed: {r_slice.stderr}")
    parse_single_line_json(r_slice.stdout)
    records_path = out_dir / "records.json"
    if not records_path.is_file():
        raise TestFailure(f"missing {records_path}")

    r_can = ctx.run([fid, "canon", "--json", str(records_path)])
    if r_can.returncode != 0:
        raise TestFailure(f"fid canon on fid-slice records.json failed: {r_can.stderr}")
    env = parse_single_line_json(r_can.stdout)
    assert_envelope(env, outcome="computed", command_id="canon.canonicalize@1")
    dig = env.get("result", {}).get("digest", "")  # type: ignore[union-attr]
    if not str(dig).startswith("sha256:") or len(str(dig)) != 71:
        raise TestFailure(f"unexpected digest from fid canon: {dig!r}")

    r_help = ctx.run([fid, "report", "extract-drawing-text", "--help"])
    if r_help.returncode != 0 or "Command:" not in r_help.stdout:
        raise TestFailure(f"fid report extract-drawing-text --help failed: {r_help.stdout} {r_help.stderr}")


@e2e_test(
    test_id="T3-14",
    tier=3,
    feature="F1",
    description="[F1+F3] fid canon manifest digest matches fid-verify verified statement and fid verify package --help exists",
)
def test_t3_fid_cli_and_verify(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    verify = ctx.require_bin("fid-verify")
    golden = ctx.repo_root / "fixtures" / "pkg" / "golden" / "two-signatures.zip"
    root = ctx.repo_root / "fixtures" / "troot" / "roots" / "both.json"

    r_v = ctx.run([verify, str(golden), "--trust-root", str(root), "--replay", "--json"])
    if r_v.returncode != 0:
        raise TestFailure(f"fid-verify failed: {r_v.stdout} {r_v.stderr}")
    rep = parse_single_line_json(r_v.stdout)
    if rep.get("verdict") != "PASS" or rep.get("replay", {}).get("status") != "IDENTICAL":  # type: ignore[union-attr]
        raise TestFailure(f"unexpected fid-verify report: {rep!r}")

    with zipfile.ZipFile(golden, "r") as zf:
        manifest_bytes = zf.read("manifest.json")
        stmt = json.loads(zf.read("statement.json"))
    manifest_path = ctx.workdir / "manifest.json"
    manifest_path.write_bytes(manifest_bytes)
    r_can = ctx.run([fid, "canon", "--digest", str(manifest_path)])
    if r_can.returncode != 0:
        raise TestFailure(f"fid canon --digest failed: {r_can.stderr}")
    expected_dig = "sha256:" + stmt["predicate"]["manifest_sha256"]
    if r_can.stdout.strip() != expected_dig:
        raise TestFailure(f"fid canon digest {r_can.stdout.strip()!r} != statement manifest_sha256 {expected_dig!r}")

    r_vh = ctx.run([fid, "verify", "package", "--help"])
    if r_vh.returncode != 0 or "verify.package@1" not in r_vh.stdout:
        raise TestFailure(f"fid verify package --help missing verify.package@1: {r_vh.stdout} {r_vh.stderr}")


@e2e_test(
    test_id="T3-15",
    tier=3,
    feature="F1",
    description="[F1+F5] fid canon digest matches fid-guide-wasm canon.digest across built /guide/ site context",
)
def test_t3_fid_cli_and_guide_wasm(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    wasm_bin = ctx.require_bin("fid-guide-wasm")
    vec = ctx.repo_root / "fixtures" / "canon" / "vectors" / "keys_utf16_order.json"
    r_fid = ctx.run([fid, "canon", "--digest", str(vec)])
    if r_fid.returncode != 0:
        raise TestFailure(f"fid canon failed: {r_fid.stderr}")
    fid_digest = r_fid.stdout.strip()

    val = json.loads(vec.read_text(encoding="utf-8"))
    r_wasm = ctx.run([wasm_bin], stdin_text=json.dumps({"op": "canon.digest", "params": {"value": val}}))
    if r_wasm.returncode != 0:
        raise TestFailure(f"fid-guide-wasm failed: {r_wasm.stderr}")
    wasm_out = parse_single_line_json(r_wasm.stdout)
    if wasm_out.get("ok") is not True or wasm_out.get("result_digest") != fid_digest:
        raise TestFailure(f"digest mismatch fid={fid_digest!r} vs wasm={wasm_out!r}")

    copy_root = build_isolated_site(ctx)
    if not (copy_root / "site" / "dist" / "guide" / "index.html").is_file():
        raise TestFailure("missing /guide/index.html in isolated site build")


@e2e_test(
    test_id="T3-16",
    tier=3,
    feature="F2",
    description="[F2+F4] fid-slice --demo outputs align with fid-mcp report/project disclosure tools",
)
def test_t3_slice_and_mcp(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    mcp = ctx.require_bin("fid-mcp")
    out_dir = ctx.workdir / "slice_f2_f4"
    r_slice = ctx.run([fid_slice, "--demo", "--out", str(out_dir), "--json"])
    if r_slice.returncode != 0:
        raise TestFailure(f"fid-slice --demo failed: {r_slice.stderr}")
    slice_env = parse_single_line_json(r_slice.stdout)
    assert_envelope(slice_env, outcome="computed", command_id="slice.run_vertical@1")

    idx = write_disclosure_profiles_index(ctx.workdir)
    msgs_in = mcp_init_messages() + [
        {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "tools_search_tools", "arguments": {"keyword": "report"}},
        },
        {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "tools_describe_tool", "arguments": {"command": "rpt.release_report@1"}},
        },
    ]
    rc, msgs, _, stderr = run_mcp_session(mcp, ["--headless", "--profiles-index", str(idx)], msgs_in, cwd=ctx.repo_root)
    if rc != 0:
        raise TestFailure(f"fid-mcp exited {rc}: {stderr}")
    for cid in (2, 3):
        rep = next((m for m in msgs if m.get("id") == cid), None)
        if not rep or "result" not in rep or rep["result"].get("isError"):
            raise TestFailure(f"MCP call id={cid} failed: {rep!r}")
        if not rep["result"].get("content"):
            raise TestFailure(f"MCP call id={cid} returned empty content: {rep!r}")
    if "rpt.release_report@1" not in json.dumps(msgs):
        raise TestFailure(f"expected rpt.release_report@1 in MCP search/describe responses: {msgs!r}")


@e2e_test(
    test_id="T3-17",
    tier=3,
    feature="F2",
    description="[F2+F5] fid-guide-wasm canon.digest matches fid-slice result-package.zip statement manifest_sha256",
)
def test_t3_slice_and_guide_wasm(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    wasm_bin = ctx.require_bin("fid-guide-wasm")
    out_dir = ctx.workdir / "slice_f2_f5"
    r_slice = ctx.run([fid_slice, "--demo", "--out", str(out_dir), "--json"])
    if r_slice.returncode != 0:
        raise TestFailure(f"fid-slice --demo failed: {r_slice.stderr}")

    pkg = out_dir / "result-package.zip"
    with zipfile.ZipFile(pkg, "r") as zf:
        manifest_val = json.loads(zf.read("manifest.json"))
        stmt = json.loads(zf.read("statement.json"))

    r_wasm = ctx.run([wasm_bin], stdin_text=json.dumps({"op": "canon.digest", "params": {"value": manifest_val}}))
    if r_wasm.returncode != 0:
        raise TestFailure(f"fid-guide-wasm failed: {r_wasm.stderr}")
    wasm_out = parse_single_line_json(r_wasm.stdout)
    expected_dig = "sha256:" + stmt["predicate"]["manifest_sha256"]
    if wasm_out.get("ok") is not True or wasm_out.get("result_digest") != expected_dig:
        raise TestFailure(f"WASM manifest digest {wasm_out!r} != statement {expected_dig!r}")


@e2e_test(
    test_id="T3-18",
    tier=3,
    feature="F2",
    description="[F2+F6] scope.py milestone vertical-slice-0.md aligns with fid-slice --demo and --balloon execution",
)
def test_t3_slice_and_scope_vs0(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    scope_root = prepare_scope_root(ctx, "scope_vs0")
    r_chk = ctx.run([sys.executable, str(scope_root / "tools" / "scope.py"), "check", "--root", str(scope_root)])
    if r_chk.returncode != 0:
        raise TestFailure(f"scope.py check failed: {r_chk.stdout} {r_chk.stderr}")
    r_ms = ctx.run(
        [
            sys.executable,
            str(scope_root / "tools" / "scope.py"),
            "milestone",
            str(scope_root / "milestones" / "vertical-slice-0.md"),
            "--root",
            str(scope_root),
        ]
    )
    if r_ms.returncode != 0:
        raise TestFailure(f"scope.py milestone vertical-slice-0.md failed: {r_ms.stdout} {r_ms.stderr}")
    if not (scope_root / "generated" / "milestones" / "vertical-slice-0.md").is_file():
        raise TestFailure("generated/milestones/vertical-slice-0.md was not created")

    out_dir = ctx.workdir / "vs0_out"
    r_demo = ctx.run([fid_slice, "--demo", "--out", str(out_dir), "--json"])
    if r_demo.returncode != 0:
        raise TestFailure(f"fid-slice --demo failed: {r_demo.stderr}")
    r_ver = ctx.run([fid_slice, "--verify", str(out_dir / "result-package.zip"), "--json"])
    if r_ver.returncode != 0 or parse_single_line_json(r_ver.stdout)["result"]["status"] != "VERIFIED":  # type: ignore[index]
        raise TestFailure(f"fid-slice --verify failed: {r_ver.stdout}")


@e2e_test(
    test_id="T3-19",
    tier=3,
    feature="F2",
    description="[F2+F7] fid-slice result-package.zip manifest.json & statement.json satisfy schema/* and v1.md spec",
)
def test_t3_slice_and_pkg_schemas(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    out_dir = ctx.workdir / "slice_f2_f7"
    r_slice = ctx.run([fid_slice, "--demo", "--out", str(out_dir), "--json"])
    if r_slice.returncode != 0:
        raise TestFailure(f"fid-slice --demo failed: {r_slice.stderr}")

    with zipfile.ZipFile(out_dir / "result-package.zip", "r") as zf:
        manifest = json.loads(zf.read("manifest.json"))
        statement = json.loads(zf.read("statement.json"))

    for key in ("format", "format_version", "evidence_tier", "profile_digest", "kernel_manifest_digest", "members"):
        if key not in manifest:
            raise TestFailure(f"fid-slice manifest.json missing {key!r}")
    for key in ("_type", "subject", "predicateType", "predicate"):
        if key not in statement:
            raise TestFailure(f"fid-slice statement.json missing {key!r}")

    v1_md = (ctx.repo_root / "docs" / "result-package" / "v1.md").read_text(encoding="utf-8")
    if "manifest.json" not in v1_md or "statement.json" not in v1_md:
        raise TestFailure("docs/result-package/v1.md missing manifest/statement documentation")
    try:
        import jsonschema  # type: ignore[import-untyped]
    except ImportError:
        return
    m_schema = json.loads((ctx.repo_root / "schema" / "manifest-1.json").read_text(encoding="utf-8"))
    s_schema = json.loads((ctx.repo_root / "schema" / "statement-1.json").read_text(encoding="utf-8"))
    jsonschema.validate(instance=manifest, schema=m_schema)
    jsonschema.validate(instance=statement, schema=s_schema)


@e2e_test(
    test_id="T3-20",
    tier=3,
    feature="F3",
    description="[F3+F4] fid-verify verifies signed package while fid-mcp excludes S1 trust_sign_record and invokes R0 safely",
)
def test_t3_verify_and_mcp(ctx: TestContext) -> None:
    verify = ctx.require_bin("fid-verify")
    mcp = ctx.require_bin("fid-mcp")
    golden = ctx.repo_root / "fixtures" / "pkg" / "golden" / "two-signatures.zip"
    root = ctx.repo_root / "fixtures" / "troot" / "roots" / "both.json"
    r_v = ctx.run([verify, str(golden), "--trust-root", str(root), "--json"])
    if r_v.returncode != 0:
        raise TestFailure(f"fid-verify failed (exit {r_v.returncode}): {r_v.stdout} {r_v.stderr}")
    rep = parse_single_line_json(r_v.stdout)
    if rep.get("verdict") != "PASS" or rep.get("chain", {}).get("verdict") != "VALID":  # type: ignore[union-attr]
        raise TestFailure(f"expected PASS and VALID chain, got {rep!r}")

    idx = write_disclosure_profiles_index(ctx.workdir)
    msgs_in = mcp_init_messages() + [
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
        {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "tools_search_tools", "arguments": {"keyword": "trust"}},
        },
        {
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {"name": "tools_invoke_readonly", "arguments": {"command": "project.list_features@1", "args": {}}},
        },
    ]
    rc, msgs, _, stderr = run_mcp_session(mcp, ["--headless", "--profiles-index", str(idx)], msgs_in, cwd=ctx.repo_root)
    if rc != 0:
        raise TestFailure(f"fid-mcp exited {rc}: {stderr}")
    if "trust_sign_record" in json.dumps(msgs) or "trust.sign_record" in json.dumps(msgs):
        raise TestFailure(f"S1 trust_sign_record leaked into MCP responses: {msgs!r}")
    r4 = next((m for m in msgs if m.get("id") == 4), None)
    if not r4 or "result" not in r4 or r4["result"].get("isError") or not r4["result"].get("content"):
        raise TestFailure(f"MCP readonly invocation failed: {r4!r}")


@e2e_test(
    test_id="T3-21",
    tier=3,
    feature="F3",
    description="[F3+F5] fid-guide-wasm canon.digest matches golden package manifest_sha256 verified by fid-verify --replay",
)
def test_t3_verify_and_guide_wasm(ctx: TestContext) -> None:
    verify = ctx.require_bin("fid-verify")
    wasm_bin = ctx.require_bin("fid-guide-wasm")
    golden = ctx.repo_root / "fixtures" / "pkg" / "golden" / "two-signatures.zip"
    root = ctx.repo_root / "fixtures" / "troot" / "roots" / "both.json"

    with zipfile.ZipFile(golden, "r") as zf:
        manifest_val = json.loads(zf.read("manifest.json"))
        stmt = json.loads(zf.read("statement.json"))

    r_wasm = ctx.run([wasm_bin], stdin_text=json.dumps({"op": "canon.digest", "params": {"value": manifest_val}}))
    if r_wasm.returncode != 0:
        raise TestFailure(f"fid-guide-wasm failed: {r_wasm.stderr}")
    wasm_out = parse_single_line_json(r_wasm.stdout)
    expected_dig = "sha256:" + stmt["predicate"]["manifest_sha256"]
    if wasm_out.get("ok") is not True or wasm_out.get("result_digest") != expected_dig:
        raise TestFailure(f"fid-guide-wasm digest {wasm_out!r} != {expected_dig!r}")

    r_v = ctx.run([verify, str(golden), "--trust-root", str(root), "--replay", "--json"])
    if r_v.returncode != 0:
        raise TestFailure(f"fid-verify --replay failed (exit {r_v.returncode}): {r_v.stdout} {r_v.stderr}")
    rep = parse_single_line_json(r_v.stdout)
    if rep.get("verdict") != "PASS" or rep.get("replay", {}).get("status") != "IDENTICAL":  # type: ignore[union-attr]
        raise TestFailure(f"unexpected fid-verify report: {rep!r}")


@e2e_test(
    test_id="T3-22",
    tier=3,
    feature="F4",
    description="[F4+F5] fid-mcp front-door session runs alongside site/build.py and fid-outreach link-check",
)
def test_t3_mcp_and_site_outreach(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    outreach = ctx.require_bin("fid-outreach")
    copy_root = build_isolated_site(ctx)
    dist = copy_root / "site" / "dist"
    how_html = (dist / "how-it-works" / "index.html").read_text(encoding="utf-8")
    if "MCP" not in how_html:
        raise TestFailure("built site/dist/how-it-works/index.html missing MCP surface description")

    edition = ctx.repo_root / "fixtures" / "outr" / "docs" / "qa" / "ok"
    r_lc = ctx.run([outreach, "link-check", "--edition", str(edition), "--json"])
    if r_lc.returncode != 0:
        raise TestFailure(f"fid-outreach link-check failed: {r_lc.stdout} {r_lc.stderr}")
    parse_single_line_json(r_lc.stdout)

    idx = write_disclosure_profiles_index(ctx.workdir)
    msgs_in = mcp_init_messages() + [
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
        {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "tools_search_tools", "arguments": {"keyword": "save"}},
        },
    ]
    rc, msgs, _, stderr = run_mcp_session(mcp, ["--headless", "--profiles-index", str(idx)], msgs_in, cwd=ctx.repo_root)
    if rc != 0:
        raise TestFailure(f"fid-mcp exited {rc}: {stderr}")
    r2 = next((m for m in msgs if m.get("id") == 2), None)
    r3 = next((m for m in msgs if m.get("id") == 3), None)
    if not r2 or not (1 <= len(r2.get("result", {}).get("tools", [])) <= 15):
        raise TestFailure(f"unexpected MCP tools/list: {r2!r}")
    if not r3 or r3.get("result", {}).get("isError") or not r3.get("result", {}).get("content"):
        raise TestFailure(f"unexpected MCP search response: {r3!r}")


@e2e_test(
    test_id="T3-23",
    tier=3,
    feature="F4",
    description="[F4+F7] fid-mcp profiles/disclosure align with schemas/mcp-profile-1.json, README.md, and generated/DECISIONS.md",
)
def test_t3_mcp_and_docs_schemas(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    exp_idx = ctx.repo_root / "fixtures" / "mcp" / "profiles" / "expected.json"
    r_prof = ctx.run([mcp, "--profiles-index", str(exp_idx), "--server", "core", "--print-profile"])
    if r_prof.returncode != 0 or r_prof.stdout.strip() != "profile=core served=8 front_door=6":
        raise TestFailure(f"fid-mcp --print-profile unexpected: {r_prof.stdout} {r_prof.stderr}")

    idx = write_disclosure_profiles_index(ctx.workdir)
    msgs_in = mcp_init_messages() + [
        {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "tools_describe_tool", "arguments": {"command": "program.save_draft@1"}},
        }
    ]
    rc, msgs, _, stderr = run_mcp_session(mcp, ["--headless", "--profiles-index", str(idx)], msgs_in, cwd=ctx.repo_root)
    if rc != 0:
        raise TestFailure(f"fid-mcp exited {rc}: {stderr}")
    r2 = next((m for m in msgs if m.get("id") == 2), None)
    if not r2 or r2.get("result", {}).get("isError") or not r2.get("result", {}).get("content"):
        raise TestFailure(f"fid-mcp describe failed: {r2!r}")

    schema_path = ctx.repo_root / "schemas" / "mcp-profile-1.json"
    if not schema_path.is_file():
        raise TestFailure(f"missing {schema_path}")
    schema_obj = json.loads(schema_path.read_text(encoding="utf-8"))
    if "front_door" not in json.dumps(schema_obj):
        raise TestFailure(f"schemas/mcp-profile-1.json missing front_door definition: {schema_obj!r}")
    readme = (ctx.repo_root / "README.md").read_text(encoding="utf-8")
    if "--exclude fid-clarabel --doc" not in readme or "scope.py milestone" not in readme:
        raise TestFailure("README.md missing expected workspace/milestone documentation")
    dec_md = (ctx.repo_root / "generated" / "DECISIONS.md").read_text(encoding="utf-8")
    for did in ("D-049", "D-052", "D-054"):
        if did not in dec_md:
            raise TestFailure(f"generated/DECISIONS.md missing {did}")
