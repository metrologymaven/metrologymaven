"""Tier 2 boundary, negative, and error-handling E2E tests for F5, F6, and F7."""

from __future__ import annotations

import json
import re
import shutil
import sys
from pathlib import Path

from e2e.lib.harness import TestContext, TestFailure, e2e_test, parse_single_line_json
from e2e.tests._helpers import build_isolated_site, prepare_scope_root


# ===========================================================================
# F5 Boundaries & Negative Tests (5 tests)
# ===========================================================================


@e2e_test(
    test_id="T2-F5-01",
    tier=2,
    feature="F5",
    description="site/build.py & worker.js enforce zero inline <script> and no wasm-unsafe-eval outside /guide/*",
)
def test_site_csp_and_no_inline_script_invariant(ctx: TestContext) -> None:
    copy_root = build_isolated_site(ctx)
    dist = copy_root / "site" / "dist"
    for p in dist.rglob("*.html"):
        txt = p.read_text(encoding="utf-8")
        rel_p = p.relative_to(dist).as_posix()
        is_guide = rel_p.startswith("guide/")
        if not is_guide and re.search(r"<script\b", txt, re.I):
            raise TestFailure(f"<script> tag forbidden outside /guide/ found in {rel_p}")
        if re.search(r"<script\b(?![^>]*\bsrc=)[^>]*>", txt, re.I):
            raise TestFailure(f"inline script tag found in {rel_p}")
        if "wasm-unsafe-eval" in txt:
            raise TestFailure(f"HTML page {rel_p} should never embed wasm-unsafe-eval")
        for m in re.finditer(
            r'''(?:src|href|action|poster)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+))''',
            txt,
            re.I,
        ):
            url = (m.group(1) or m.group(2) or m.group(3) or "").strip()
            if url.startswith("//") or (
                re.match(r"https?://", url, re.I)
                and not (
                    url.startswith("https://metrologymaven.io")
                    or url.startswith("https://github.com/metrologymaven/")
                )
            ):
                raise TestFailure(f"disallowed external/protocol-relative URL {url!r} in {rel_p}")
        for m in re.finditer(r'''url\(\s*["']?\s*(https?:|//)[^)"'\s]+''', txt, re.I):
            raise TestFailure(f"external CSS url() {m.group(0)!r} in {rel_p}")

    worker = (ctx.repo_root / "site" / "worker.js").read_text(encoding="utf-8")
    # Default route CSP must remain script-src 'none'
    if "script-src 'none'" not in worker:
        raise TestFailure("site/worker.js missing default script-src 'none' CSP")


@e2e_test(
    test_id="T2-F5-02",
    tier=2,
    feature="F5",
    description="M2 #565 fid-guide-wasm rejects >262144 bytes, NaN, <3 circle points, and unknown op without panicking",
)
def test_guide_wasm_hostile_inputs(ctx: TestContext) -> None:
    wasm_bin = ctx.require_bin("fid-guide-wasm")
    hostile_cases = [
        ("oversized", json.dumps({"op": "canon.digest", "params": {"pad": "x" * 265_000}})),
        ("nan", '{"op":"fit.circle2d","params":{"points":[[NaN,0],[0,1],[1,0]]}}'),
        ("too_few_points", json.dumps({"op": "fit.circle2d", "params": {"points": [[0, 0], [1, 0]]}})),
        ("unknown_op", json.dumps({"op": "evil.unknown", "params": {}})),
    ]
    for label, payload in hostile_cases:
        res = ctx.run([wasm_bin], stdin_text=payload)
        if res.returncode == 0:
            raise TestFailure(f"fid-guide-wasm accepted hostile input {label}: {res.stdout}")
        if "panicked" in res.stderr.lower():
            raise TestFailure(f"fid-guide-wasm panicked on {label}: {res.stderr}")
        out = parse_single_line_json(res.stdout)
        if out.get("ok") is not False:
            raise TestFailure(f"expected ok=false typed JSON error for {label}, got {out!r}")


