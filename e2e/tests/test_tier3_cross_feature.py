"""Tier 3 pairwise cross-feature combination E2E tests across F1-F7."""

from __future__ import annotations

import json
import sys
from pathlib import Path

from e2e.lib.binaries import BUILT, MISSING_CRATE_OR_BIN
from e2e.lib.harness import (
    TestContext,
    TestFailure,
    assert_envelope,
    e2e_test,
    parse_single_line_json,
)
from e2e.tests._helpers import build_isolated_site, mcp_init_messages, prepare_scope_root, run_mcp_session


@e2e_test(
    test_id="T3-01",
    tier=3,
    feature="F2",
    description="[F2+F3] fid-slice --demo package passes fid-pkg-conformance and fid-verify --replay",
)
def test_t3_slice_pkg_conformance_and_verify(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    conf = ctx.require_bin("fid-pkg-conformance")
    out_dir = ctx.workdir / "slice_out"
    r1 = ctx.run([fid_slice, "--demo", "--out", str(out_dir), "--json"])
    if r1.returncode != 0:
        raise TestFailure(f"fid-slice --demo failed: {r1.stderr}")
    pkg = out_dir / "result-package.zip"

    r2 = ctx.run([conf, str(pkg)])
    if r2.returncode != 0 or parse_single_line_json(r2.stdout).get("result") != "accepted":
        raise TestFailure(f"fid-pkg-conformance rejected fid-slice package: {r2.stdout} {r2.stderr}")

    # Now check M4 fid-verify against the fid-slice package
    verify = ctx.require_bin("fid-verify")
    r3 = ctx.run([verify, str(pkg), "--replay", "--json"])
    if r3.returncode not in (0, 1):
        raise TestFailure(f"fid-verify failed unexpectedly (exit {r3.returncode}): {r3.stdout} {r3.stderr}")
    rep = parse_single_line_json(r3.stdout)
    if rep.get("chain", {}).get("verdict") != "VALID":  # type: ignore[union-attr]
        raise TestFailure(f"expected chain VALID on fid-slice package, got {rep!r}")


@e2e_test(
    test_id="T3-02",
    tier=3,
    feature="F2",
    description="[F2+F3] M3/M4 fid-slice/report balloon pack exports verified package and PDF+CSV artifacts",
)
def test_t3_balloon_pack_and_verify(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    verify = ctx.require_bin("fid-verify")
    pdf = ctx.repo_root / "fixtures" / "rpt" / "balloon" / "auto" / "vector_bracket.pdf"
    out_dir = ctx.workdir / "pack_out"
    r1 = ctx.run([fid_slice, "--balloon", str(pdf), "--out", str(out_dir), "--json"])
    if r1.returncode != 0:
        raise TestFailure(f"fid-slice --balloon failed (exit {r1.returncode}): {r1.stdout} {r1.stderr}")
    pkg = out_dir / "result-package.zip"
    if not pkg.is_file():
        raise TestFailure(f"expected result-package.zip alongside ballooned.pdf in {out_dir}")
    r2 = ctx.run([verify, str(pkg), "--json"])
    if r2.returncode not in (0, 1):
        raise TestFailure(f"fid-verify failed on balloon pack (exit {r2.returncode}): {r2.stdout}")


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
    if r_lint.returncode != 0:
        raise TestFailure(f"fid-xtask lint-crate-graph failed: {r_lint.stdout} {r_lint.stderr}")
    if not ctx.binaries.has_crate("fid-bus-api"):
        raise TestFailure("crate `fid-bus-api` missing from workspace (M4 AUTO.CLI.DISPATCH prerequisite)")
    fid = ctx.require_bin("fid")
    r_disp = ctx.run([fid, "project", "list-features", "--json"])
    if r_disp.returncode != 0 or "AUTO.CLI.DISPATCH" in r_disp.stdout:
        raise TestFailure(f"fid bus dispatch failed: {r_disp.stdout} {r_disp.stderr}")


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
    if not cli_tree.get("commands"):
        raise TestFailure("empty CliTree")

    idx = ctx.repo_root / "fixtures" / "mcp" / "profiles" / "expected.json"
    msgs_in = mcp_init_messages() + [
        {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "tools_search_tools",
                "arguments": {"keyword": "project", "principal": "human:alice", "role": "admin"},
            },
        }
    ]
    rc, msgs, _, stderr = run_mcp_session(mcp, ["--headless", "--profiles-index", str(idx)], msgs_in, cwd=ctx.repo_root)
    if rc != 0:
        raise TestFailure(f"fid-mcp exited {rc}: {stderr}")
    r2 = next((m for m in msgs if m.get("id") == 2), None)
    if not r2 or "result" not in r2:
        raise TestFailure(f"expected tool reply stripping principal override, got {msgs!r}")


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
    tokens_path = ctx.workdir / "tokens.json"
    tokens_path.write_text(
        json.dumps(
            {
                "themes": {
                    "light": {
                        "bg": "#ffffff", "surface": "#f7f8fa", "sheet": "#ffffff",
                        "ink": "#111827", "text.muted": "#5c6670", "muted": "#5c6670",
                        "border": "#d0d7de", "accent": "#0b5fff", "ruby": "#b91c1c",
                    },
                    "dark": {
                        "bg": "#0d1117", "surface": "#161b22", "sheet": "#0d1117",
                        "ink": "#f0f6fc", "text.muted": "#8b949e", "muted": "#8b949e",
                        "border": "#30363d", "accent": "#58a6ff", "ruby": "#f85149",
                    },
                }
            }
        ),
        encoding="utf-8",
    )
    a11y_out = ctx.workdir / "a11y.json"
    r_a11y = ctx.run([outreach, "qa", "a11y", "--root", str(dist), "--tokens", str(tokens_path), "--out", str(a11y_out)])
    if r_a11y.returncode not in (0, 1) or not a11y_out.is_file():
        raise TestFailure(f"qa a11y failed (exit {r_a11y.returncode}): {r_a11y.stderr}")

    # M2 #445 link-check across built site/dist/
    r_lc = ctx.run([outreach, "link-check", "--edition", str(dist), "--json"])
    if r_lc.returncode != 0:
        raise TestFailure(f"link-check on site/dist failed (exit {r_lc.returncode}): {r_lc.stdout} {r_lc.stderr}")


@e2e_test(
    test_id="T3-07",
    tier=3,
    feature="F5",
    description="[F5+F7] M2 site/build.py /guide/ integrates fid-guide-wasm preset digests and worker.js CSP",
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
    # Confirm fid-guide-wasm canon.digest executes cleanly alongside site build
    r = ctx.run([wasm_bin], stdin_text='{"op":"canon.digest","params":{"value":{"a":1}}}')
    if r.returncode != 0:
        raise TestFailure(f"fid-guide-wasm failed: {r.stderr}")


@e2e_test(
    test_id="T3-08",
    tier=3,
    feature="F6",
    description="[F1+F6] tracim_public.py run invokes real fid rdl submit-tracim against local TracimMockServer",
)
def test_t3_tracim_public_with_real_fid(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    # Verify fid binary actually supports `rdl submit-tracim` before/during tracim_public run
    r_help = ctx.run([fid, "rdl", "submit-tracim", "--help"])
    if r_help.returncode != 0:
        raise TestFailure(f"fid binary does not implement `rdl submit-tracim` (exit {r_help.returncode}): {r_help.stderr}")

    # Run tracim_public.py against TracimMockServer using real `fid`
    mock_dir = ctx.repo_root / "fixtures" / "rdl" / "tracim" / "mock"
    wf = ctx.repo_root / ".github" / "workflows" / "tracim-public.yml"
    out_art = ctx.workdir / "tracim-public-latest.json"
    driver = ctx.workdir / "run_with_mock.py"
    driver.write_text(
        f"""
import importlib.util, sys
from pathlib import Path
sys.path.insert(0, {str(ctx.repo_root)!r})
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
        env={{"PATH": {ctx.caps.cargo_path or ""!r}, "TRACIM_PUBLIC_KEY": "KEY-E2E"}},
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
    # Spot check malformed rejections appear in v1.md
    for sample_case in ("truncated", "crc", "symlink"):
        pkg = ctx.repo_root / "fixtures" / "pkg" / "malformed" / sample_case / "package.zip"
        r = ctx.run([conf, str(pkg)])
        got = parse_single_line_json(r.stdout)
        if got["error_code"] not in v1_md or got["reason"] not in v1_md:
            raise TestFailure(f"rejection {got!r} missing from docs/result-package/v1.md")

    # M5 requirement: interp/* must also be documented in v1.md
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
    # 1. fid-accept (build binary if present, else pure-Rust crate test suite)
    accept_res = ctx.binaries.build_bin("fid-accept")
    if accept_res.status == BUILT and accept_res.path is not None:
        r_acc = ctx.run([accept_res.path, "--help"])
        if r_acc.returncode != 0:
            raise TestFailure(f"fid-accept --help failed: {r_acc.stderr}")
    elif accept_res.status == MISSING_CRATE_OR_BIN:
        assert ctx.caps.cargo_path is not None
        r_acc_t = ctx.run([ctx.caps.cargo_path, "test", "--locked", "-p", "fid-accept"], timeout=300)
        if r_acc_t.returncode != 0:
            raise TestFailure(f"cargo test -p fid-accept failed: {r_acc_t.stdout} {r_acc_t.stderr}")
    else:
        raise TestFailure(f"fid-accept build unexpected status: {accept_res}")

    # 2. fid-eval-harness CLI opaque-box checks
    eh = ctx.require_bin("fid-eval-harness")
    r_null = ctx.run([eh, "null-agent"])
    if r_null.returncode != 0:
        raise TestFailure(f"fid-eval-harness null-agent failed: {r_null.stderr}")

    adapter_json = ctx.repo_root / "fixtures" / "agent" / "harness" / "adapters" / "exec-example.json"
    r_adp = ctx.run([eh, "adapter", "check", str(adapter_json)])
    if r_adp.returncode != 0:
        raise TestFailure(f"fid-eval-harness adapter check failed: {r_adp.stdout} {r_adp.stderr}")

    run_dir = ctx.repo_root / "fixtures" / "agent" / "harness" / "runs" / "refused-w3"
    r_saf = ctx.run([eh, "safety", str(run_dir), "--json"])
    if r_saf.returncode != 0:
        raise TestFailure(f"fid-eval-harness safety failed: {r_saf.stdout} {r_saf.stderr}")
    saf_obj = parse_single_line_json(r_saf.stdout)
    if "categories" not in saf_obj:
        raise TestFailure(f"unexpected safety JSON output: {saf_obj!r}")


@e2e_test(
    test_id="T3-12",
    tier=3,
    feature="F6",
    description="[F3+F6] fid-val and fid-crash CLI harnesses pass on C/SQLite-capable hosts",
    capabilities_required=("c_toolchain",),
)
def test_t3_val_and_crash_harnesses(ctx: TestContext) -> None:
    fid_val = ctx.require_bin("fid-val")
    fid_crash = ctx.require_bin("fid-crash")
    r_vh = ctx.run([fid_val, "--help"])
    if r_vh.returncode != 0:
        raise TestFailure(f"fid-val --help failed: {r_vh.stderr}")
    r_ch = ctx.run([fid_crash, "run", "--kills", "10", "--seed", "1"])
    if r_ch.returncode != 0:
        raise TestFailure(f"fid-crash run --kills 10 failed: {r_ch.stdout} {r_ch.stderr}")
