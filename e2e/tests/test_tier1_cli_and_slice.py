"""Tier 1 happy-path E2E tests for F1 (fid CLI) and F2 (fid-slice & VS0 auto-ballooning)."""

from __future__ import annotations

import csv
import json
import zipfile
from pathlib import Path

from e2e.lib.harness import (
    TestContext,
    TestFailure,
    assert_envelope,
    e2e_test,
    parse_single_line_json,
)
from e2e.tests._helpers import FidCtrlMockServer


# ===========================================================================
# F1: fid CLI, Canon, Ctrl & Bus Dispatch (6 happy-path tests)
# ===========================================================================


@e2e_test(
    test_id="T1-F1-01",
    tier=1,
    feature="F1",
    description="fid --version prints semver and embedded manifest sha256 digest",
)
def test_fid_version(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    res = ctx.run([fid, "--version"])
    if res.returncode != 0:
        raise TestFailure(f"fid --version exited {res.returncode}: {res.stderr}")
    line = res.stdout.strip()
    if not line.startswith("fid ") or "manifest sha256:" not in line:
        raise TestFailure(f"unexpected --version output: {line!r}")


@e2e_test(
    test_id="T1-F1-02",
    tier=1,
    feature="F1",
    description="fid --help --json returns valid CliTree JSON schema with commands and host subcommands",
)
def test_fid_help_json(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    res = ctx.run([fid, "--help", "--json"])
    if res.returncode != 0:
        raise TestFailure(f"fid --help --json exited {res.returncode}: {res.stderr}")
    tree = parse_single_line_json(res.stdout)
    ver = tree.get("version")
    if not isinstance(ver, str) or not ver.strip():
        raise TestFailure(f"expected non-empty version in CliTree JSON, got {ver!r}")
    digest = str(tree.get("manifest_digest", ""))
    if not digest.startswith("sha256:") or len(digest) != 71:
        raise TestFailure(f"invalid manifest_digest: {digest!r}")
    host_subs = tree.get("host_subcommands")
    if not isinstance(host_subs, list) or "ctrl" not in host_subs or "mcp" not in host_subs:
        raise TestFailure(f"expected ctrl and mcp in host_subcommands, got {host_subs!r}")
    cmds = tree.get("commands")
    if not isinstance(cmds, list) or len(cmds) == 0:
        raise TestFailure("expected non-empty commands list in CliTree")


@e2e_test(
    test_id="T1-F1-03",
    tier=1,
    feature="F1",
    description="fid align run-bestfit --help renders command metadata and parameters",
)
def test_fid_command_help(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    res = ctx.run([fid, "align", "run-bestfit", "--help"])
    if res.returncode != 0:
        raise TestFailure(f"fid align run-bestfit --help exited {res.returncode}: {res.stderr}")
    for token in ("Command:", "Manifest:", "Side-effect class:", "Determinism:", "Parameters:"):
        if token not in res.stdout:
            raise TestFailure(f"missing {token!r} in command help:\n{res.stdout}")


@e2e_test(
    test_id="T1-F1-04",
    tier=1,
    feature="F1",
    description="fid canon, --digest, and --json match fixtures/canon/vectors/* golden outputs",
)
def test_fid_canon_vectors(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    vec_dir = ctx.repo_root / "fixtures" / "canon" / "vectors"
    pairs = sorted(vec_dir.glob("*.json"))
    if not pairs:
        raise TestFailure(f"no canon vectors found in {vec_dir}")
    # Exercise a representative diverse subset across scalar, keys, numbers, utf16, depth
    sample = [
        "rfc8785_sample.json" if (vec_dir / "rfc8785_sample.json").exists() else "journal_like_header.json",
        "keys_ascii_order.json",
        "keys_utf16_order.json",
        "numbers_decimal_forms.json",
        "depth_128_mixed.json",
        "strings_raw_utf8.json",
    ]
    for name in sample:
        in_path = vec_dir / name
        canon_path = vec_dir / name.replace(".json", ".canon")
        expected = canon_path.read_text(encoding="utf-8").rstrip("\r\n")

        r_plain = ctx.run([fid, "canon", str(in_path)])
        if r_plain.returncode != 0:
            raise TestFailure(f"fid canon {name} exited {r_plain.returncode}: {r_plain.stderr}")
        if r_plain.stdout.rstrip("\r\n") != expected:
            raise TestFailure(f"fid canon {name} mismatch:\ngot:  {r_plain.stdout!r}\nwant: {expected!r}")

        r_dig = ctx.run([fid, "canon", "--digest", str(in_path)])
        if r_dig.returncode != 0:
            raise TestFailure(f"fid canon --digest {name} exited {r_dig.returncode}: {r_dig.stderr}")
        dig = r_dig.stdout.strip()
        if not dig.startswith("sha256:") or len(dig) != 71:
            raise TestFailure(f"bad digest output for {name}: {dig!r}")

        r_json = ctx.run([fid, "canon", "--json", str(in_path)])
        if r_json.returncode != 0:
            raise TestFailure(f"fid canon --json {name} exited {r_json.returncode}: {r_json.stderr}")
        env = parse_single_line_json(r_json.stdout)
        assert_envelope(env, outcome="computed", command_id="canon.canonicalize@1")
        result = env.get("result")
        if not isinstance(result, dict) or result.get("digest") != dig:
            raise TestFailure(f"digest mismatch between --digest and --json for {name}: {result!r} vs {dig!r}")


@e2e_test(
    test_id="T1-F1-05",
    tier=1,
    feature="F1",
    description="fid ctrl status & ping authenticate over loopback with mutual HMAC-SHA256",
)
def test_fid_ctrl_authenticated_status_and_ping(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    srv = FidCtrlMockServer(ctx.workdir / "ctrl")
    srv.start()
    try:
        r_status = ctx.run(
            [
                fid,
                "ctrl",
                "status",
                "--json",
                "--runtime-dir",
                str(srv.runtime_dir),
                "--token-file",
                str(srv.token_file),
            ]
        )
        if r_status.returncode != 0:
            raise TestFailure(
                f"fid ctrl status failed (exit {r_status.returncode}): stdout={r_status.stdout!r} stderr={r_status.stderr!r}"
            )
        env = parse_single_line_json(r_status.stdout)
        assert_envelope(env, outcome="computed", command_id="ctrl.status@1")
        res_obj = env.get("result")
        if not isinstance(res_obj, dict):
            raise TestFailure(f"missing result object: {env!r}")
        if res_obj.get("found") is not True or res_obj.get("authenticated") is not True:
            raise TestFailure(f"expected found=true & authenticated=true, got {res_obj!r}")
        if res_obj.get("instance_id") != srv.instance_id or res_obj.get("port") != srv.port:
            raise TestFailure(f"unexpected instance/port in ctrl status: {res_obj!r}")

        r_ping = ctx.run(
            [
                fid,
                "ctrl",
                "ping",
                "--count",
                "2",
                "--json",
                "--runtime-dir",
                str(srv.runtime_dir),
                "--token-file",
                str(srv.token_file),
            ]
        )
        if r_ping.returncode != 0:
            raise TestFailure(f"fid ctrl ping failed (exit {r_ping.returncode}): {r_ping.stderr}")
        env_p = parse_single_line_json(r_ping.stdout)
        assert_envelope(env_p, outcome="computed", command_id="ctrl.ping@1")
        p_obj = env_p.get("result")
        if not isinstance(p_obj, dict) or p_obj.get("count") != 2:
            raise TestFailure(f"unexpected ctrl ping result: {p_obj!r}")
    finally:
        srv.stop()


@e2e_test(
    test_id="T1-F1-06",
    tier=1,
    feature="F1",
    description="M4 AUTO.CLI.DISPATCH routes registry commands via fid-bus-api and includes rdl submit-tracim",
)
def test_fid_bus_dispatch_and_rdl_tracim(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    # 1. Verify rdl submit-tracim is wired into CLI tree (PROJECT.md item 4 / M4).
    r_help = ctx.run([fid, "rdl", "submit-tracim", "--help"])
    if r_help.returncode != 0:
        raise TestFailure(f"fid rdl submit-tracim --help failed (exit {r_help.returncode}): {r_help.stderr}")
    if "submit-tracim" not in r_help.stdout or "Command:" not in r_help.stdout:
        raise TestFailure(f"unexpected rdl submit-tracim --help output:\n{r_help.stdout}")

    # 2. Verify registry command dispatch does not hit pre-M4 placeholder stub.
    r_disp = ctx.run([fid, "project", "list-features", "--json"])
    if r_disp.returncode != 0:
        raise TestFailure(
            f"fid project list-features --json failed (exit {r_disp.returncode}): "
            f"stdout={r_disp.stdout!r} stderr={r_disp.stderr!r}"
        )
    env = parse_single_line_json(r_disp.stdout)
    if "AUTO.CLI.DISPATCH" in json.dumps(env):
        raise TestFailure(f"fid still returns pre-M4 placeholder: {env!r}")
    assert_envelope(env, command_id="project.list_features@1")
    if env.get("outcome") not in ("computed", "committed", "dry_run"):
        raise TestFailure(f"expected successful dispatch outcome, got {env!r}")
    if not isinstance(env.get("result"), dict):
        raise TestFailure(f"expected non-empty result dict in envelope, got {env!r}")


# ===========================================================================
# F2: fid-slice Vertical Slice 0 & 2D PDF Auto-Ballooning (6 happy-path tests)
# ===========================================================================


@e2e_test(
    test_id="T1-F2-01",
    tier=1,
    feature="F2",
    description="fid-slice --demo --out writes 6-member ZIP, form3.csv, records.json, 4 conforming chars",
)
def test_slice_demo_outputs(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    out_dir = ctx.workdir / "demo_out"
    res = ctx.run([fid_slice, "--demo", "--out", str(out_dir), "--json"])
    if res.returncode != 0:
        raise TestFailure(f"fid-slice --demo exited {res.returncode}: {res.stderr}")
    env = parse_single_line_json(res.stdout)
    assert_envelope(env, outcome="computed", command_id="slice.run_vertical@1")
    result = env.get("result")
    if not isinstance(result, dict):
        raise TestFailure(f"missing result object: {env!r}")
    if result.get("verified_members") != 6:
        raise TestFailure(f"expected verified_members=6, got {result.get('verified_members')!r}")
    chars = result.get("characteristics")
    if not isinstance(chars, list) or len(chars) != 4:
        raise TestFailure(f"expected 4 characteristics, got {chars!r}")
    if not all(str(c.get("outcome", "")).upper() == "CONFORMS" for c in chars):
        raise TestFailure(f"expected all 4 demo characteristics to conform: {chars!r}")

    pkg_zip = out_dir / "result-package.zip"
    form3_csv = out_dir / "form3.csv"
    records_json = out_dir / "records.json"
    for p in (pkg_zip, form3_csv, records_json):
        if not p.is_file():
            raise TestFailure(f"missing output artifact: {p}")

    with zipfile.ZipFile(pkg_zip, "r") as zf:
        names = set(zf.namelist())
    expected_members = {
        "manifest.json",
        "statement.json",
        "journal/events.json",
        "kernel/algorithms.json",
        "points/measured.json",
        "profile/spec.json",
        "results/form3.csv",
        "results/records.json",
    }
    if not expected_members.issubset(names):
        raise TestFailure(f"ZIP missing expected members: {expected_members - names}")

    with form3_csv.open("r", encoding="utf-8", newline="") as fh:
        rows = list(csv.DictReader(fh))
    if len(rows) != 4 or [r["balloon"] for r in rows] != ["1", "2", "3", "4"]:
        raise TestFailure(f"unexpected form3.csv rows: {rows!r}")


@e2e_test(
    test_id="T1-F2-02",
    tier=1,
    feature="F2",
    description="fid-slice --verify verifies generated result-package.zip with 6 members",
)
def test_slice_verify_valid_zip(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    out_dir = ctx.workdir / "gen"
    r1 = ctx.run([fid_slice, "--demo", "--out", str(out_dir), "--json"])
    if r1.returncode != 0:
        raise TestFailure(f"fid-slice --demo failed: {r1.stderr}")
    pkg_zip = out_dir / "result-package.zip"

    r2 = ctx.run([fid_slice, "--verify", str(pkg_zip), "--json"])
    if r2.returncode != 0:
        raise TestFailure(f"fid-slice --verify failed (exit {r2.returncode}): {r2.stderr}")
    env = parse_single_line_json(r2.stdout)
    assert_envelope(env, outcome="computed", command_id="slice.verify_package@1")
    result = env.get("result")
    if not isinstance(result, dict) or result.get("status") != "VERIFIED" or result.get("members_verified") != 6:
        raise TestFailure(f"unexpected verify result: {result!r}")


@e2e_test(
    test_id="T1-F2-03",
    tier=1,
    feature="F2",
    description="fid-slice --demo repeat runs emit byte-identical ZIP packages (D0)",
)
def test_slice_deterministic_repeat_run(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    d1 = ctx.workdir / "run1"
    d2 = ctx.workdir / "run2"
    r1 = ctx.run([fid_slice, "--demo", "--out", str(d1), "--json"])
    r2 = ctx.run([fid_slice, "--demo", "--out", str(d2), "--json"])
    if r1.returncode != 0 or r2.returncode != 0:
        raise TestFailure(f"fid-slice --demo failed: {r1.stderr} / {r2.stderr}")
    b1 = (d1 / "result-package.zip").read_bytes()
    b2 = (d2 / "result-package.zip").read_bytes()
    if b1 != b2:
        raise TestFailure(f"non-deterministic ZIP output: len1={len(b1)}, len2={len(b2)}")
    e1 = parse_single_line_json(r1.stdout)
    e2 = parse_single_line_json(r2.stdout)
    if e1["result"]["package_digest"] != e2["result"]["package_digest"]:  # type: ignore[index]
        raise TestFailure("package_digest differed across identical runs")


@e2e_test(
    test_id="T1-F2-04",
    tier=1,
    feature="F2",
    description="fid-slice --input <job.json> evaluates a custom conforming bracket inspection job",
)
def test_slice_custom_input_job(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    # Build synthetic conforming job JSON matching JobSpec schema.
    da: list[float] = []
    for ix in range(4):
        for iy in range(4):
            da.extend([0.010 + ix * 0.025, 0.010 + iy * 0.020, -0.000001 * ((ix + iy) % 2)])
    db: list[float] = []
    for ix in range(3):
        for iz in range(3):
            db.extend([0.015 + ix * 0.035, 0.0, -0.005 - iz * 0.010])
    dc: list[float] = []
    for iy in range(3):
        for iz in range(3):
            dc.extend([0.0, 0.015 + iy * 0.025, -0.005 - iz * 0.010])
    import math

    bore: list[float] = []
    for rz in (-0.005, -0.015, -0.025):
        for k in range(12):
            th = k * (2.0 * math.pi / 12.0)
            bore.extend([0.050 + 0.010 * math.cos(th), 0.040 + 0.010 * math.sin(th), rz])

    job = {
        "part_number": "CUSTOM-BRKT-002",
        "revision": "B",
        "serial_number": "SN-E2E-001",
        "dialect": "ASME",
        "datum_a": da,
        "datum_b": db,
        "datum_c": dc,
        "bore_points": bore,
        "bore_nominal_xy_m": [-0.050, -0.040],
        "flatness_limit_m": "0.000025",
        "diameter_lower_m": "0.019980",
        "diameter_upper_m": "0.020020",
        "position_limit_m": "0.000050",
        "perp_limit_m": "0.000030",
    }
    job_path = ctx.workdir / "custom_job.json"
    job_path.write_text(json.dumps(job), encoding="utf-8")
    out_dir = ctx.workdir / "custom_out"

    res = ctx.run([fid_slice, "--input", str(job_path), "--out", str(out_dir), "--json"])
    if res.returncode != 0:
        raise TestFailure(f"fid-slice --input exited {res.returncode}: stdout={res.stdout} stderr={res.stderr}")
    env = parse_single_line_json(res.stdout)
    assert_envelope(env, outcome="computed", command_id="slice.run_vertical@1")
    result = env.get("result")
    assert isinstance(result, dict)
    if result.get("part_number") != "CUSTOM-BRKT-002":
        raise TestFailure(f"unexpected part_number: {result.get('part_number')!r}")
    if not all(str(c.get("outcome", "")).upper() == "CONFORMS" for c in result.get("characteristics", [])):
        raise TestFailure(f"expected conforming custom job, got {result.get('characteristics')!r}")


@e2e_test(
    test_id="T1-F2-05",
    tier=1,
    feature="F2",
    description="M3/VS0 fid-slice --balloon extracts 15 callouts from vector_bracket.pdf matching truth",
)
def test_slice_vs0_pdf_balloon_text_and_propose(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    pdf_path = ctx.repo_root / "fixtures" / "rpt" / "balloon" / "auto" / "vector_bracket.pdf"
    truth_path = ctx.repo_root / "fixtures" / "rpt" / "balloon" / "auto" / "vector_bracket.truth.json"
    truth = json.loads(truth_path.read_text(encoding="utf-8"))
    out_dir = ctx.workdir / "balloon_extract"

    res = ctx.run([fid_slice, "--balloon", str(pdf_path), "--out", str(out_dir), "--json"])
    if res.returncode != 0:
        raise TestFailure(
            f"fid-slice --balloon failed (exit {res.returncode}): "
            f"stdout={res.stdout!r} stderr={res.stderr!r}"
        )
    env = parse_single_line_json(res.stdout)
    assert_envelope(env, outcome="computed")
    payload = env.get("result")
    if not isinstance(payload, dict):
        raise TestFailure(f"expected dict result from fid-slice --balloon, got {payload!r}")
    expected_count = int(truth["callout_count"])
    csv_path = out_dir / "form3-starter.csv"
    if not csv_path.is_file():
        raise TestFailure(f"expected form3-starter.csv in {out_dir}")
    with csv_path.open("r", encoding="utf-8", newline="") as fh:
        rows = list(csv.DictReader(fh))
    if len(rows) != expected_count:
        raise TestFailure(f"expected {expected_count} callout rows in form3-starter.csv, got {len(rows)}")
    for count_key in ("balloon_count", "callout_count"):
        if count_key in payload and payload[count_key] != expected_count:
            raise TestFailure(f"expected {count_key}={expected_count}, got {payload[count_key]!r}")


@e2e_test(
    test_id="T1-F2-06",
    tier=1,
    feature="F2",
    description="M3/VS0 fid-slice --balloon exports valid ballooned.pdf (%PDF-) + 15-row form3-starter.csv",
)
def test_slice_vs0_balloon_pack_export(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    pdf_path = ctx.repo_root / "fixtures" / "rpt" / "balloon" / "auto" / "vector_bracket.pdf"
    truth = json.loads(
        (ctx.repo_root / "fixtures" / "rpt" / "balloon" / "auto" / "vector_bracket.truth.json").read_text(
            encoding="utf-8"
        )
    )
    out_dir = ctx.workdir / "balloon_out"
    res = ctx.run([fid_slice, "--balloon", str(pdf_path), "--out", str(out_dir), "--json"])
    if res.returncode != 0:
        raise TestFailure(
            f"fid-slice --balloon failed (exit {res.returncode}): stdout={res.stdout!r} stderr={res.stderr!r}"
        )
    env = parse_single_line_json(res.stdout)
    assert_envelope(env, outcome="computed")
    ballooned_pdf = out_dir / "ballooned.pdf"
    form3_csv = out_dir / "form3-starter.csv"
    if not ballooned_pdf.is_file() or not form3_csv.is_file():
        raise TestFailure(f"expected ballooned.pdf and form3-starter.csv in {out_dir}")
    pdf_bytes = ballooned_pdf.read_bytes()
    if len(pdf_bytes) <= 100 or not pdf_bytes.startswith(b"%PDF-"):
        raise TestFailure(f"invalid ballooned.pdf: len={len(pdf_bytes)}, header={pdf_bytes[:16]!r}")
    with form3_csv.open("r", encoding="utf-8", newline="") as fh:
        reader = csv.DictReader(fh)
        fieldnames = reader.fieldnames or []
        rows = list(reader)
    if len(fieldnames) < 3:
        raise TestFailure(f"invalid form3-starter.csv headers: {fieldnames!r}")
    if len(rows) != truth["callout_count"]:
        raise TestFailure(f"expected {truth['callout_count']} callout rows in form3-starter.csv, got {len(rows)}")