@e2e_test(
    test_id="T2-F5-03",
    tier=2,
    feature="F5",
    description="fid-outreach doc-run rejects stale revision (exit 1) and shell metacharacters / python -c (exit 2)",
)
def test_outreach_doc_run_negative(ctx: TestContext) -> None:
    outreach = ctx.require_bin("fid-outreach")

    # 1. Stale revision -> exit 1 revision_mismatch
    stale_guide = ctx.workdir / "stale.md"
    stale_guide.write_text(
        "---\ntitle: Stale\nrevision: " + ("0" * 40) + "\n---\n\n```sh run\npython tools/scope.py stats\n```\n",
        encoding="utf-8",
    )
    r_stale = ctx.run([outreach, "doc-run", "--guide", str(stale_guide), "--repo", str(ctx.repo_root), "--json"])
    if r_stale.returncode != 1:
        raise TestFailure(f"expected exit 1 on stale revision, got {r_stale.returncode}: {r_stale.stdout}")
    rep = parse_single_line_json(r_stale.stdout)
    if rep.get("error") != "revision_mismatch":
        raise TestFailure(f"expected error=revision_mismatch, got {rep!r}")

    # 2. Shell metacharacter (`|`) & `python -c` -> exit 2 guide_invalid
    head = ctx.run(["git", "rev-parse", "HEAD"]).stdout.strip()
    for label, cmd_line in (
        ("pipe", "python tools/scope.py stats | cat"),
        ("python_c", "python -c \"print(1)\""),
    ):
        bad_guide = ctx.workdir / f"bad_{label}.md"
        bad_guide.write_text(
            f"---\ntitle: Bad\nrevision: {head}\n---\n\n```sh run\n{cmd_line}\n```\n",
            encoding="utf-8",
        )
        r_bad = ctx.run([outreach, "doc-run", "--guide", str(bad_guide), "--repo", str(ctx.repo_root), "--json"])
        if r_bad.returncode != 2:
            raise TestFailure(f"expected exit 2 on {label}, got {r_bad.returncode}: {r_bad.stdout}")
        err = parse_single_line_json(r_bad.stdout)
        if err.get("error") != "guide_invalid":
            raise TestFailure(f"expected guide_invalid on {label}, got {err!r}")


@e2e_test(
    test_id="T2-F5-04",
    tier=2,
    feature="F5",
    description="M2 #445 fid-outreach link-check exits 1 on broken internal link and missing fragment",
)
def test_outreach_link_check_broken(ctx: TestContext) -> None:
    outreach = ctx.require_bin("fid-outreach")
    broken_dir = ctx.workdir / "broken_edition"
    broken_dir.mkdir(parents=True, exist_ok=True)
    (broken_dir / "index.html").write_text(
        '<!doctype html><html lang="en"><head><title>Test</title></head><body>'
        '<a href="ghost.html">Missing</a>'
        '<a href="other.html#nope">Missing fragment</a>'
        "</body></html>",
        encoding="utf-8",
    )
    (broken_dir / "other.html").write_text(
        '<!doctype html><html lang="en"><head><title>Other</title></head><body><h1 id="ok">Ok</h1></body></html>',
        encoding="utf-8",
    )
    res = ctx.run([outreach, "link-check", "--edition", str(broken_dir), "--json"])
    if res.returncode != 1:
        raise TestFailure(f"expected exit 1 on broken internal links, got {res.returncode}: {res.stdout} {res.stderr}")
    if "link_target_missing" not in res.stdout or "fragment_missing" not in res.stdout:
        raise TestFailure(f"expected link_target_missing and fragment_missing in output: {res.stdout!r}")


