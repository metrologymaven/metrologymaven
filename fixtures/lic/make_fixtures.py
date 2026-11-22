#!/usr/bin/env python3
"""Independent stdlib-Python fixture generator for ECO.CI.LIC (AC-1..AC-6).

Generates synthetic `cargo metadata --format-version 1`-shaped JSON files, allowlist TOMLs,
clarification TOMLs (pinned by SHA-256 of a short MIT licence text), and small asset trees
under `fixtures/lic/`.

Usage: python3 fixtures/lic/make_fixtures.py [--check]
Licence: MIT OR Apache-2.0.
"""

import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent

MIT_LICENCE_TEXT = (
    "MIT License\n"
    "\n"
    "Copyright (c) 2026 Synthetic Fixture Authors\n"
    "\n"
    "Permission is hereby granted, free of charge, to any person obtaining a copy\n"
    "of this software to deal in the Software without restriction.\n"
)


def make_metadata(packages: list[dict], workspace_members: list[str] | None = None) -> bytes:
    doc = {
        "packages": packages,
        "workspace_members": workspace_members if workspace_members is not None else ["fid-demo 0.1.0 (path+file:///ws)"],
        "version": 1,
    }
    return (json.dumps(doc, sort_keys=True, indent=2) + "\n").encode("utf-8")


def pkg(
    name: str,
    version: str,
    license_expr: str | None,
    *,
    source: str | None = "registry+https://github.com/rust-lang/crates.io-index",
    license_file: str | None = None,
    pkg_id: str | None = None,
) -> dict:
    return {
        "id": pkg_id or f"{name} {version} ({source or 'path+file:///ws'})",
        "name": name,
        "version": version,
        "license": license_expr,
        "license_file": license_file,
        "source": source,
    }


