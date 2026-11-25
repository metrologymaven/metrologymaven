#!/usr/bin/env python3
"""Independent fixture generator for TRUST.REPLAY.KART.LOOKUP (stdlib only; CC-BY-4.0).

Uses `fixtures/pkg/make_fixtures.py` to produce genuine `fid-pkg` Result Packages:
  fixtures/replay/kart/stub-kernel.wasm            Minimal valid stub WASM module
  fixtures/replay/kart/kernel-manifest-ok.json     KRN.MAN.MANI.SCHEMA v1 manifest referencing stub-kernel.wasm
  fixtures/replay/kart/kernel-manifest-9.9.9.json  Manifest for unavailable kernel 9.9.9 (AC-4)
  fixtures/replay/kart/kernel-manifest-no-wasm.json Manifest listing no wasm32-wasip1 artifact (REQ-003)
  fixtures/replay/kart/points-with-kernel.fidpkg   Points-tier package embedding kernel/<sha256>.wasm (AC-1)
  fixtures/replay/kart/points-no-kernel.fidpkg     Points-tier package without embedded kernel (AC-2)
  fixtures/replay/kart/README.md                   Provenance and CC-BY-4.0 license

Usage:
  python3 fixtures/replay/kart/make_fixtures.py          # write files
  python3 fixtures/replay/kart/make_fixtures.py --check  # verify byte-for-byte match
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(ROOT / "fixtures" / "pkg"))
import make_fixtures as mf  # noqa: E402

# Minimal valid WebAssembly binary module (magic + version 1 + custom section "fid.stub").
STUB_WASM = (
    b"\x00asm\x01\x00\x00\x00"  # WASM header
    b"\x00\x0d"  # custom section id=0, size=13
    b"\x08fid.stub"  # section name len=8
    b"v0.1"  # payload
)


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_digest(data: bytes) -> str:
    return "sha256:" + sha256_hex(data)


def canonical_bytes(obj) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def build_kernel_manifest(semver: str, wasm_sha: str | None) -> bytes:
    artifacts = [
        {
            "role": "native",
            "sha256": sha256_digest(f"native:linux:{semver}".encode("utf-8")),
            "target": "x86_64-unknown-linux-gnu",
        },
    ]
    if wasm_sha is not None:
        artifacts.append(
            {
                "role": "wasm",
                "sha256": wasm_sha,
                "target": "wasm32-wasip1",
            }
        )
    doc = {
        "artifacts": artifacts,
        "core": {
            "algorithms": [],
            "name": "fid-kernel",
            "numeric_profile": "knp-1",
            "schema_version": "1",
            "semver": semver,
        },
        "core_digest": sha256_digest(f"core:{semver}".encode("utf-8")),
    }
    return canonical_bytes(doc)


def build_package(kernel_manifest: bytes, embedded_wasm: bytes | None) -> bytes:
    km_digest = sha256_digest(kernel_manifest)
    members = [
        {
            "path": "kernel/manifest.json",
            "media_type": "application/json",
            "body": kernel_manifest.decode("utf-8"),
        },
        {"path": "profile/profile.json", "media_type": "application/json", "body": mf.PROFILE_BODY},
        {"path": "results/records.json", "media_type": "application/json", "body": "[]"},
    ]
    if embedded_wasm is not None:
        wasm_str = embedded_wasm.decode("ascii")
        assert wasm_str.encode("utf-8") == embedded_wasm
        wasm_hex = sha256_hex(embedded_wasm)
        members.append(
            {
                "path": f"kernel/{wasm_hex}.wasm",
                "media_type": "application/wasm",
                "body": wasm_str,
            }
        )
    contents = {
        "created_by": "fid-vreplay kart lookup fixture generator 1",
        "evidence_tier": "points",
        "profile_digest": mf.digest_of(mf.PROFILE_BODY),
        "kernel_manifest_digest": km_digest,
        "result_count": 0,
        "members": members,
    }
    _, _, zip_bytes = mf.build_package(contents)
    return zip_bytes


def build_all():
    wasm_sha = sha256_digest(STUB_WASM)
    km_ok = build_kernel_manifest("0.1.0", wasm_sha)
    km_999 = build_kernel_manifest("9.9.9", sha256_digest(b"nonexistent-kernel-9.9.9"))
    km_no_wasm = build_kernel_manifest("0.2.0", None)

    pkg_with = build_package(km_ok, STUB_WASM)
    pkg_without = build_package(km_ok, None)

    readme = (
        "# TRUST.REPLAY.KART.LOOKUP test fixtures\n"
        "\n"
        "- **Spec node:** `TRUST.REPLAY.KART.LOOKUP` (`fid-vreplay::kart`)\n"
        "- **License:** CC-BY-4.0 (synthetic stub WASM module and `fid-pkg` test containers)\n"
        "- **Generator:** `python3 fixtures/replay/kart/make_fixtures.py` (verify with `--check`)\n"
        "\n"
        "## Files\n"
        "\n"
        "- `stub-kernel.wasm`: minimal valid 23-byte WebAssembly stub module.\n"
        "- `kernel-manifest-ok.json`: `KRN.MAN.MANI.SCHEMA` v1 manifest (`semver = 0.1.0`) referencing `stub-kernel.wasm`.\n"
        "- `kernel-manifest-9.9.9.json`: manifest for unavailable kernel `9.9.9` (AC-4).\n"
        "- `kernel-manifest-no-wasm.json`: manifest listing no `wasm32-wasip1` artifact (REQ-003).\n"
        "- `points-with-kernel.fidpkg`: genuine `fid-pkg` container embedding `kernel/<sha256>.wasm` (AC-1).\n"
        "- `points-no-kernel.fidpkg`: genuine `fid-pkg` container without embedded kernel (AC-2).\n"
    ).encode("utf-8")

    return {
        HERE / "stub-kernel.wasm": STUB_WASM,
        HERE / "kernel-manifest-ok.json": km_ok,
        HERE / "kernel-manifest-9.9.9.json": km_999,
        HERE / "kernel-manifest-no-wasm.json": km_no_wasm,
        HERE / "points-with-kernel.fidpkg": pkg_with,
        HERE / "points-no-kernel.fidpkg": pkg_without,
        HERE / "README.md": readme,
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = build_all()
    if check:
        ok = True
        for path, expected in files.items():
            if not path.exists():
                print(f"MISSING: {path}", file=sys.stderr)
                ok = False
                continue
            if path.read_bytes() != expected:
                print(f"MISMATCH: {path}", file=sys.stderr)
                ok = False
        return 0 if ok else 1
    HERE.mkdir(parents=True, exist_ok=True)
    for path, content in files.items():
        path.write_bytes(content)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
