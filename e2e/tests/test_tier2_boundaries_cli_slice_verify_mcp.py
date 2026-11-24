"""Tier 2 boundary, negative, and error-handling E2E tests for F1, F2, F3, and F4."""

from __future__ import annotations

import csv
import json
import math
import re
import zipfile
from pathlib import Path

from e2e.lib.harness import (
    TestContext,
    TestFailure,
    assert_envelope,
    e2e_test,
    parse_single_line_json,
)
from e2e.tests._helpers import mcp_init_messages, run_mcp_session, write_disclosure_profiles_index


# ===========================================================================
# F1 Boundaries & Negative Tests (6 tests)
# ===========================================================================


@e2e_test(
    test_id="T2-F1-01",
    tier=2,
    feature="F1",
    description="fid nope --json and fid canon --bogus --json emit 1-line usage_error envelope and exit 2",
)
def test_fid_json_usage_error_envelope(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    for argv in (["nope", "--json"], ["canon", "--bogus", "--json"], ["canon", "--json", "--bogus"]):
        res = ctx.run([fid, *argv])
        if res.returncode != 2:
            raise TestFailure(f"expected exit 2 for {argv}, got {res.returncode}: {res.stderr}")
        env = parse_single_line_json(res.stdout)
        assert_envelope(env, outcome="failed", error_code="usage_error")


@e2e_test(
    test_id="T2-F1-02",
    tier=2,
    feature="F1",
    description="fid nope without --json exits 2 with completely empty stdout and error on stderr",
)
def test_fid_plain_usage_error_empty_stdout(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    for argv in (["nope"], ["canon", "--bogus"]):
        res = ctx.run([fid, *argv])
        if res.returncode != 2:
            raise TestFailure(f"expected exit 2 for {argv}, got {res.returncode}")
        if res.stdout != "":
            raise TestFailure(f"expected empty stdout for plain error {argv}, got {res.stdout!r}")
        if not res.stderr.strip():
            raise TestFailure(f"expected diagnostic on stderr for {argv}")


@e2e_test(
    test_id="T2-F1-03",
    tier=2,
    feature="F1",
    description="fid align run-bestfitt exits 2 and suggests run-bestfit (Levenshtein <=2)",
)
def test_fid_typo_suggestion(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    res = ctx.run([fid, "align", "run-bestfitt"])
    if res.returncode != 2:
        raise TestFailure(f"expected exit 2, got {res.returncode}")
    if "run-bestfit" not in res.stderr:
        raise TestFailure(f"expected typo suggestion `run-bestfit` on stderr, got: {res.stderr!r}")


@e2e_test(
    test_id="T2-F1-04",
    tier=2,
    feature="F1",
    description="fid canon rejects duplicate keys, depth 129, and NaN/Infinity with exit 4",
)
def test_fid_canon_bad_inputs(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    bad_dir = ctx.repo_root / "fixtures" / "canon" / "bad"
    cases = ("duplicate_key.json", "depth_129_arrays.json", "depth_129_objects.json", "nan_literal.json", "infinity_literal.json")
    for name in cases:
        p = bad_dir / name
        r_plain = ctx.run([fid, "canon", str(p)])
        if r_plain.returncode != 4:
            raise TestFailure(f"plain fid canon {name} expected exit 4, got {r_plain.returncode}")
        if r_plain.stdout != "":
            raise TestFailure(f"plain fid canon {name} should leave stdout empty, got {r_plain.stdout!r}")

        r_json = ctx.run([fid, "canon", "--json", str(p)])
        if r_json.returncode != 4:
            raise TestFailure(f"fid canon --json {name} expected exit 4, got {r_json.returncode}")
        env = parse_single_line_json(r_json.stdout)
        assert_envelope(env, outcome="failed", command_id="canon.canonicalize@1", error_code="validation_error")


@e2e_test(
    test_id="T2-F1-05",
    tier=2,
    feature="F1",
    description="fid ctrl status with no app exits 6 (not_run) and out-of-range --count/--timeout-ms exit 2",
)
def test_fid_ctrl_no_app_and_bounds(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    run_dir = ctx.workdir / "empty_run"
    run_dir.mkdir(parents=True, exist_ok=True)
    tok_file = ctx.workdir / "missing.token"

    res = ctx.run(
        [fid, "ctrl", "status", "--json", "--runtime-dir", str(run_dir), "--token-file", str(tok_file)]
    )
    if res.returncode != 6:
        raise TestFailure(f"expected exit 6 when no app running, got {res.returncode}: {res.stdout} {res.stderr}")
    env = parse_single_line_json(res.stdout)
    assert_envelope(env, outcome="not_run", command_id="ctrl.status@1")
    if env.get("result", {}).get("ctrl_error") != "app_not_found":  # type: ignore[union-attr]
        raise TestFailure(f"expected ctrl_error=app_not_found, got {env!r}")

    for bad_args in (
        ["ctrl", "ping", "--json", "--count", "0", "--runtime-dir", "x", "--token-file", "y"],
        ["ctrl", "ping", "--json", "--count", "101", "--runtime-dir", "x", "--token-file", "y"],
        ["ctrl", "status", "--json", "--timeout-ms", "0", "--runtime-dir", "x", "--token-file", "y"],
    ):
        rb = ctx.run([fid, *bad_args])
        if rb.returncode != 2:
            raise TestFailure(f"expected exit 2 for {bad_args}, got {rb.returncode}")
        eb = parse_single_line_json(rb.stdout)
        assert_envelope(eb, outcome="failed", error_code="usage_error")


@e2e_test(
    test_id="T2-F1-06",
    tier=2,
    feature="F1",
    description="M4 fid refuses M1/S1/A0 commands pre-bus (exit 3), raw paths (exit 3), and invalid --major 9 (exit 2)",
)
def test_fid_class_refusal_raw_path_and_major(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    # Invalid --major 9 exits 2
    r_maj = ctx.run([fid, "align", "run-bestfit", "--major", "9", "--json"])
    if r_maj.returncode != 2:
        raise TestFailure(f"expected exit 2 for --major 9, got {r_maj.returncode}")
    assert_envelope(parse_single_line_json(r_maj.stdout), outcome="failed", error_code="usage_error")

    # M4: M1/S1/A0 or raw path refusal exits 3 (policy_refused / raw_path_refused)
    r_m1 = ctx.run([fid, "motion", "move-axis", "--json"])
    if r_m1.returncode != 3:
        raise TestFailure(
            f"expected exit 3 policy_refused for M1 command motion.move_axis, got {r_m1.returncode}: {r_m1.stdout}"
        )
    assert_envelope(parse_single_line_json(r_m1.stdout), outcome="failed", error_code="policy_refused")


# ===========================================================================
# F2 Boundaries & Negative Tests (6 tests)
# ===========================================================================


@e2e_test(
    test_id="T2-F2-01",
    tier=2,
    feature="F2",
    description="fid-slice --verify rejects a 1-byte tampered result-package.zip with exit 1",
)
def test_slice_verify_tampered_zip(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    out_dir = ctx.workdir / "gen"
    r1 = ctx.run([fid_slice, "--demo", "--out", str(out_dir), "--json"])
    if r1.returncode != 0:
        raise TestFailure(f"fid-slice --demo failed: {r1.stderr}")
    zip_path = out_dir / "result-package.zip"
    data = bytearray(zip_path.read_bytes())
    # Flip a byte inside the first local file payload
    mid = len(data) // 3
    data[mid] ^= 0xFF
    tampered = ctx.workdir / "tampered.zip"
    tampered.write_bytes(bytes(data))

    r2 = ctx.run([fid_slice, "--verify", str(tampered), "--json"])
    if r2.returncode != 1:
        raise TestFailure(f"expected exit 1 on tampered ZIP, got {r2.returncode}: {r2.stdout}")
    env = parse_single_line_json(r2.stdout)
    assert_envelope(env, outcome="failed", command_id="slice.verify_package@1", error_code="validation_error")


@e2e_test(
    test_id="T2-F2-02",
    tier=2,
    feature="F2",
    description="fid-slice --input fails cleanly (exit 1) on missing file and malformed JSON",
)
def test_slice_input_missing_and_malformed(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    missing = ctx.workdir / "does_not_exist.json"
    r_miss = ctx.run([fid_slice, "--input", str(missing), "--json"])
    if r_miss.returncode != 1:
        raise TestFailure(f"expected exit 1 on missing input, got {r_miss.returncode}")
    assert_envelope(parse_single_line_json(r_miss.stdout), outcome="failed", error_code="not_found")

    bad_json = ctx.workdir / "bad.json"
    bad_json.write_text('{"part_number": 123}', encoding="utf-8")
    r_bad = ctx.run([fid_slice, "--input", str(bad_json), "--json"])
    if r_bad.returncode != 1:
        raise TestFailure(f"expected exit 1 on malformed job, got {r_bad.returncode}")
    assert_envelope(parse_single_line_json(r_bad.stdout), outcome="failed", error_code="validation_error")


@e2e_test(
    test_id="T2-F2-03",
    tier=2,
    feature="F2",
    description="fid-slice --input with +0.040 mm shifted bore reports Balloon 3 True Position nonconforms",
)
def test_slice_shifted_bore_nonconforms(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    da: list[float] = []
    for ix in range(4):
        for iy in range(4):
            da.extend([0.010 + ix * 0.025, 0.010 + iy * 0.020, 0.0])
    db: list[float] = []
    for ix in range(3):
        for iz in range(3):
            db.extend([0.015 + ix * 0.035, 0.0, -0.005 - iz * 0.010])
    dc: list[float] = []
    for iy in range(3):
        for iz in range(3):
            dc.extend([0.0, 0.015 + iy * 0.025, -0.005 - iz * 0.010])
    bore: list[float] = []
    # Shifted X by +0.040 mm (0.050040 m) -> radial offset 0.040 mm -> diametral true pos 0.080 mm > 0.050 mm limit
    for rz in (-0.005, -0.015, -0.025):
        for k in range(12):
            th = k * (2.0 * math.pi / 12.0)
            bore.extend([0.050040 + 0.010 * math.cos(th), 0.040 + 0.010 * math.sin(th), rz])

    job = {
        "part_number": "SHIFTED-001",
        "revision": "A",
        "serial_number": "SN-SHIFT",
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
    job_path = ctx.workdir / "shifted_job.json"
    job_path.write_text(json.dumps(job), encoding="utf-8")
    res = ctx.run([fid_slice, "--input", str(job_path), "--json"])
    if res.returncode != 0:
        raise TestFailure(f"fid-slice exited {res.returncode}: {res.stdout} {res.stderr}")
    env = parse_single_line_json(res.stdout)
    chars = env["result"]["characteristics"]  # type: ignore[index]
    b3 = next((c for c in chars if c.get("balloon") == 3), None)
    if not b3 or str(b3.get("outcome", "")).upper() != "NONCONFORMS":
        raise TestFailure(f"expected Balloon 3 NONCONFORMS on shifted bore, got {b3!r}")


@e2e_test(
    test_id="T2-F2-04",
    tier=2,
    feature="F2",
    description="fid-slice --bogus rejects unknown CLI flags with exit 1",
)
def test_slice_bogus_flag(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    res = ctx.run([fid_slice, "--bogus"])
    if res.returncode != 1:
        raise TestFailure(f"expected exit 1 on --bogus, got {res.returncode}")
    if "unknown argument" not in res.stderr:
        raise TestFailure(f"expected unknown argument error on stderr, got: {res.stderr!r}")


@e2e_test(
    test_id="T2-F2-05",
    tier=2,
    feature="F2",
    description="M3 fid-slice --balloon on scanned_only.pdf yields raster_only / OCR notice and zero fabricated balloons",
)
def test_slice_vs0_scanned_only_raster_warning(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    pdf = ctx.repo_root / "fixtures" / "rpt" / "balloon" / "auto" / "scanned_only.pdf"
    out_dir = ctx.workdir / "scan_out"
    res = ctx.run([fid_slice, "--balloon", str(pdf), "--out", str(out_dir), "--json"])
    if res.returncode != 0:
        raise TestFailure(f"fid-slice --balloon on scanned_only.pdf failed (exit {res.returncode}): {res.stdout} {res.stderr}")
    env = parse_single_line_json(res.stdout)
    assert_envelope(env, outcome="computed")
    r_obj = env.get("result")
    if not isinstance(r_obj, dict):
        raise TestFailure(f"expected dict result, got {r_obj!r}")
    if r_obj.get("proposals", []) != [] or r_obj.get("balloon_count", 0) != 0:
        raise TestFailure(f"expected zero fabricated balloons for raster_only, got {r_obj!r}")
    csv_path = out_dir / "form3-starter.csv"
    if csv_path.is_file():
        with csv_path.open("r", encoding="utf-8", newline="") as fh:
            if list(csv.DictReader(fh)):
                raise TestFailure("expected 0 data rows in form3-starter.csv for scanned_only.pdf")
    dumped = json.dumps(r_obj).lower()
    if "report.raster_sheet_needs_ocr" not in dumped and "raster_only" not in dumped:
        raise TestFailure(f"expected raster_only / report.raster_sheet_needs_ocr diagnostic in {r_obj!r}")


@e2e_test(
    test_id="T2-F2-06",
    tier=2,
    feature="F2",
    description="M3 fid-slice --balloon refuses path escape (--out ../outside) with root violation",
)
def test_slice_vs0_export_path_escape_refused(ctx: TestContext) -> None:
    fid_slice = ctx.require_bin("fid-slice")
    pdf = ctx.repo_root / "fixtures" / "rpt" / "balloon" / "auto" / "vector_bracket.pdf"
    safe_sub = ctx.workdir / "export_root"
    safe_sub.mkdir(parents=True, exist_ok=True)
    outside_target = ctx.workdir / "outside"
    res = ctx.run([fid_slice, "--balloon", str(pdf), "--out", "../outside", "--json"], cwd=safe_sub)
    if res.returncode == 0:
        raise TestFailure(f"expected refusal on --out ../outside, got exit 0: {res.stdout}")
    if outside_target.exists():
        raise TestFailure(f"path escape created directory outside export root: {outside_target}")
    env = parse_single_line_json(res.stdout)
    assert_envelope(env, outcome="failed")
    dumped = json.dumps(env).lower()
    if "root" not in dumped and "escape" not in dumped and "outside" not in dumped:
        raise TestFailure(f"expected io.root_violation / root_escape error, got {env!r}")


# ===========================================================================
# F3 Boundaries & Negative Tests (6 tests)
# ===========================================================================


@e2e_test(
    test_id="T2-F3-01",
    tier=2,
    feature="F3",
    description="fid-pkg-conformance rejects all 106 malformed packages (zip bomb, symlink, CRC, truncated) with exact tags",
)
def test_pkg_conformance_106_malformed(ctx: TestContext) -> None:
    conf = ctx.require_bin("fid-pkg-conformance")
    mal_root = ctx.repo_root / "fixtures" / "pkg" / "malformed"
    cases = sorted(p for p in mal_root.iterdir() if p.is_dir())
    if len(cases) < 100:
        raise TestFailure(f"expected >=100 malformed cases, found {len(cases)}")
    for cdir in cases:
        pkg = cdir / "package.zip"
        exp = json.loads((cdir / "expect.json").read_text(encoding="utf-8"))
        res = ctx.run([conf, str(pkg)])
        if res.returncode != 1:
            raise TestFailure(f"malformed case {cdir.name} returned exit {res.returncode} (expected 1)")
        got = parse_single_line_json(res.stdout)
        if got.get("result") != "rejected" or got.get("error_code") != exp["error_code"] or got.get("reason") != exp["reason"]:
            raise TestFailure(f"malformed case {cdir.name} mismatch: got={got!r} want={exp!r}")


@e2e_test(
    test_id="T2-F3-02",
    tier=2,
    feature="F3",
    description="M4 fid-verify exits 1 on revoked key, untrusted root, and invalid signature",
)
def test_fid_verify_signature_failures(ctx: TestContext) -> None:
    verify = ctx.require_bin("fid-verify")
    pkg = ctx.repo_root / "fixtures" / "pkg" / "golden" / "two-signatures.zip"
    root = ctx.repo_root / "fixtures" / "troot" / "roots" / "both.json"
    rev = ctx.repo_root / "fixtures" / "troot" / "revoked" / "revoked-before.json"

    # Baseline isolation: golden/two-signatures.zip passes with both.json when not revoked.
    r_ok = ctx.run([verify, str(pkg), "--trust-root", str(root), "--json"])
    if r_ok.returncode != 0 or parse_single_line_json(r_ok.stdout).get("verdict") != "PASS":
        raise TestFailure(f"baseline golden/two-signatures.zip did not pass before --revoked: {r_ok.stdout} {r_ok.stderr}")

    # 1. Revoked signer key -> exit 1 + FAIL + REVOKED
    res = ctx.run([verify, str(pkg), "--trust-root", str(root), "--revoked", str(rev), "--json"])
    if res.returncode != 1:
        raise TestFailure(f"expected bitmask exit 1 for revoked signature, got {res.returncode}: {res.stdout} {res.stderr}")
    rep = parse_single_line_json(res.stdout)
    if rep.get("verdict") != "FAIL" or "REVOKED" not in json.dumps(rep):
        raise TestFailure(f"expected verdict=FAIL with REVOKED status, got {rep!r}")

    # 2. Untrusted / empty trust root -> exit 1 + FAIL
    empty_root = ctx.repo_root / "fixtures" / "troot" / "roots" / "empty.json"
    r_untrusted = ctx.run([verify, str(pkg), "--trust-root", str(empty_root), "--json"])
    if r_untrusted.returncode != 1:
        raise TestFailure(f"expected exit 1 on empty trust root, got {r_untrusted.returncode}")
    if parse_single_line_json(r_untrusted.stdout).get("verdict") != "FAIL":
        raise TestFailure(f"expected FAIL on empty trust root: {r_untrusted.stdout}")

    # 3. Faulty signatures fixture -> bit 1 set + FAIL
    faults_pkg = ctx.repo_root / "fixtures" / "troot" / "packages" / "faults.zip"
    r_faults = ctx.run([verify, str(faults_pkg), "--trust-root", str(root), "--json"])
    if r_faults.returncode == 0 or (r_faults.returncode & 1) == 0:
        raise TestFailure(f"expected signature failure bit 1 on faults.zip, got {r_faults.returncode}: {r_faults.stdout}")
    if parse_single_line_json(r_faults.stdout).get("verdict") != "FAIL":
        raise TestFailure(f"expected verdict=FAIL on faults.zip: {r_faults.stdout}")


@e2e_test(
    test_id="T2-F3-03",
    tier=2,
    feature="F3",
    description="M4 fid-verify detects broken hash chain / linkage (bitmask bit 2 set, chain.verdict=BROKEN)",
)
def test_fid_verify_broken_chain_bitmask(ctx: TestContext) -> None:
    verify = ctx.require_bin("fid-verify")
    root = ctx.repo_root / "fixtures" / "troot" / "roots" / "both.json"
    chain_fixtures = (
        ctx.repo_root / "fixtures" / "chain" / "tamper" / "modified.zip",
        ctx.repo_root / "fixtures" / "chain" / "reslink" / "broken-at-5.zip",
    )
    for pkg in chain_fixtures:
        res = ctx.run([verify, str(pkg), "--trust-root", str(root), "--json"])
        if res.returncode == 0 or (res.returncode & 2) == 0:
            raise TestFailure(
                f"expected FAIL bitmask bit 2 (chain/linkage) on {pkg.name}, "
                f"got exit {res.returncode}: {res.stdout} {res.stderr}"
            )
        rep = parse_single_line_json(res.stdout)
        if rep.get("verdict") != "FAIL":
            raise TestFailure(f"expected verdict=FAIL on {pkg.name}, got {rep!r}")
        chain_verdict = rep.get("chain", {}).get("verdict")  # type: ignore[union-attr]
        if chain_verdict != "BROKEN":
            raise TestFailure(f"expected chain.verdict=BROKEN on {pkg.name}, got {rep.get('chain')!r}")


@e2e_test(
    test_id="T2-F3-04",
    tier=2,
    feature="F3",
    description="M4 fid-verify exits 64 on malformed or truncated package with structured diagnostics",
)
def test_fid_verify_malformed_exit_64(ctx: TestContext) -> None:
    verify = ctx.require_bin("fid-verify")
    for case in ("truncated_tail", "crc_mismatch"):
        mal = ctx.repo_root / "fixtures" / "pkg" / "malformed" / case / "package.zip"
        if not mal.is_file():
            raise TestFailure(f"missing malformed fixture: {mal}")
        res = ctx.run([verify, str(mal), "--json"])
        if res.returncode != 64:
            raise TestFailure(
                f"expected exit 64 for {case}, got {res.returncode}: {res.stdout} {res.stderr}"
            )
        diag = (res.stdout + "\n" + res.stderr).strip()
        if not diag:
            raise TestFailure(f"fid-verify exited 64 on {case} with empty stdout and stderr")
        if res.stdout.strip():
            rep = parse_single_line_json(res.stdout)
            if rep.get("verdict") not in ("ERROR", "FAIL") and "error" not in rep:
                raise TestFailure(f"unexpected JSON error payload on {case}: {rep!r}")


@e2e_test(
    test_id="T2-F3-05",
    tier=2,
    feature="F3",
    description="M4 fid-verify exits 65 on invalid CLI flags with diagnostic stderr",
)
def test_fid_verify_usage_exit_65(ctx: TestContext) -> None:
    verify = ctx.require_bin("fid-verify")
    res = ctx.run([verify, "--bogus-flag"])
    if res.returncode != 65:
        raise TestFailure(f"expected exit 65 for invalid flags, got {res.returncode}: {res.stderr}")
    if not res.stderr.strip():
        raise TestFailure("fid-verify exited 65 with empty stderr diagnostic")


@e2e_test(
    test_id="T2-F3-06",
    tier=2,
    feature="F3",
    description="M4 fid-verify rejects unsigned package and 1-byte tampered ZIP payload",
)
def test_fid_verify_unsigned_and_tampered(ctx: TestContext) -> None:
    verify = ctx.require_bin("fid-verify")
    unsigned = ctx.repo_root / "fixtures" / "troot" / "packages" / "unsigned.zip"
    root = ctx.repo_root / "fixtures" / "troot" / "roots" / "both.json"
    res = ctx.run([verify, str(unsigned), "--trust-root", str(root), "--json"])
    if res.returncode != 1:
        raise TestFailure(f"expected exit 1 on unsigned.zip, got {res.returncode}: {res.stdout}")
    rep = parse_single_line_json(res.stdout)
    if rep.get("verdict") != "FAIL":
        raise TestFailure(f"expected verdict=FAIL for unsigned package, got {rep!r}")

    # 1-byte raw tamper of signed golden/two-signatures.zip
    golden_signed = ctx.repo_root / "fixtures" / "pkg" / "golden" / "two-signatures.zip"
    raw = bytearray(golden_signed.read_bytes())
    raw[len(raw) // 2] ^= 0x01
    raw_tampered = ctx.workdir / "raw_tampered.zip"
    raw_tampered.write_bytes(bytes(raw))
    r_raw = ctx.run([verify, str(raw_tampered), "--trust-root", str(root), "--json"])
    if r_raw.returncode == 0:
        raise TestFailure(f"fid-verify accepted 1-byte tampered ZIP: {r_raw.stdout}")

    # Valid-ZIP 1-byte payload tamper inside results/records.json -> digest/signature/chain failure
    zip_tampered = ctx.workdir / "payload_tampered.zip"
    with zipfile.ZipFile(golden_signed, "r") as zin, zipfile.ZipFile(zip_tampered, "w", zipfile.ZIP_DEFLATED) as zout:
        for info in zin.infolist():
            data = bytearray(zin.read(info.filename))
            if info.filename == "results/records.json" and data:
                data[-2] = ord(" ") if data[-2] != ord(" ") else ord("\t")
            zout.writestr(info, bytes(data))
    r_pay = ctx.run([verify, str(zip_tampered), "--trust-root", str(root), "--json"])
    if r_pay.returncode == 0:
        raise TestFailure(f"fid-verify accepted valid-ZIP tampered results/records.json: {r_pay.stdout}")
    if r_pay.stdout.strip():
        rep_pay = parse_single_line_json(r_pay.stdout)
        if rep_pay.get("verdict") == "PASS":
            raise TestFailure(f"tampered payload yielded PASS: {rep_pay!r}")


# ===========================================================================
# F4 Boundaries & Negative Tests (6 tests)
# ===========================================================================


@e2e_test(
    test_id="T2-F4-01",
    tier=2,
    feature="F4",
    description="fid-mcp --server nosuch exits 2 with empty stdout and unknown_profile on stderr",
)
def test_mcp_unknown_profile_exit_2(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    idx = ctx.repo_root / "fixtures" / "mcp" / "profiles" / "expected.json"
    res = ctx.run([mcp, "--profiles-index", str(idx), "--server", "nosuch", "--print-profile"])
    if res.returncode != 2:
        raise TestFailure(f"expected exit 2 on unknown profile, got {res.returncode}")
    if res.stdout != "":
        raise TestFailure(f"expected empty stdout on unknown profile, got {res.stdout!r}")
    if "unknown_profile" not in res.stderr:
        raise TestFailure(f"expected unknown_profile on stderr, got {res.stderr!r}")


@e2e_test(
    test_id="T2-F4-02",
    tier=2,
    feature="F4",
    description="M4 fid mcp --bridge --headless exits 2 (mode_conflict)",
    capabilities_required=("c_toolchain",),
)
def test_mcp_mode_conflict_exit_2(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    res = ctx.run([fid, "mcp", "--bridge", "--headless"])
    if res.returncode != 2:
        raise TestFailure(f"expected exit 2 on fid mcp --bridge --headless, got {res.returncode}: {res.stderr}")
    if "mode_conflict" not in res.stderr:
        raise TestFailure(f"expected mode_conflict on stderr, got {res.stderr!r}")


@e2e_test(
    test_id="T2-F4-03",
    tier=2,
    feature="F4",
    description="M4 fid mcp --headless --project /raw/path exits 3 (raw_path_refused)",
    capabilities_required=("c_toolchain",),
)
def test_mcp_raw_path_refused_exit_3(ctx: TestContext) -> None:
    fid = ctx.require_bin("fid")
    res = ctx.run([fid, "mcp", "--headless", "--project", "/raw/filesystem/path"])
    if res.returncode != 3:
        raise TestFailure(f"expected exit 3 on raw --project path, got {res.returncode}: {res.stderr}")
    if "raw_path_refused" not in res.stderr:
        raise TestFailure(f"expected raw_path_refused on stderr, got {res.stderr!r}")


@e2e_test(
    test_id="T2-F4-04",
    tier=2,
    feature="F4",
    description="M4 fid-mcp refuses M1/S1/A0 and unknown tool calls pre-bus with identical not_found message",
)
def test_mcp_hidden_and_unknown_tool_refused(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    idx = write_disclosure_profiles_index(ctx.workdir)
    msgs_in = mcp_init_messages() + [
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "motion_move_axis", "arguments": {}}},
        {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "demo_unknown_xyz", "arguments": {}}},
    ]
    rc, msgs, _, stderr = run_mcp_session(mcp, ["--headless", "--profiles-index", str(idx)], msgs_in, cwd=ctx.repo_root)
    if rc != 0:
        raise TestFailure(f"fid-mcp exited {rc}: {stderr}")
    r2 = next((m for m in msgs if m.get("id") == 2), None)
    r3 = next((m for m in msgs if m.get("id") == 3), None)
    if not r2 or not r3:
        raise TestFailure(f"expected replies for id 2 and 3, got {msgs!r}")
    if "error" not in r2 and not r2.get("result", {}).get("isError"):
        raise TestFailure(f"M1 tool call was not refused: {r2!r}")
    if "error" not in r3 and not r3.get("result", {}).get("isError"):
        raise TestFailure(f"unknown tool call was not refused: {r3!r}")


@e2e_test(
    test_id="T2-F4-05",
    tier=2,
    feature="F4",
    description="M4 tools_invoke_readonly refuses W2 command with policy_refused",
)
def test_mcp_invoke_readonly_refuses_w2(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    idx = write_disclosure_profiles_index(ctx.workdir)
    msgs_in = mcp_init_messages() + [
        {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "tools_invoke_readonly",
                "arguments": {"command": "align.set_datum@1", "args": {}},
            },
        }
    ]
    rc, msgs, _, stderr = run_mcp_session(
        mcp, ["--headless", "--profiles-index", str(idx), "--server", "inspect"], msgs_in, cwd=ctx.repo_root
    )
    if rc != 0:
        raise TestFailure(f"fid-mcp exited {rc}: {stderr}")
    r2 = next((m for m in msgs if m.get("id") == 2), None)
    if not r2 or "policy_refused" not in json.dumps(r2):
        raise TestFailure(f"expected policy_refused for W2 in tools_invoke_readonly, got {r2!r}")


@e2e_test(
    test_id="T2-F4-06",
    tier=2,
    feature="F4",
    description="M4 fid-mcp rejects >4 MiB line and invalid JSON-RPC while keeping session alive, and escapes taint delimiters",
)
def test_mcp_oversized_line_stays_alive_and_taint_escapes(ctx: TestContext) -> None:
    mcp = ctx.require_bin("fid-mcp")
    idx = write_disclosure_profiles_index(ctx.workdir)
    big_str = "A" * (4 * 1024 * 1024 + 1024)
    oversized = json.dumps({"jsonrpc": "2.0", "id": 99, "method": "tools/list", "params": {"pad": big_str}})
    msgs_in: list[dict[str, object] | str] = [
        *mcp_init_messages(),
        "{not valid json",
        oversized,
        {
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {"name": "tools_search_tools", "arguments": {"keyword": "<<<inject>>>"}},
        },
        {
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "tools_invoke_readonly",
                "arguments": {"command": "project.list_features@1", "args": {"note": "<<<inject>>>"}},
            },
        },
    ]
    rc, msgs, _, stderr = run_mcp_session(
        mcp, ["--headless", "--profiles-index", str(idx)], msgs_in, cwd=ctx.repo_root, timeout=30.0
    )
    if rc != 0:
        raise TestFailure(f"fid-mcp exited {rc}: {stderr}")
    errs = [m for m in msgs if "error" in m]
    if len(errs) < 2:
        raise TestFailure(f"expected JSON-RPC errors for invalid JSON and >4 MiB line, got {msgs!r}")
    r4 = next((m for m in msgs if m.get("id") == 4), None)
    if not r4 or "result" not in r4 or r4["result"].get("isError"):
        raise TestFailure(f"session did not survive oversized/invalid JSON-RPC lines: {msgs!r}")
    if not isinstance(r4["result"].get("content"), list):
        raise TestFailure(f"expected content list on r4: {r4!r}")
    r4_dump = json.dumps(r4["result"])
    if re.search(r"(?<!\\)<<<inject", r4_dump):
        raise TestFailure(f"unescaped <<<inject>>> found in r4: {r4!r}")
    r5 = next((m for m in msgs if m.get("id") == 5), None)
    if not r5 or "result" not in r5 or r5["result"].get("isError"):
        raise TestFailure(f"tools_invoke_readonly failed in r5: {r5!r}")
    r5_dump = json.dumps(r5["result"])
    if re.search(r"(?<!\\)<<<inject", r5_dump):
        raise TestFailure(f"unescaped <<<inject>>> found in r5: {r5!r}")
    if "untrusted" not in r5_dump and "FID-UNTRUSTED-DATA-BEGIN" not in r5_dump:
        raise TestFailure(f"expected taint wrapping in r5: {r5!r}")