@e2e_test(
    test_id="T2-F5-05",
    tier=2,
    feature="F5",
    description="tools/verify_guide.py fails cleanly if SHA256SUMS target or doc tag is invalid",
)
def test_verify_guide_tamper_and_missing_tag(ctx: TestContext) -> None:
    script = ctx.repo_root / "tools" / "verify_guide.py"
    # Verify --doc without the requested tag exits non-zero cleanly
    empty_doc = ctx.workdir / "EMPTY_VERIFY.md"
    empty_doc.write_text("# Empty\n", encoding="utf-8")
    tag = "verify-windows" if ctx.caps.is_windows else "verify-linux"
    r_missing = ctx.run([sys.executable, str(script), "--tag", tag, "--doc", str(empty_doc)])
    if r_missing.returncode == 0:
        raise TestFailure("verify_guide.py should fail when tag block is missing")

    # Verify a broken VERIFY.md that ignores checksum failures is caught by verify_guide.py
    noop_doc = ctx.workdir / "NOOP_VERIFY.md"
    noop_doc.write_text(f"```sh {tag}\nexit 0\n```\n", encoding="utf-8")
    r_noop = ctx.run([sys.executable, str(script), "--tag", tag, "--doc", str(noop_doc)])
    if r_noop.returncode == 0:
        raise TestFailure("verify_guide.py should fail when tampered copy is accepted with exit 0")

    # Direct 1-byte tampered SHA256SUMS negative test against real VERIFY.md
    tampered_rel = ctx.workdir / "tampered_sums_rel"
    shutil.copytree(ctx.repo_root / "fixtures" / "rel" / "guide" / "release", tampered_rel)
    sums_path = tampered_rel / "SHA256SUMS"
    sums_txt = sums_path.read_text(encoding="ascii")
    flipped_char = "1" if sums_txt[0] != "1" else "2"
    sums_path.write_text(flipped_char + sums_txt[1:], encoding="ascii")
    r_bad_sums = ctx.run([sys.executable, str(script), "--tag", tag, "--release", str(tampered_rel)], timeout=120)
    if r_bad_sums.returncode == 0:
        raise TestFailure("verify_guide.py accepted a 1-byte tampered SHA256SUMS file")


# ===========================================================================
# F6 Boundaries & Negative Tests (5 tests)
# ===========================================================================