def build_fixtures() -> dict[Path, bytes]:
    mit_bytes = MIT_LICENCE_TEXT.encode("utf-8")
    mit_sha = "sha256:" + hashlib.sha256(mit_bytes).hexdigest()
    stale_sha = "sha256:" + ("0" * 64)

    ws_pkg = pkg(
        "fid-demo",
        "0.1.0",
        "MIT OR Apache-2.0",
        source=None,
        pkg_id="fid-demo 0.1.0 (path+file:///ws)",
    )

    allow_toml = (
        "# Synthetic allowlist for ECO.CI.LIC fixtures (AC-1..AC-6).\n"
        "licences = [\n"
        '    "MIT",\n'
        '    "Apache-2.0",\n'
        '    "BSD-3-Clause",\n'
        "]\n"
        "\n"
        "exceptions = []\n"
        "\n"
        "asset_licences = [\n"
        '    "CC-BY-4.0",\n'
        '    "CC0-1.0",\n'
        '    "MIT",\n'
        '    "Apache-2.0",\n'
        "]\n"
        "\n"
        "asset_paths = [\n"
        '    "assets/**",\n'
        "]\n"
    ).encode("utf-8")

    allow_with_llvm_toml = (
        "# Synthetic allowlist with LLVM-exception enabled.\n"
        "licences = [\n"
        '    "MIT",\n'
        '    "Apache-2.0",\n'
        "]\n"
        "\n"
        "exceptions = [\n"
        '    "LLVM-exception",\n'
        "]\n"
        "\n"
        "asset_licences = [\n"
        '    "CC-BY-4.0",\n'
        "]\n"
        "\n"
        "asset_paths = [\n"
        '    "assets/**",\n'
        "]\n"
    ).encode("utf-8")

    clarify_ok = (
        "[[clarify]]\n"
        'package = "unlabelled"\n'
        'version = "0.2.0"\n'
        'expression = "MIT"\n'
        f'licence_file_sha256 = "{mit_sha}"\n'
    ).encode("utf-8")

    clarify_stale = (
        "[[clarify]]\n"
        'package = "unlabelled"\n'
        'version = "0.2.0"\n'
        'expression = "MIT"\n'
        f'licence_file_sha256 = "{stale_sha}"\n'
    ).encode("utf-8")

    icon_svg = b'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"></svg>\n'

    attr_ok = (
        "# Third-party assets\n"
        "\n"
        "| Path | Licence | Source |\n"
        "|---|---|---|\n"
        "| `assets/icons/ok.svg` | `CC-BY-4.0` | project |\n"
    ).encode("utf-8")

    attr_missing = (
        "# Third-party assets\n"
        "\n"
        "| Path | Licence | Source |\n"
        "|---|---|---|\n"
        "| `assets/icons/ok.svg` | `CC-BY-4.0` | project |\n"
    ).encode("utf-8")

    attr_orphan = (
        "# Third-party assets\n"
        "\n"
        "| Path | Licence | Source |\n"
        "|---|---|---|\n"
        "| `assets/icons/ok.svg` | `CC-BY-4.0` | project |\n"
        "| `assets/icons/deleted.svg` | `CC-BY-4.0` | project |\n"
    ).encode("utf-8")

    return {
        HERE / "allow.toml": allow_toml,
        HERE / "allow-with-llvm.toml": allow_with_llvm_toml,
        HERE / "license-mit.txt": mit_bytes,
        HERE / "clarify.toml": clarify_ok,
        HERE / "clarify-stale.toml": clarify_stale,
        HERE / "metadata-ok.json": make_metadata(
            [
                ws_pkg,
                pkg("dep-mit", "1.0.0", "MIT"),
                pkg("dep-apache", "2.1.0", "Apache-2.0"),
                pkg("dep-dual", "0.5.0", "MIT OR Apache-2.0"),
            ]
        ),
        HERE / "metadata-gpl.json": make_metadata(
            [
                ws_pkg,
                pkg("dep-mit", "1.0.0", "MIT"),
                pkg("bad-gpl", "3.0.1", "GPL-3.0-only"),
            ]
        ),
        HERE / "metadata-or.json": make_metadata(
            [
                ws_pkg,
                pkg("dual-gpl-mit", "1.2.3", "GPL-3.0-only OR MIT"),
            ]
        ),
        HERE / "metadata-and.json": make_metadata(
            [
                ws_pkg,
                pkg("both-mit-gpl", "1.2.3", "MIT AND GPL-3.0-only"),
            ]
        ),
        HERE / "metadata-nolicence.json": make_metadata(
            [
                ws_pkg,
                pkg("unlabelled", "0.2.0", None, license_file="license-mit.txt"),
            ]
        ),
        HERE / "metadata-with.json": make_metadata(
            [
                ws_pkg,
                pkg("llvm-dep", "0.9.0", "Apache-2.0 WITH LLVM-exception"),
            ]
        ),
        HERE / "tree-ok" / "ATTRIBUTION.md": attr_ok,
        HERE / "tree-ok" / "assets" / "icons" / "ok.svg": icon_svg,
        HERE / "tree-missing-attr" / "ATTRIBUTION.md": attr_missing,
        HERE / "tree-missing-attr" / "assets" / "icons" / "ok.svg": icon_svg,
        HERE / "tree-missing-attr" / "assets" / "icons" / "x.svg": icon_svg,
        HERE / "tree-orphan-attr" / "ATTRIBUTION.md": attr_orphan,
        HERE / "tree-orphan-attr" / "assets" / "icons" / "ok.svg": icon_svg,
    }


def main() -> int:
    files = build_fixtures()
    if "--check" in sys.argv[1:]:
        stale = []
        for path, want in sorted(files.items()):
            if not path.is_file() or path.read_bytes() != want:
                stale.append(str(path.relative_to(ROOT)))
        if stale:
            print(f"FAIL: stale licence fixtures: {', '.join(stale)}")
            return 1
        print("fixtures/lic: ok")
        return 0
    for path, want in sorted(files.items()):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(want)
    print(f"wrote {len(files)} fixture files under {HERE}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