@e2e_test(
    test_id="T2-F6-01",
    tier=2,
    feature="F6",
    description="scope.py check rejects circular depends_on, Windows reserved ID CON, and built_in on commercial_repo",
)
def test_scope_py_negative_invariants(ctx: TestContext) -> None:
    scope_py = ctx.repo_root / "tools" / "scope.py"

    # Case 1: Windows reserved ID CON
    c1 = prepare_scope_root(ctx, "c_reserved")
    bad_dir = c1 / "spec" / "core" / "con"
    bad_dir.mkdir(parents=True, exist_ok=True)
    (bad_dir / "NODE.md").write_text(
        "---\nid: CORE.CON\ntitle: Reserved\nlevel: 2\nparent: CORE\nkind: subsystem\n"
        "status: open\nphase: P0\nsize: M\nsatisfies: []\ncovers: []\n---\n# CORE.CON\n",
        encoding="utf-8",
    )
    r1 = ctx.run([sys.executable, str(scope_py), "check", "--root", str(c1)])
    if r1.returncode != 1 or "reserved Windows device name" not in r1.stdout:
        raise TestFailure(f"expected exit 1 + reserved Windows device name, got {r1.returncode}: {r1.stdout}")

    # Case 2: Circular depends_on (CORE.JRN.CANON <-> CORE.JRN.ENTRY)
    c2 = prepare_scope_root(ctx, "c_cycle")
    n_a = c2 / "spec" / "core" / "jrn" / "canon" / "NODE.md"
    txt_a = n_a.read_text(encoding="utf-8")
    txt_a = re.sub(r"^depends_on:.*$", "depends_on: [CORE.JRN.ENTRY]", txt_a, flags=re.M)
    if "depends_on:" not in txt_a:
        txt_a = txt_a.replace("\n---\n", "\ndepends_on: [CORE.JRN.ENTRY]\n---\n", 1)
    n_a.write_text(txt_a, encoding="utf-8")
    n_b = c2 / "spec" / "core" / "jrn" / "entry" / "NODE.md"
    txt_b = n_b.read_text(encoding="utf-8")
    txt_b = re.sub(r"^depends_on:.*$", "depends_on: [CORE.JRN.CANON]", txt_b, flags=re.M)
    if "depends_on:" not in txt_b:
        txt_b = txt_b.replace("\n---\n", "\ndepends_on: [CORE.JRN.CANON]\n---\n", 1)
    n_b.write_text(txt_b, encoding="utf-8")
    r2 = ctx.run([sys.executable, str(scope_py), "check", "--root", str(c2)])
    if r2.returncode != 1 or "cycle:" not in r2.stdout:
        raise TestFailure(f"expected exit 1 + cycle: error, got {r2.returncode}: {r2.stdout}")

    # Case 3: built_in on commercial_repo node (D-053)
    c3 = prepare_scope_root(ctx, "c_comm")
    for candidate in sorted((c3 / "spec" / "core").rglob("NODE.md")):
        t = candidate.read_text(encoding="utf-8")
        if "status: leaf" in t and "commercial_repo:" not in t:
            t = t.replace("status: leaf", "status: leaf\ncommercial_repo: metrologymaven-enterprise\nbuilt_in: [fid-canon]", 1)
            candidate.write_text(t, encoding="utf-8")
            break
    r3 = ctx.run([sys.executable, str(scope_py), "check", "--root", str(c3)])
    if r3.returncode != 1 or "D-053" not in r3.stdout:
        raise TestFailure(f"expected exit 1 + D-053 error on built_in + commercial_repo, got {r3.returncode}: {r3.stdout}")


@e2e_test(
    test_id="T2-F6-02",
    tier=2,
    feature="F6",
    description="tracim_public.py fails closed (exit 1 paid_key_exposed) on TRACIM_PAID_KEY before running fid",
)
def test_tracim_public_paid_key_guard(ctx: TestContext) -> None:
    script = ctx.repo_root / "tools" / "rdl" / "tracim_public.py"
    wf = ctx.repo_root / ".github" / "workflows" / "tracim-public.yml"
    out_json = ctx.workdir / "tracim-out.json"
    sentinel_marker = ctx.workdir / "should_not_exist.txt"

    # Pass a non-existent fid command that would fail differently if executed; paid_key_exposed must trigger first.
    res = ctx.run(
        [
            sys.executable,
            str(script),
            "run",
            "--out",
            str(out_json),
            "--workflow",
            str(wf),
            "--",
            sys.executable,
            "-c",
            f"open({str(sentinel_marker)!r}, 'w').write('ran')",
        ],
        env={"TRACIM_PAID_ORDER_KEY": "SENTINEL-PAID-SECRET"},
    )
    if res.returncode != 1:
        raise TestFailure(f"expected exit 1 on TRACIM_PAID_ORDER_KEY, got {res.returncode}: {res.stdout} {res.stderr}")
    if sentinel_marker.exists():
        raise TestFailure("tracim_public.py executed child command despite paid_key_exposed")
    artifact = json.loads(out_json.read_text(encoding="utf-8"))
    if artifact.get("reason") != "paid_key_exposed" or "SENTINEL-PAID-SECRET" in out_json.read_text(encoding="utf-8"):
        raise TestFailure(f"unexpected artifact on paid_key_exposed: {artifact!r}")


@e2e_test(
    test_id="T2-F6-03",
    tier=2,
    feature="F6",
    description="M1 fid-xtask fmt exits 2 on --max-batch-bytes 0 and exits 1 on formatting drift",
    capabilities_required=("c_toolchain",),
)
def test_xtask_fmt_negative(ctx: TestContext) -> None:
    xtask = ctx.require_bin("fid-xtask")
    r_bad = ctx.run([xtask, "fmt", "--check", "--max-batch-bytes", "0", "--workspace", str(ctx.repo_root)])
    if r_bad.returncode != 2:
        raise TestFailure(f"expected exit 2 on --max-batch-bytes 0, got {r_bad.returncode}")

    # Create a tiny valid Rust workspace in temp with unformatted code to verify drift detection (exit 1).
    ws = ctx.workdir / "mini_ws"
    (ws / "src").mkdir(parents=True)
    (ws / "Cargo.toml").write_text(
        '[package]\nname = "mini"\nversion = "0.1.0"\nedition = "2021"\n', encoding="utf-8"
    )
    (ws / "src" / "lib.rs").write_text("pub   fn   x( ) ->  u32{1}\n", encoding="utf-8")
    r_drift = ctx.run([xtask, "fmt", "--check", "--workspace", str(ws)])
    if r_drift.returncode != 1:
        raise TestFailure(f"expected exit 1 on unformatted temp crate, got {r_drift.returncode}: {r_drift.stderr}")


@e2e_test(
    test_id="T2-F6-04",
    tier=2,
    feature="F6",
    description="fid-xtask rel notes neutralizes hostile PR titles and exits 1 (NOTES-MISSING) on missing reports",
    capabilities_required=("c_toolchain",),
)
def test_xtask_rel_notes_hostile_and_missing(ctx: TestContext) -> None:
    xtask = ctx.require_bin("fid-xtask")
    # 1. Missing report -> exit 1 NOTES-MISSING
    miss_dir = ctx.repo_root / "fixtures" / "rel" / "notes" / "missing-status"
    prs = ctx.repo_root / "fixtures" / "rel" / "notes" / "basic" / "prs.json"
    out_dir = ctx.workdir / "notes_out"
    r_miss = ctx.run(
        [
            xtask,
            "rel",
            "notes",
            "--tag",
            "v1.4.0-beta.2",
            "--channel",
            "beta",
            "--prs",
            str(prs),
            "--reports",
            str(miss_dir / "reports"),
            "--out",
            str(out_dir),
        ]
    )
    if r_miss.returncode != 1 or "NOTES-MISSING" not in r_miss.stdout:
        raise TestFailure(f"expected exit 1 NOTES-MISSING, got {r_miss.returncode}: {r_miss.stdout} {r_miss.stderr}")

    # 2. Hostile title -> rendered safely inside code span
    hostile_prs = ctx.repo_root / "fixtures" / "rel" / "notes" / "hostile-title" / "prs.json"
    reports_ok = ctx.repo_root / "fixtures" / "rel" / "notes" / "basic" / "reports"
    r_hostile = ctx.run(
        [
            xtask,
            "rel",
            "notes",
            "--tag",
            "v1.4.0-beta.2",
            "--channel",
            "beta",
            "--prs",
            str(hostile_prs),
            "--reports",
            str(reports_ok),
            "--out",
            str(out_dir),
        ]
    )
    if r_hostile.returncode != 0:
        raise TestFailure(f"expected exit 0 neutralizing hostile PR title, got {r_hostile.returncode}: {r_hostile.stderr}")
    md = (out_dir / "release-notes.md").read_text(encoding="utf-8")
    if "[click](javascript:" in md:
        raise TestFailure(f"unescaped markdown link in release-notes.md:\n{md}")


@e2e_test(
    test_id="T2-F6-05",
    tier=2,
    feature="F6",
    description="M4 SCAN.CMP.DEV.PRIM invalid-parameter / boundary tests exist and pass via cargo test -p fid-scan-cmp",
)
def test_scan_cmp_dev_prim_invalid_params(ctx: TestContext) -> None:
    if not ctx.caps.cargo_path:
        raise TestFailure("cargo executable required for fid-scan-cmp tests")
    r_list = ctx.run([ctx.caps.cargo_path, "test", "--locked", "-p", "fid-scan-cmp", "--", "--list"])
    if r_list.returncode != 0:
        raise TestFailure(f"cargo test -p fid-scan-cmp --list failed: {r_list.stdout} {r_list.stderr}")
    dev_boundary_tests = [
        line.split(": test")[0].strip()
        for line in r_list.stdout.splitlines()
        if ": test" in line
        and not line.lstrip().startswith("field::")
        and (line.lstrip().startswith("dev::") or "deviation" in line.lower())
        and any(tok in line.lower() for tok in ("invalid", "param", "angle", "distance", "ac4", "reject"))
    ]
    if not dev_boundary_tests:
        raise TestFailure(
            "SCAN.CMP.DEV.PRIM boundary/invalid_parameter tests missing in `cargo test -p fid-scan-cmp -- --list`"
        )
    r_run = ctx.run([ctx.caps.cargo_path, "test", "--locked", "-p", "fid-scan-cmp", "--", "dev"])
    if r_run.returncode != 0:
        raise TestFailure(f"cargo test -p fid-scan-cmp failed: {r_run.stdout} {r_run.stderr}")
    m_pass = re.search(r"test result:\s*ok\.\s*(\d+)\s+passed;\s*0\s+failed", r_run.stdout)
    if not m_pass or int(m_pass.group(1)) == 0:
        raise TestFailure(f"expected >=1 passing SCAN.CMP.DEV.PRIM boundary test, got:\n{r_run.stdout}")


# ===========================================================================
# F7 Boundaries & Negative Tests (6 tests)
# ===========================================================================


@e2e_test(
    test_id="T2-F7-01",
    tier=2,
    feature="F7",
    description="check_spec_drift.py detects injected drift in temp repo copy and v1.md path/interp alignment",
)
def test_check_spec_drift_catches_drift(ctx: TestContext) -> None:
    script = ctx.repo_root / "fixtures" / "pkg" / "check_spec_drift.py"
    res = ctx.run([sys.executable, str(script)])
    if res.returncode != 0:
        raise TestFailure(f"check_spec_drift.py failed: {res.stdout} {res.stderr}")
    if "spec and code agree" not in res.stdout:
        raise TestFailure(f"unexpected check_spec_drift.py output: {res.stdout!r}")

    # Inject spec drift in an isolated temp copy and verify check_spec_drift.py exits 1 naming the drift.
    probe_root = ctx.workdir / "drift_probe"
    ctx.copy_repo_subset(probe_root, ["fixtures/pkg/check_spec_drift.py", "docs/result-package/v1.md", "crates/fid-pkg/src"])
    v1_copy = probe_root / "docs" / "result-package" / "v1.md"
    v1_txt = v1_copy.read_text(encoding="utf-8")
    v1_copy.write_text(v1_txt + "\n### `pkg.injected_e2e_drift`\n\nInjected drift.\n", encoding="utf-8")
    r_drift = ctx.run([sys.executable, str(probe_root / "fixtures" / "pkg" / "check_spec_drift.py")], cwd=probe_root)
    if r_drift.returncode != 1 or "pkg.injected_e2e_drift" not in r_drift.stdout:
        raise TestFailure(
            f"check_spec_drift.py failed to catch injected drift (exit {r_drift.returncode}): {r_drift.stdout}"
        )

    v1_md = (ctx.repo_root / "docs" / "result-package" / "v1.md").read_text(encoding="utf-8")
    if "evidence/points/" in v1_md or "evidence/register/" in v1_md:
        raise TestFailure("docs/result-package/v1.md contains incorrect evidence/points/ or evidence/register/ path")
    m = re.search(r"<!-- members -->(.*?)<!-- /members -->", v1_md, re.S)
    if not m or "interp/registers/<sha256>.json" not in m.group(1) or "interp/rosters/<sha256>.json" not in m.group(1):
        raise TestFailure("docs/result-package/v1.md <!-- members --> omits interp/registers/<sha256>.json or interp/rosters/<sha256>.json")


@e2e_test(
    test_id="T2-F7-02",
    tier=2,
    feature="F7",
    description="M5 docs/result-package/v1.md <!-- members --> includes interp/registers and interp/rosters",
)
def test_v1_md_members_table_complete(ctx: TestContext) -> None:
    v1_md = (ctx.repo_root / "docs" / "result-package" / "v1.md").read_text(encoding="utf-8")
    m = re.search(r"<!-- members -->(.*?)<!-- /members -->", v1_md, re.S)
    if not m:
        raise TestFailure("missing <!-- members --> block in docs/result-package/v1.md")
    block = m.group(1)
    if "interp/registers/" not in block or "interp/rosters/" not in block:
        raise TestFailure("docs/result-package/v1.md <!-- members --> omits interp/registers/ and interp/rosters/")


@e2e_test(
    test_id="T2-F7-03",
    tier=2,
    feature="F7",
    description="M5 docs/platform/tiers.md does not falsely claim PLAT.MATRIX.POLICY.EOL is unbuilt",
)
def test_tiers_md_no_false_unbuilt_eol(ctx: TestContext) -> None:
    tiers = (ctx.repo_root / "docs" / "platform" / "tiers.md").read_text(encoding="utf-8")
    if "Not built yet" in tiers and "PLAT.MATRIX.POLICY.EOL" in tiers.split("Not built yet", 1)[1]:
        raise TestFailure("docs/platform/tiers.md falsely lists PLAT.MATRIX.POLICY.EOL under Not built yet")


@e2e_test(
    test_id="T2-F7-04",
    tier=2,
    feature="F7",
    description="M5 .github/PULL_REQUEST_TEMPLATE.md has no forbidden D-042/D-052 checklist items",
)
def test_pr_template_no_forbidden_clauses(ctx: TestContext) -> None:
    pr = (ctx.repo_root / ".github" / "PULL_REQUEST_TEMPLATE.md").read_text(encoding="utf-8")
    forbidden = (
        "generated/` is committed",
        "domain maintainer is requested as reviewer",
        "A person (not an agent or bot)",
    )
    hits = [f for f in forbidden if f in pr]
    if hits:
        raise TestFailure(f".github/PULL_REQUEST_TEMPLATE.md retains forbidden clauses: {hits}")


@e2e_test(
    test_id="T2-F7-05",
    tier=2,
    feature="F7",
    description="M5 generated/DECISIONS.md includes all decisions D-049..D-054",
)
def test_generated_decisions_includes_latest(ctx: TestContext) -> None:
    dec_dir = ctx.repo_root / "decisions"
    expected_ids = sorted(
        m.group(1)
        for p in dec_dir.glob("D-*.md")
        if (m := re.match(r"^(D-\d+)", p.name))
    )
    dec_md = (ctx.repo_root / "generated" / "DECISIONS.md").read_text(encoding="utf-8")
    missing = [did for did in expected_ids if did not in dec_md]
    if missing:
        raise TestFailure(f"generated/DECISIONS.md is missing decision records: {missing}")


@e2e_test(
    test_id="T2-F7-06",
    tier=2,
    feature="F7",
    description="fid-xtask rel runbook-check rejects drift, missing-gate, and orphan-job fixtures with exit 1",
    capabilities_required=("c_toolchain",),
)
def test_xtask_rel_runbook_check_negative_fixtures(ctx: TestContext) -> None:
    xtask = ctx.require_bin("fid-xtask")
    base = ctx.repo_root / "fixtures" / "rel" / "runbook"
    for bad in ("drift", "missing-gate", "orphan-job"):
        res = ctx.run([xtask, "rel", "runbook-check", "--dir", str(base / bad)])
        if res.returncode != 1:
            raise TestFailure(f"expected exit 1 on runbook fixture {bad}, got {res.returncode}: {res.stdout}")
